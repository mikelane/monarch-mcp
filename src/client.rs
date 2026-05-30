//! Monarch API client — auth, session persistence, and GraphQL transport.
//!
//! All knowledge of Monarch's HTTP API lives here. Nothing above this layer
//! touches reqwest or knows about HTTP status codes.
//!
//! # Token-from-env mode
//! When `MONARCH_TOKEN` is set the client skips interactive login and uses
//! the env-var value directly. The BDD harness relies on this.
//!
//! # Config isolation
//! Session files are written to `{config_dir}/monarch-mcp/session.json`, where
//! `config_dir` is resolved by [`config_dir`]. Tests set `MONARCH_CONFIG_DIR`
//! (or `XDG_CONFIG_HOME`) to a temp directory so they never touch the real
//! `~/.config/monarch-mcp/session.json`.
//!
//! # Session expiry
//! HTTP 401 from any authenticated endpoint is mapped to `MonarchError::SessionExpired`.
//!
//! # Real GraphQL operations (validated against live Monarch, ADR 0002)
//! - `GetAccounts`                — `accounts { id displayName currentBalance … }`
//! - `GetTransactionsList`        — `allTransactions { totalCount results { … } }`
//! - `Web_GetCashFlowPage`        — `summary: aggregates { summary { sumIncome sumExpense … } }`
//! - `GetAggregateSnapshots`      — `aggregateSnapshots { date balance }`
//! - `GetCategories`              — `categories { id name group { … } }`
//! - `GetHouseholdTransactionTags`— `householdTransactionTags { id name color … }`
//! - `GetJointPlanningData`       — `budgetData { monthlyAmountsByCategory { … } }`
//! - `Common_UpdateTransactionMutation` — `updateTransaction(input: {id, …})`

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
// Internal domain types — consumed by compute/aggregation functions.
//
// These shapes are STABLE: compute_overview, compute_spending_report, etc. all
// depend on them. The GraphQL parsing layer (further below) translates from
// Monarch's real response shapes into these internal types.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Account {
    #[allow(dead_code)]
    pub id: String,
    #[serde(rename = "displayName")]
    #[allow(dead_code)]
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

/// A transaction as consumed by spending_report.rs and triage.rs.
///
/// Populated from `GetTransactionsList → allTransactions.results[]`.
/// Monarch's real response has `merchant.name` (not `merchantName`) and
/// `tags` as objects with `{id, name, color, order}` (not bare strings).
#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub amount: f64,
    pub date: String,
    /// Populated from `merchant.name` in the Monarch response.
    pub merchant_name: String,
    pub category: Category,
    /// Tag names only — the compute layer only needs the name strings.
    #[allow(dead_code)]
    pub tags: Vec<String>,
    #[allow(dead_code)]
    pub notes: String,
    /// Whether this transaction is flagged for review (from `needsReview`).
    #[allow(dead_code)]
    pub needs_review: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Category {
    pub name: String,
}

/// A budget entry as consumed by spending_report.rs.
///
/// Populated from `GetJointPlanningData → budgetData.monthlyAmountsByCategory`.
/// Monarch returns category by ID only; we enrich with category name by joining
/// against the categories list fetched separately.
pub struct Budget {
    pub category: Category,
    /// `plannedCashFlowAmount` from the budget monthly amounts.
    ///
    /// Monarch stores expense/outflow budgets as **negative** values
    /// (e.g., "Loan Repayment" → `-1280`). Callers must use `amount.abs()`
    /// when comparing magnitudes. `spending_report.rs` does this via
    /// `percent_of_budget` and `find_over_budget_categories`.
    pub amount: f64,
}

/// Cashflow summary as consumed by financial_overview.rs and spending_report.rs.
///
/// Populated from `Web_GetCashFlowPage → summary[0].summary`.
/// `prior_month_spending` is derived by a second call with the prior-month
/// date range — Monarch has no single-call "prior month" comparison field.
pub struct Cashflow {
    /// `sumIncome` from the aggregates summary.
    pub income: f64,
    /// Absolute value of `sumExpense` (Monarch returns this as negative).
    pub spending: f64,
    /// Prior month's `sumExpense` (absolute), fetched separately.
    pub prior_month_spending: f64,
}

