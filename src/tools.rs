//! Tool registry — registers the four compound tool names for `tools/list`.

use crate::client::MonarchClient;
use crate::error::MonarchError;
use crate::financial_overview::compute_overview;
use crate::spending_report::compute_spending_report;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::*,
    service::RequestContext,
    tool, tool_router,
};
use serde_json::json;

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
        Err(McpError::internal_error(
            "triage_uncategorized is not yet implemented (issue A6)",
            None,
        ))
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
