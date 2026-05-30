//! Monarch API client — auth, session persistence, and GraphQL transport.
#![allow(dead_code)] // Public API consumed by A4–A7 tool implementations
//!
//! All knowledge of Monarch's HTTP API lives here. Nothing above this layer
//! touches reqwest or knows about HTTP status codes.
//!
//! # Token-from-env mode
//! When `MONARCH_TOKEN` is set the client skips interactive login and uses
//! the env-var value directly. The BDD harness relies on this.
//!
//! # Session expiry
//! HTTP 401 from any authenticated endpoint is mapped to `MonarchError::SessionExpired`.

use crate::error::MonarchError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_BASE: &str = "https://api.monarch.com";
const DEFAULT_ORIGIN: &str = "https://app.monarch.com";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

// ---------------------------------------------------------------------------
// Response types — shaped to match bdd/mock_monarch/server.py exactly
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Account {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "currentBalance")]
    pub current_balance: f64,
    #[serde(rename = "type")]
    pub account_type: AccountType,
}

#[derive(Debug, Deserialize)]
pub struct AccountType {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub amount: f64,
    pub date: String,
    #[serde(rename = "merchantName")]
    pub merchant_name: String,
    pub category: Category,
    pub tags: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct Category {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Budget {
    pub category: Category,
    pub amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct Cashflow {
    pub income: f64,
    pub spending: f64,
    pub prior_month_spending: f64,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NetWorthHistory {
    #[serde(rename = "priorMonthNetWorth")]
    pub prior_month_net_worth: f64,
}

// ---------------------------------------------------------------------------
// Session persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    token: String,
}

fn session_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("monarch-mcp")
        .join("session.json")
}

fn persist_token(token: &str) -> Result<(), MonarchError> {
    let path = session_path();
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| MonarchError::Internal(format!("create config dir: {e}")))?;
    let contents = serde_json::to_string(&SessionFile {
        token: token.to_string(),
    })
    .unwrap();
    std::fs::write(&path, contents)
        .map_err(|e| MonarchError::Internal(format!("write session: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| MonarchError::Internal(format!("chmod session: {e}")))?;
    }
    Ok(())
}

fn load_persisted_token() -> Option<String> {
    let path = session_path();
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<SessionFile>(&contents)
        .ok()
        .map(|s| s.token)
}

// ---------------------------------------------------------------------------
// MonarchClient
// ---------------------------------------------------------------------------

/// HTTP + GraphQL client for the Monarch Money API.
pub struct MonarchClient {
    http: Client,
    base: String,
    origin: String,
    device_uuid: String,
    /// The active session token. `None` until `authenticate()` is called.
    token: Option<String>,
}

impl MonarchClient {
    /// Build a client. `base` defaults to `https://api.monarch.com` when `None`.
    pub fn new(base: Option<String>) -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_store(true)
            .build()
            .expect("building reqwest client");

        let base = base.unwrap_or_else(|| DEFAULT_BASE.to_string());
        let device_uuid = Uuid::new_v4().to_string();

        MonarchClient {
            http,
            base,
            origin: DEFAULT_ORIGIN.to_string(),
            device_uuid,
            token: None,
        }
    }

    /// Resolve the active token, preferring env var → persisted file → None.
    ///
    /// When `MONARCH_TOKEN` is set the client uses it directly and skips the
    /// interactive login flow. The BDD harness relies on this.
    pub fn resolve_token_from_env_or_disk(&mut self) {
        if let Ok(t) = std::env::var("MONARCH_TOKEN") {
            if !t.is_empty() {
                self.token = Some(t);
                return;
            }
        }
        self.token = load_persisted_token();
    }

    /// Return the current session token, or `None` if not authenticated.
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    // -----------------------------------------------------------------------
    // Auth
    // -----------------------------------------------------------------------

    /// Attempt password-only login. Returns the token on success.
    /// Returns `Err` containing the HTTP status and body on failure so the
    /// caller can decide whether to retry with TOTP.
    pub async fn login_password(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<String, LoginError> {
        let body = json!({
            "username": username,
            "password": password,
            "trusted_device": true,
            "supports_mfa": true,
        });
        self.post_login(body).await
    }

    /// Re-attempt login with a TOTP code after a 403 MFA challenge.
    pub async fn login_totp(
        &mut self,
        username: &str,
        password: &str,
        totp: &str,
    ) -> Result<String, LoginError> {
        let body = json!({
            "username": username,
            "password": password,
            "trusted_device": true,
            "supports_mfa": true,
            "totp": totp,
        });
        self.post_login(body).await
    }

    async fn post_login(&mut self, body: Value) -> Result<String, LoginError> {
        let url = format!("{}/auth/login/", self.base);
        let resp = self
            .http
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Client-Platform", "web")
            .header("Origin", &self.origin)
            .header("device-uuid", &self.device_uuid)
            .json(&body)
            .send()
            .await
            .map_err(|e| LoginError::Http(e.to_string()))?;

        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

        if status == 403 {
            return Err(LoginError::MfaRequired);
        }
        if status == 401 {
            return Err(LoginError::Unauthorized(text));
        }
        if !matches!(status, 200..=299) {
            return Err(LoginError::Http(format!("HTTP {status}: {text}")));
        }

        // Extract the token from the response
        let token = v
            .get("token")
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| LoginError::NoToken(text.clone()))?
            .to_string();

        self.token = Some(token.clone());
        persist_token(&token).map_err(|e| LoginError::Http(e.to_string()))?;
        Ok(token)
    }

    // -----------------------------------------------------------------------
    // GraphQL transport
    // -----------------------------------------------------------------------

    async fn graphql(&self, operation: &str, query: &str, variables: Value) -> Result<Value, MonarchError> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| MonarchError::Internal("not authenticated".to_string()))?;

        let url = format!("{}/graphql", self.base);
        let payload = json!({
            "operationName": operation,
            "variables": variables,
            "query": query,
        });

        let resp = self
            .http
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Client-Platform", "web")
            .header("Origin", &self.origin)
            .header("device-uuid", &self.device_uuid)
            .header("Authorization", format!("Token {token}"))
            .json(&payload)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();

        if status == 401 {
            return Err(MonarchError::SessionExpired);
        }

        let v: Value = serde_json::from_str(&text)
            .map_err(|e| MonarchError::Internal(format!("non-JSON response: {e}")))?;

        if let Some(errors) = v.get("errors") {
            return Err(MonarchError::GraphQL(errors.to_string()));
        }

        Ok(v["data"].clone())
    }