/// Net-worth snapshot as consumed by financial_overview.rs.
///
/// Populated from `GetAggregateSnapshots → aggregateSnapshots`.
/// `prior_month_net_worth` is the `balance` of the last snapshot in the
/// prior-month date window.
pub struct NetWorthHistory {
    pub prior_month_net_worth: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Raw GraphQL response deserialization helpers (Monarch's real shapes)
// ---------------------------------------------------------------------------

/// One item in `aggregateSnapshots[]`.
#[derive(Debug, Deserialize)]
struct AggregateSnapshot {
    #[allow(dead_code)]
    pub date: String,
    pub balance: f64,
}

/// `summary: aggregates(…, fillEmptyValues: true)[0].summary`
#[derive(Debug, Deserialize)]
struct CashflowSummaryRaw {
    #[serde(rename = "sumIncome")]
    pub sum_income: f64,
    #[serde(rename = "sumExpense")]
    pub sum_expense: f64,
}

/// One item from `allTransactions.results[]`.
#[derive(Debug, Deserialize)]
struct TransactionRaw {
    pub id: String,
    pub amount: f64,
    pub date: String,
    pub merchant: Option<MerchantRaw>,
    pub category: Option<CategoryRaw>,
    pub tags: Vec<TagRaw>,
    pub notes: Option<String>,
    #[serde(rename = "needsReview")]
    pub needs_review: bool,
}

#[derive(Debug, Deserialize)]
struct MerchantRaw {
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct CategoryRaw {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct TagRaw {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
}

/// One item from `budgetData.monthlyAmountsByCategory[]`.
#[derive(Debug, Deserialize)]
struct BudgetByCategoryRaw {
    pub category: BudgetCategoryRaw,
    #[serde(rename = "monthlyAmounts")]
    pub monthly_amounts: Vec<MonthlyAmountRaw>,
}

#[derive(Debug, Deserialize)]
struct BudgetCategoryRaw {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct MonthlyAmountRaw {
    #[serde(rename = "plannedCashFlowAmount")]
    pub planned_cash_flow_amount: f64,
}

// ---------------------------------------------------------------------------
// Session persistence
// ---------------------------------------------------------------------------

/// Resolve the config directory for session storage.
///
/// Priority: `MONARCH_CONFIG_DIR` env → `XDG_CONFIG_HOME` env → `~/.config`.
/// Tests set `MONARCH_CONFIG_DIR` to a temp path so they never touch
/// `~/.config/monarch-mcp/session.json`.
fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MONARCH_CONFIG_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(d) = std::env::var("XDG_CONFIG_HOME") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config")
}

fn session_path() -> PathBuf {
    config_dir().join("monarch-mcp").join("session.json")
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    token: String,
}

fn persist_token_to(path: &PathBuf, token: &str) -> Result<(), MonarchError> {
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| MonarchError::Internal(format!("create config dir: {e}")))?;
    let contents = serde_json::to_string(&SessionFile {
        token: token.to_string(),
    })
    .unwrap();
    std::fs::write(path, contents)
        .map_err(|e| MonarchError::Internal(format!("write session: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| MonarchError::Internal(format!("chmod session: {e}")))?;
    }
    Ok(())
}

/// Convenience wrapper — writes to the default config path (env-resolved).
/// Not used when the client is constructed via `new()` (which captures the
/// path at construction time), but kept for any future non-client callers.
#[allow(dead_code)]
fn persist_token(token: &str) -> Result<(), MonarchError> {
    persist_token_to(&session_path(), token)
}

fn load_token_from(path: &PathBuf) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<SessionFile>(&contents)
        .ok()
        .map(|s| s.token)
}

#[allow(dead_code)]
fn load_persisted_token() -> Option<String> {
    load_token_from(&session_path())
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
    /// Session file path — resolved once at construction from MONARCH_CONFIG_DIR /
    /// XDG_CONFIG_HOME / HOME so async calls don't race on env-var reads.
    session_file_path: PathBuf,
}

impl MonarchClient {
    /// Build a client. `base` defaults to `https://api.monarch.com` when `None`.
    ///
    /// The session file path is resolved **once at construction** from the current
    /// env so that concurrent tests setting `MONARCH_CONFIG_DIR` don't interfere
    /// with each other after the client is built.
    pub fn new(base: Option<String>) -> Self {
        Self::with_session_path(base, session_path())
    }

    /// Build a client with an explicit session file path.
    ///
    /// Used by tests to avoid env-var races: each test passes its own temp path
    /// directly rather than using `MONARCH_CONFIG_DIR`.
    pub fn with_session_path(base: Option<String>, session_file_path: PathBuf) -> Self {
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
            session_file_path,
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
        self.token = load_token_from(&self.session_file_path);
    }

    /// Return the current session token, or `None` if not authenticated.
    #[allow(dead_code)]
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
        // Use the path captured at construction time — avoids env-var races in tests.
        persist_token_to(&self.session_file_path, &token)
            .map_err(|e| LoginError::Http(e.to_string()))?;
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
    // Typed read operations — real Monarch GraphQL (see ADR 0002)
    // -----------------------------------------------------------------------

    pub async fn get_accounts(&self) -> Result<Vec<Account>, MonarchError> {
        let data = self
            .graphql(
                "GetAccounts",
                "query GetAccounts {
                  accounts {
                    id
                    displayName
                    currentBalance
                    isHidden
                    type { name display __typename }
                    subtype { name display __typename }
                    __typename
                  }
                }",
                json!({}),
            )
            .await?;
        let accounts: Vec<Account> = serde_json::from_value(data["accounts"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse accounts: {e}")))?;
        Ok(accounts)
    }

    /// Fetch transactions for a date range.
    ///
    /// `start_date` and `end_date` are ISO-8601 strings (`YYYY-MM-DD`).
    /// The real Monarch field is `allTransactions.results[]`, not a top-level
    /// `transactions` field (see ADR 0002).
    pub async fn get_transactions(
        &self,
        start_date: &str,
        end_date: &str,
        limit: u32,
    ) -> Result<Vec<Transaction>, MonarchError> {
        let data = self
            .graphql(
                "GetTransactionsList",
                "query GetTransactionsList($offset: Int, $limit: Int, $filters: TransactionFilterInput, $orderBy: TransactionOrdering) {
                  allTransactions(filters: $filters) {
                    totalCount
                    results(offset: $offset, limit: $limit, orderBy: $orderBy) {
                      id
                      amount
                      date
                      needsReview
                      notes
                      category { id name __typename }
                      merchant { name id __typename }
                      account { id displayName __typename }
                      tags { id name color order __typename }
                      __typename
                    }
                    __typename
                  }
                }",
                json!({
                    "offset": 0,
                    "limit": limit,
                    "orderBy": "date",
                    "filters": {
                        "search": "",
                        "categories": [],
                        "accounts": [],
                        "tags": [],
                        "startDate": start_date,
                        "endDate": end_date,
                    }
                }),
            )
            .await?;

        let results = &data["allTransactions"]["results"];
        let raw: Vec<TransactionRaw> = serde_json::from_value(results.clone())
            .map_err(|e| MonarchError::Internal(format!("parse transactions: {e}")))?;

        Ok(raw.into_iter().map(transaction_from_raw).collect())
    }

    /// Fetch transactions flagged as needing review.
    ///
    /// Uses the same `GetTransactionsList` operation with `needsReview: true`
    /// in the filter — Monarch has no separate root field for this (ADR 0002).
    pub async fn get_transactions_needing_review(&self) -> Result<Vec<Transaction>, MonarchError> {
        let data = self
            .graphql(
                "GetTransactionsList",
                "query GetTransactionsList($offset: Int, $limit: Int, $filters: TransactionFilterInput, $orderBy: TransactionOrdering) {
                  allTransactions(filters: $filters) {
                    totalCount
                    results(offset: $offset, limit: $limit, orderBy: $orderBy) {
                      id
                      amount
                      date
                      needsReview
                      notes
                      category { id name __typename }
                      merchant { name id __typename }
                      account { id displayName __typename }
                      tags { id name color order __typename }
                      __typename
                    }
                    __typename
                  }
                }",
                json!({
                    "offset": 0,
                    "limit": 100,
                    "orderBy": "date",
                    "filters": {
                        "search": "",
                        "categories": [],
                        "accounts": [],
                        "tags": [],
                        "needsReview": true,
                    }
                }),
            )
            .await?;

        let results = &data["allTransactions"]["results"];
        let raw: Vec<TransactionRaw> = serde_json::from_value(results.clone())
            .map_err(|e| MonarchError::Internal(format!("parse review txns: {e}")))?;

        Ok(raw.into_iter().map(transaction_from_raw).collect())
    }

    /// Fetch category budgets for a given month.
    ///
    /// Returns budgets enriched with category names by joining against the
    /// categories list. Uses `GetJointPlanningData` (real operation) not the
    /// invented `GetBudgets { budgets { category { name } amount } }` (ADR 0002).
    pub async fn get_budgets(&self, start_date: &str, end_date: &str) -> Result<Vec<Budget>, MonarchError> {
        // Fetch budgets (by category id) and categories (id→name) in parallel.
        let (budget_data, categories) = tokio::try_join!(
            self.graphql(
                "GetJointPlanningData",
                "query GetJointPlanningData($startDate: Date!, $endDate: Date!) {
                  budgetData(startMonth: $startDate, endMonth: $endDate) {
                    monthlyAmountsByCategory {
                      category { id __typename }
                      monthlyAmounts {
                        month
                        plannedCashFlowAmount
                        actualAmount
                        remainingAmount
                        __typename
                      }
                      __typename
                    }
                    __typename
                  }
                }",
                json!({"startDate": start_date, "endDate": end_date}),
            ),
            self.get_categories(),
        )?;

        // Build id→name lookup.
        let name_by_id: std::collections::HashMap<String, String> = categories
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect();

        let monthly_by_cat: Vec<BudgetByCategoryRaw> = serde_json::from_value(
            budget_data["budgetData"]["monthlyAmountsByCategory"].clone(),
        )
        .map_err(|e| MonarchError::Internal(format!("parse budgets: {e}")))?;

        // Take the first month's plannedCashFlowAmount for each category.
        let budgets = monthly_by_cat
            .into_iter()
            .filter_map(|entry| {
                let amount = entry
                    .monthly_amounts
                    .first()
                    .map(|m| m.planned_cash_flow_amount)
                    .unwrap_or(0.0);
                // Skip categories with no budget set.
                if amount == 0.0 {
                    return None;
                }
                let name = name_by_id
                    .get(&entry.category.id)
                    .cloned()
                    .unwrap_or_else(|| entry.category.id.clone());
                Some(Budget {
                    category: Category { name },
                    amount,
                })
            })
            .collect();

        Ok(budgets)
    }

    /// Fetch current-month cashflow summary.
    ///
    /// Income and spending come from `Web_GetCashFlowPage → summary[0].summary`.
    /// There is no `cashflow` root field in Monarch's real schema (ADR 0002).
    /// `sumExpense` is already negative — we store its absolute value as `spending`.
    ///
    /// Prior-month spending requires a second call with the prior-month date range.
    pub async fn get_cashflow(
        &self,
        start_date: &str,
        end_date: &str,
        prior_start: &str,
        prior_end: &str,
    ) -> Result<Cashflow, MonarchError> {
        let filters = json!({
            "startDate": start_date,
            "endDate": end_date,
            "search": "",
            "categories": [],
            "accounts": [],
            "tags": [],
        });
        let prior_filters = json!({
            "startDate": prior_start,
            "endDate": prior_end,
            "search": "",
            "categories": [],
            "accounts": [],
            "tags": [],
        });

        let query = "query Web_GetCashFlowPage($filters: TransactionFilterInput) {
          summary: aggregates(filters: $filters, fillEmptyValues: true) {
            summary {
              sumIncome
              sumExpense
              savings
              savingsRate
              __typename
            }
            __typename
          }
        }";

        let (current_data, prior_data) = tokio::try_join!(
            self.graphql("Web_GetCashFlowPage", query, json!({"filters": filters})),
            self.graphql("Web_GetCashFlowPage", query, json!({"filters": prior_filters})),
        )?;

        let current_raw: CashflowSummaryRaw = extract_cashflow_summary(&current_data)?;
        let prior_raw: CashflowSummaryRaw = extract_cashflow_summary(&prior_data)?;

        Ok(Cashflow {
            income: current_raw.sum_income,
            // sumExpense is negative in Monarch — store as positive spending
            spending: current_raw.sum_expense.abs(),
            prior_month_spending: prior_raw.sum_expense.abs(),
        })
    }

    pub async fn get_categories(&self) -> Result<Vec<CategoryWithId>, MonarchError> {
        let data = self
            .graphql(
                "GetCategories",
                "query GetCategories {
                  categories {
                    id
                    order
                    name
                    systemCategory
                    isSystemCategory
                    isDisabled
                    group { id name type __typename }
                    __typename
                  }
                }",
                json!({}),
            )
            .await?;

        let categories: Vec<CategoryWithId> = serde_json::from_value(data["categories"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse categories: {e}")))?;
        Ok(categories)
    }

    /// Fetch tags from `householdTransactionTags` (not the invented `tags` root field).
    #[allow(dead_code)]
    pub async fn get_tags(&self) -> Result<Vec<Tag>, MonarchError> {
        let data = self
            .graphql(
                "GetHouseholdTransactionTags",
                "query GetHouseholdTransactionTags($search: String, $limit: Int, $bulkParams: BulkTransactionDataParams) {
                  householdTransactionTags(search: $search, limit: $limit, bulkParams: $bulkParams) {
                    id
                    name
                    color
                    order
                    transactionCount
                    __typename
                  }
                }",
                json!({}),
            )
            .await?;

        let tags: Vec<Tag> = serde_json::from_value(data["householdTransactionTags"].clone())
            .map_err(|e| MonarchError::Internal(format!("parse tags: {e}")))?;
        Ok(tags)
    }

    /// Fetch daily net-worth snapshots and return the last balance in the window.
    ///
    /// Uses `GetAggregateSnapshots` — there is no `netWorthHistory` root field
    /// in Monarch's real schema (ADR 0002).
    pub async fn get_net_worth_history(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<NetWorthHistory, MonarchError> {
        let data = self
            .graphql(
                "GetAggregateSnapshots",
                "query GetAggregateSnapshots($filters: AggregateSnapshotFilters) {
                  aggregateSnapshots(filters: $filters) {
                    date
                    balance
                    __typename
                  }
                }",
                json!({"filters": {"startDate": start_date, "endDate": end_date}}),
            )
            .await?;

        let snapshots: Vec<AggregateSnapshot> =
            serde_json::from_value(data["aggregateSnapshots"].clone())
                .map_err(|e| MonarchError::Internal(format!("parse net worth snapshots: {e}")))?;

        // The prior-month net worth is the balance of the last snapshot in the window.
        let prior_month_net_worth = snapshots.last().map(|s| s.balance).unwrap_or(0.0);

        Ok(NetWorthHistory { prior_month_net_worth })
    }

    /// Apply a category/tags/notes change to a single transaction.
    ///
    /// Uses `Common_UpdateTransactionMutation` with the `input: {id, …}` shape
    /// validated against real Monarch (ADR 0002). Amount changes are rejected
    /// at the `triage.rs` parse layer before this is called.
    pub async fn update_transaction(
        &self,
        id: &str,
        category_id: Option<&str>,
        tag_ids: Option<Vec<String>>,
        notes: Option<&str>,
    ) -> Result<(), MonarchError> {
        let mut input = serde_json::Map::new();
        input.insert("id".to_string(), json!(id));
        if let Some(cid) = category_id {
            input.insert("categoryId".to_string(), json!(cid));
        }
        if let Some(tids) = tag_ids {
            input.insert("tagIds".to_string(), json!(tids));
        }
        if let Some(n) = notes {
            input.insert("notes".to_string(), json!(n));
        }

        self.graphql(
            "Common_UpdateTransactionMutation",
            "mutation Common_UpdateTransactionMutation($input: UpdateTransactionMutationInput!) {
              updateTransaction(input: $input) {
                transaction {
                  id
                  notes
                  category { id name __typename }
                  __typename
                }
                errors { message __typename }
                __typename
              }
            }",
            json!({"input": Value::Object(input)}),
        )
        .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper types — public so financial_overview.rs / spending_report.rs tests
// can construct Category with an id field when needed.
// ---------------------------------------------------------------------------

/// A category as returned by `GetCategories` — includes the `id` field that
/// the budget join logic needs. The simpler `Category` (name-only) is what
/// the compute layer uses after enrichment.
#[derive(Debug, Deserialize)]
pub struct CategoryWithId {
    pub id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Private parsing helpers
// ---------------------------------------------------------------------------

fn transaction_from_raw(raw: TransactionRaw) -> Transaction {
    Transaction {
        id: raw.id,
        amount: raw.amount,
        date: raw.date,
        merchant_name: raw.merchant.map(|m| m.name).unwrap_or_default(),
        category: Category {
            name: raw
                .category
                .map(|c| c.name)
                .unwrap_or_else(|| "Uncategorized".to_string()),
        },
        tags: raw.tags.into_iter().map(|t| t.name).collect(),
        notes: raw.notes.unwrap_or_default(),
        needs_review: raw.needs_review,
    }
}

fn extract_cashflow_summary(data: &Value) -> Result<CashflowSummaryRaw, MonarchError> {
    // `summary` is an alias for `aggregates(…, fillEmptyValues: true)` — it
    // returns an array; we want the first element's `.summary` sub-object.
    let summaries = data["summary"]
        .as_array()
        .ok_or_else(|| MonarchError::Internal("cashflow summary missing 'summary' array".to_string()))?;

    let first = summaries
        .first()
        .ok_or_else(|| MonarchError::Internal("cashflow summary array is empty".to_string()))?;

    serde_json::from_value(first["summary"].clone())
        .map_err(|e| MonarchError::Internal(format!("parse cashflow summary: {e}")))
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

        // Use with_session_path so the session write goes to an isolated temp
        // dir with no env-var races against concurrent tests.
        let tmp = tempfile::tempdir().unwrap();
        let session_file = tmp.path().join("monarch-mcp").join("session.json");
        let mut client = MonarchClient::with_session_path(Some(server.uri()), session_file);
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
    //
    // Does NOT use set_var (env vars race in parallel tokio tests). Instead
    // we verify the token returned by login_totp directly; session persistence
    // is tested separately via persist_and_load_token_roundtrip_in_isolated_dir.
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

        // Use with_session_path — no env-var races with concurrent tests.
        let tmp = tempfile::tempdir().unwrap();
        let session_file = tmp.path().join("monarch-mcp").join("session.json");
        let mut client = MonarchClient::with_session_path(Some(server.uri()), session_file);
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
    // GetAccounts: real Monarch response shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_accounts_parses_real_response_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "accounts": [
                        {
                            "id": "acct-1",
                            "displayName": "Checking",
                            "currentBalance": 2500.00,
                            "isHidden": false,
                            "type": {"name": "depository", "display": "Depository", "__typename": "AccountType"},
                            "subtype": {"name": "checking", "display": "Checking", "__typename": "AccountSubtype"},
                            "__typename": "Account"
                        },
                        {
                            "id": "acct-2",
                            "displayName": "Credit Card",
                            "currentBalance": -800.00,
                            "isHidden": false,
                            "type": {"name": "credit", "display": "Credit Card", "__typename": "AccountType"},
                            "subtype": {"name": "credit_card", "display": "Credit Card", "__typename": "AccountSubtype"},
                            "__typename": "Account"
                        }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let accounts = client.get_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].display_name, "Checking");
        assert_eq!(accounts[0].current_balance, 2500.00);
        assert_eq!(accounts[1].current_balance, -800.00);
        assert_eq!(accounts[1].account_type.name, "credit");
    }

