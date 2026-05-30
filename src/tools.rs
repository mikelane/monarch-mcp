//! Tool registry — registers the four compound tool names for `tools/list`.

use crate::client::MonarchClient;
use crate::error::MonarchError;
use crate::financial_overview::compute_overview;
use crate::spending_report::compute_spending_report;
use crate::triage::{build_category_suggestion_map, partition_changeset, propose_changes, ChangeEntry};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_router,
};
use rmcp::schemars;
use serde::Deserialize;
use serde_json::json;

/// Input parameters for the `apply_changeset` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyChangesetParams {
    /// List of change entries to apply. Each entry may include id, category, tags, notes.
    /// Entries containing forbidden fields (e.g. amount) are rejected and reported.
    pub changes: Vec<serde_json::Value>,
}

#[derive(Clone)]
pub struct MonarchTools {
    #[allow(dead_code)] // required by rmcp tool_router macro
    tool_router: ToolRouter<MonarchTools>,
}

#[tool_router]
impl MonarchTools {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Return a snapshot of the household's current financial position: \
        net worth, month-over-month change, this-month cash flow (income/spending/net), \
        and balances by account type. Start every advising session here.")]
    async fn financial_overview(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute(&client).await {
            Ok(overview) => serde_json::to_value(&overview)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            Err(MonarchError::SessionExpired) => {
                json!({
                    "error": "Session expired — re-authenticate by running `monarch-mcp login`"
                })
            }
            Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&payload)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Break down spending for a period by category, compare against \
        budget and the prior period, surface anomalies and over-budget flags.")]
    async fn spending_report(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_spending(&client).await {
            Ok(report) => serde_json::to_value(&report)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            Err(MonarchError::SessionExpired) => {
                json!({
                    "error": "Session expired — re-authenticate by running `monarch-mcp login`"
                })
            }
            Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&payload)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Identify uncategorized transactions and suggest category/tags/notes \
        based on the household's own history. Returns a proposed changeset for review — \
        nothing is written until apply_changeset is called.")]
    async fn triage_uncategorized(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_triage(&client).await {
            Ok(result) => serde_json::to_value(&result)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            Err(MonarchError::SessionExpired) => {
                json!({
                    "error": "Session expired — re-authenticate by running `monarch-mcp login`"
                })
            }
            Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&payload)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Apply an approved changeset, updating only category, tags, and notes. \
        Amount, merchant, and date fields are forbidden — entries containing them are rejected \
        and reported back. The set of transaction ids is never altered.")]
    async fn apply_changeset(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(ApplyChangesetParams { changes }): Parameters<ApplyChangesetParams>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match apply_approved_changeset(&client, changes).await {
            Ok(result) => serde_json::to_value(&result)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            Err(MonarchError::SessionExpired) => {
                json!({
                    "error": "Session expired — re-authenticate by running `monarch-mcp login`"
                })
            }
            Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&payload)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Measure actual finances against the household's remembered goals \
        (savings rate, emergency-fund runway, debt payoff). Reports each goal as \
        on-track, drifting, or off, with the lever to pull.")]
    async fn progress_vs_goals(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::internal_error(
            "progress_vs_goals is not yet implemented (issue A7)",
            None,
        ))
    }
}

// ---------------------------------------------------------------------------
// Data fetching helper — isolated so the tool handler stays readable
// ---------------------------------------------------------------------------

async fn fetch_and_compute(
    client: &MonarchClient,
) -> Result<crate::financial_overview::OverviewResult, MonarchError> {
    let accounts = client.get_accounts().await?;
    let cashflow = client.get_cashflow().await?;
    let history = client.get_net_worth_history().await?;
    Ok(compute_overview(&accounts, &cashflow, &history))
}

async fn fetch_and_compute_spending(
    client: &MonarchClient,
) -> Result<crate::spending_report::SpendingReport, MonarchError> {
    let transactions = client.get_transactions().await?;
    let budgets = client.get_budgets().await?;
    let cashflow = client.get_cashflow().await?;
    Ok(compute_spending_report(&transactions, &budgets, &cashflow))
}

async fn fetch_and_compute_triage(
    client: &MonarchClient,
) -> Result<crate::triage::TriageResult, MonarchError> {
    let all_transactions = client.get_transactions().await?;
    let uncategorized = client.get_transactions_needing_review().await?;
    let suggestion_map = build_category_suggestion_map(&all_transactions);
    Ok(propose_changes(&uncategorized, &suggestion_map))
}

async fn apply_approved_changeset(
    client: &MonarchClient,
    raw_changes: Vec<serde_json::Value>,
) -> Result<crate::triage::ApplyResult, MonarchError> {
    // Parse and validate entries before touching the API.
    let entries: Vec<ChangeEntry> = raw_changes
        .into_iter()
        .map(|v| serde_json::from_value(v).unwrap_or(ChangeEntry {
            id: None,
            merchant: None,
            category: None,
            tags: None,
            notes: None,
            amount: None,
        }))
        .collect();

    // Partition into allowed and forbidden entries — forbidden ones never reach the API.
    let all_transactions = client.get_transactions().await?;
    let total_count = all_transactions.len();
    let result = partition_changeset(&entries, total_count);

    // Send only the allowed changes to the Monarch API.
    for change in &result.applied_changes {
        client
            .update_transaction(
                &change.id,
                change.category.as_deref(),
                change.tags.clone(),
                change.notes.as_deref(),
            )
            .await?;
    }

    Ok(result)
}

#[rmcp::tool_handler]
impl ServerHandler for MonarchTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_server_info(Implementation::new("monarch-mcp", env!("CARGO_PKG_VERSION")))
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_instructions(
            "Monarch Money budgeting advisor. Tools: financial_overview, \
             spending_report, triage_uncategorized, progress_vs_goals."
                .to_string(),
        )
    }
}