    // -----------------------------------------------------------------------
    // Typed read operations — operationNames must match mock_monarch/server.py
    // -----------------------------------------------------------------------

    pub async fn get_accounts(&self) -> Result<Vec<Account>, MonarchError> {
        let data = self
            .graphql(
                "GetAccounts",
                "query GetAccounts { accounts { id displayName currentBalance type { name } } }",
                json!({}),
            )
            .await?;
        let accounts: Vec<Account> = serde_json::from_value(data["accounts"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse accounts: {e}")))?;
        Ok(accounts)
    }

    pub async fn get_transactions(&self) -> Result<Vec<Transaction>, MonarchError> {
        let data = self
            .graphql(
                "GetTransactions",
                "query GetTransactions { transactions { id amount date merchantName category { name } tags notes } }",
                json!({}),
            )
            .await?;
        let txns: Vec<Transaction> = serde_json::from_value(data["transactions"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse transactions: {e}")))?;
        Ok(txns)
    }

    pub async fn get_transactions_needing_review(&self) -> Result<Vec<Transaction>, MonarchError> {
        let data = self
            .graphql(
                "GetTransactionsNeedingReview",
                "query GetTransactionsNeedingReview { transactionsNeedingReview { id amount date merchantName category { name } tags notes } }",
                json!({}),
            )
            .await?;
        let txns: Vec<Transaction> =
            serde_json::from_value(data["transactionsNeedingReview"].clone())
                .map_err(|e| MonarchError::Internal(format!("parse review txns: {e}")))?;
        Ok(txns)
    }

    pub async fn get_budgets(&self) -> Result<Vec<Budget>, MonarchError> {
        let data = self
            .graphql(
                "GetBudgets",
                "query GetBudgets { budgets { category { name } amount } }",
                json!({}),
            )
            .await?;
        let budgets: Vec<Budget> = serde_json::from_value(data["budgets"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse budgets: {e}")))?;
        Ok(budgets)
    }

    pub async fn get_cashflow(&self) -> Result<Cashflow, MonarchError> {
        let data = self
            .graphql(
                "GetCashflow",
                "query GetCashflow { cashflow { income spending prior_month_spending } }",
                json!({}),
            )
            .await?;
        let cashflow: Cashflow = serde_json::from_value(data["cashflow"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse cashflow: {e}")))?;
        Ok(cashflow)
    }

    pub async fn get_categories(&self) -> Result<Vec<Category>, MonarchError> {
        let data = self
            .graphql(
                "GetCategories",
                "query GetCategories { categories { id name } }",
                json!({}),
            )
            .await?;
        let categories: Vec<Category> = serde_json::from_value(data["categories"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse categories: {e}")))?;
        Ok(categories)
    }

    pub async fn get_tags(&self) -> Result<Vec<Tag>, MonarchError> {
        let data = self
            .graphql(
                "GetTags",
                "query GetTags { tags { id name } }",
                json!({}),
            )
            .await?;
        let tags: Vec<Tag> = serde_json::from_value(data["tags"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse tags: {e}")))?;
        Ok(tags)
    }

    pub async fn get_net_worth_history(&self) -> Result<NetWorthHistory, MonarchError> {
        let data = self
            .graphql(
                "GetNetWorthHistory",
                "query GetNetWorthHistory { netWorthHistory { priorMonthNetWorth } }",
                json!({}),
            )
            .await?;
        let history: NetWorthHistory =
            serde_json::from_value(data["netWorthHistory"].clone())
                .map_err(|e| MonarchError::Internal(format!("parse net worth history: {e}")))?;
        Ok(history)
    }

    /// Apply a category/tags/notes change to a single transaction.
    /// Amount changes are not permitted — the mock rejects them.
    pub async fn update_transaction(
        &self,
        id: &str,
        category: Option<&str>,
        tags: Option<Vec<String>>,
        notes: Option<&str>,
    ) -> Result<(), MonarchError> {
        let mut vars = serde_json::Map::new();
        vars.insert("id".to_string(), json!(id));
        if let Some(c) = category {
            vars.insert("category".to_string(), json!(c));
        }
        if let Some(t) = tags {
            vars.insert("tags".to_string(), json!(t));
        }
        if let Some(n) = notes {
            vars.insert("notes".to_string(), json!(n));
        }
        self.graphql(
            "UpdateTransaction",
            "mutation UpdateTransaction($id: ID!, $category: String, $tags: [String], $notes: String) { updateTransaction(id: $id, category: $category, tags: $tags, notes: $notes) { id category { name } } }",
            Value::Object(vars),
        )
        .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LoginError — used only for the two-step login flow
// ---------------------------------------------------------------------------

/// Errors specific to the login flow. Callers use this to decide whether to
/// prompt for a TOTP code before converting to `MonarchError`.
#[derive(Debug)]
pub enum LoginError {
    /// Monarch returned 403 — MFA code required.
    MfaRequired,
    /// Monarch returned 401 — credentials rejected.
    Unauthorized(String),
    /// The response did not contain a token.
    NoToken(String),
    /// Network or HTTP error.
    Http(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::MfaRequired => write!(f, "MFA required"),
            LoginError::Unauthorized(b) => write!(f, "unauthorized: {b}"),
            LoginError::NoToken(b) => write!(f, "no token in response: {b}"),
            LoginError::Http(s) => write!(f, "HTTP error: {s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(base: &str) -> MonarchClient {
        let mut c = MonarchClient::new(Some(base.to_string()));
        c.token = Some("test-token".to_string());
        c
    }

    // -----------------------------------------------------------------------
    // Auth: password-only success
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn login_password_success_captures_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "tok-abc"})))
            .mount(&server)
            .await;

        let mut client = MonarchClient::new(Some(server.uri()));
        let token = client.login_password("user@example.com", "secret").await.unwrap();
        assert_eq!(token, "tok-abc");
        assert_eq!(client.token(), Some("tok-abc"));
    }

    // -----------------------------------------------------------------------
    // Auth: 403 → MfaRequired
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn login_returns_mfa_required_on_403() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login/"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"detail": "MFA required"})))
            .mount(&server)
            .await;

        let mut client = MonarchClient::new(Some(server.uri()));
        let err = client.login_password("user@example.com", "secret").await.unwrap_err();
        assert!(matches!(err, LoginError::MfaRequired), "got: {err:?}");
    }

    // -----------------------------------------------------------------------
    // Auth: 403 → retry with TOTP succeeds
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn login_totp_retry_succeeds_after_mfa_challenge() {
        let server = MockServer::start().await;
        // First call (password only) → 403
        Mock::given(method("POST"))
            .and(path("/auth/login/"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"detail": "MFA"})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        // Second call (with totp) → 200
        Mock::given(method("POST"))
            .and(path("/auth/login/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "tok-mfa"})))
            .mount(&server)
            .await;

        let mut client = MonarchClient::new(Some(server.uri()));
        let err = client.login_password("u", "p").await.unwrap_err();
        assert!(matches!(err, LoginError::MfaRequired));

        let token = client.login_totp("u", "p", "123456").await.unwrap();
        assert_eq!(token, "tok-mfa");
    }

    // -----------------------------------------------------------------------
    // Token-from-env mode skips login
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_token_prefers_env_var() {
        unsafe { std::env::set_var("MONARCH_TOKEN", "env-token-xyz") };
        let mut client = MonarchClient::new(None);
        client.resolve_token_from_env_or_disk();
        assert_eq!(client.token(), Some("env-token-xyz"));
        unsafe { std::env::remove_var("MONARCH_TOKEN") };
    }

    // -----------------------------------------------------------------------
    // GraphQL: HTTP 401 → SessionExpired
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn graphql_401_maps_to_session_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({"detail": "Authentication credentials were not provided."})),
            )
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let err = client.get_accounts().await.unwrap_err();
        assert!(
            matches!(err, MonarchError::SessionExpired),
            "expected SessionExpired, got: {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // GraphQL: GetAccounts response shape matches mock
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_accounts_parses_mock_response_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "accounts": [
                        {"id": "1", "displayName": "Checking", "currentBalance": 1000.0, "type": {"name": "checking"}},
                        {"id": "2", "displayName": "Credit Card", "currentBalance": -500.0, "type": {"name": "credit"}},
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let accounts = client.get_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].display_name, "Checking");
        assert_eq!(accounts[0].current_balance, 1000.0);
        assert_eq!(accounts[1].account_type.name, "credit");
    }

    // -----------------------------------------------------------------------
    // TRIANGULATE: GetAccounts with empty list
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_accounts_handles_empty_list() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"accounts": []}
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let accounts = client.get_accounts().await.unwrap();
        assert!(accounts.is_empty());
    }

    // -----------------------------------------------------------------------
    // GraphQL: GetTransactionsNeedingReview response shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_transactions_needing_review_parses_mock_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "transactionsNeedingReview": [
                        {
                            "id": "t1",
                            "amount": 42.50,
                            "date": "2026-05-15",
                            "merchantName": "ACME",
                            "category": {"name": "Uncategorized"},
                            "tags": [],
                            "notes": ""
                        }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let txns = client.get_transactions_needing_review().await.unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].merchant_name, "ACME");
        assert_eq!(txns[0].amount, 42.50);
    }

    // -----------------------------------------------------------------------
    // GraphQL: GetCashflow response shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_cashflow_parses_mock_response_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "cashflow": {
                        "income": 8000.0,
                        "spending": 6500.0,
                        "prior_month_spending": 6000.0
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let cf = client.get_cashflow().await.unwrap();
        assert_eq!(cf.income, 8000.0);
        assert_eq!(cf.spending, 6500.0);
        assert_eq!(cf.prior_month_spending, 6000.0);
    }

    // -----------------------------------------------------------------------
    // TRIANGULATE: GraphQL errors array maps to GraphQL error variant
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn graphql_errors_array_maps_to_graphql_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errors": [{"message": "Unknown operation: BadOp"}]
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let err = client.get_accounts().await.unwrap_err();
        assert!(
            matches!(err, MonarchError::GraphQL(_)),
            "expected GraphQL error, got: {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // GetNetWorthHistory response shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_net_worth_history_parses_mock_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "netWorthHistory": {
                        "priorMonthNetWorth": 68000.0
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let h = client.get_net_worth_history().await.unwrap();
        assert_eq!(h.prior_month_net_worth, 68000.0);
    }
}
