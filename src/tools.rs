//! Tool registry — registers the four compound tool names for `tools/list`.
//!
//! Tool logic is not implemented yet (issues A4–A7). Each handler returns an
//! honest "not implemented" MCP error so the BDD harness can confirm the
//! binary launches and handshakes before the tool bodies exist.

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::*,
    service::RequestContext,
    tool, tool_router,
};

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
        Err(McpError::internal_error(
            "financial_overview is not yet implemented (issue A4)",
            None,
        ))
    }

    #[tool(description = "Break down spending for a period by category, compare against \
        budget and the prior period, surface anomalies and over-budget flags.")]
    async fn spending_report(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::internal_error(
            "spending_report is not yet implemented (issue A5)",
            None,
        ))
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