    // -----------------------------------------------------------------------
    // GetAccounts: empty list
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
    // GetTransactionsList: real allTransactions.results[] shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_transactions_parses_real_all_transactions_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "allTransactions": {
                        "totalCount": 1,
                        "results": [
                            {
                                "id": "txn-1",
                                "amount": -45.50,
                                "date": "2026-05-15",
                                "needsReview": false,
                                "notes": "lunch",
                                "category": {"id": "cat-1", "name": "Dining", "__typename": "Category"},
                                "merchant": {"name": "ACME Cafe", "id": "m-1", "__typename": "Merchant"},
                                "account": {"id": "acct-1", "displayName": "Checking", "__typename": "Account"},
                                "tags": [{"id": "tag-1", "name": "business", "color": "#fff", "order": 1, "__typename": "TransactionTag"}],
                                "__typename": "Transaction"
                            }
                        ],
                        "__typename": "TransactionList"
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let txns = client.get_transactions("2026-05-01", "2026-05-31", 100).await.unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].merchant_name, "ACME Cafe");
        assert_eq!(txns[0].amount, -45.50);
        assert_eq!(txns[0].category.name, "Dining");
        assert_eq!(txns[0].tags, vec!["business"]);
        assert_eq!(txns[0].notes, "lunch");
        assert!(!txns[0].needs_review);
    }

    // -----------------------------------------------------------------------
    // get_transactions_needing_review: filters on needsReview=true
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_transactions_needing_review_uses_needsreview_filter() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "allTransactions": {
                        "totalCount": 1,
                        "results": [
                            {
                                "id": "txn-review",
                                "amount": -25.00,
                                "date": "2026-05-20",
                                "needsReview": true,
                                "notes": "",
                                "category": {"id": "cat-0", "name": "Uncategorized", "__typename": "Category"},
                                "merchant": {"name": "Unknown Store", "id": "m-2", "__typename": "Merchant"},
                                "account": {"id": "acct-1", "displayName": "Checking", "__typename": "Account"},
                                "tags": [],
                                "__typename": "Transaction"
                            }
                        ],
                        "__typename": "TransactionList"
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let txns = client.get_transactions_needing_review().await.unwrap();
        assert_eq!(txns.len(), 1);
        assert!(txns[0].needs_review);
        assert_eq!(txns[0].merchant_name, "Unknown Store");
    }

    // -----------------------------------------------------------------------
    // Web_GetCashFlowPage: real summary shape
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_cashflow_parses_real_aggregate_summary_shape() {
        let server = MockServer::start().await;
        // Both current and prior month calls get the same mock response
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "summary": [
                        {
                            "summary": {
                                "sumIncome": 8000.00,
                                "sumExpense": -6500.00,
                                "savings": 1500.00,
                                "savingsRate": 0.1875,
                                "__typename": "AggregateSummary"
                            },
                            "__typename": "Aggregate"
                        }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let cf = client
            .get_cashflow("2026-05-01", "2026-05-31", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(cf.income, 8000.00);
        // sumExpense is -6500 in Monarch; we store absolute value
        assert_eq!(cf.spending, 6500.00);
        // prior month same mock → prior_month_spending also 6500
        assert_eq!(cf.prior_month_spending, 6500.00);
    }

    // -----------------------------------------------------------------------
    // GetAggregateSnapshots: last snapshot becomes prior_month_net_worth
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_net_worth_history_uses_last_snapshot_balance() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "aggregateSnapshots": [
                        {"date": "2026-04-01", "balance": 65000.00, "__typename": "AggregateSnapshot"},
                        {"date": "2026-04-15", "balance": 66000.00, "__typename": "AggregateSnapshot"},
                        {"date": "2026-04-30", "balance": 68500.00, "__typename": "AggregateSnapshot"},
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let h = client.get_net_worth_history("2026-04-01", "2026-04-30").await.unwrap();
        assert_eq!(h.prior_month_net_worth, 68500.00);
    }

    // -----------------------------------------------------------------------
    // GetHouseholdTransactionTags: real field name
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_tags_parses_household_transaction_tags_field() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "householdTransactionTags": [
                        {"id": "t1", "name": "business", "color": "#19D2A5", "order": 1, "transactionCount": 12, "__typename": "TransactionTag"},
                        {"id": "t2", "name": "personal", "color": "#FF5733", "order": 2, "transactionCount": 5, "__typename": "TransactionTag"},
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let tags = client.get_tags().await.unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "business");
        assert_eq!(tags[1].name, "personal");
    }

    // -----------------------------------------------------------------------
    // GraphQL: errors array maps to GraphQL error variant
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
    // Session isolation: persist_token_to / load_token_from work with an
    // arbitrary path — no env var needed, no parallel-test races.
    // -----------------------------------------------------------------------

    #[test]
    fn persist_and_load_token_roundtrip_in_isolated_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let session_file = tmp.path().join("monarch-mcp").join("session.json");

        persist_token_to(&session_file, "isolated-tok-roundtrip")
            .expect("persist should succeed");

        assert!(session_file.exists(), "session.json must exist after persist");

        let loaded = load_token_from(&session_file)
            .expect("should load the token we just wrote");

        assert_eq!(loaded, "isolated-tok-roundtrip");
    }
}
