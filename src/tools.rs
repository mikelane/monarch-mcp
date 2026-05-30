//! Tool registry — registers the four compound tool names for `tools/list`.

use crate::cashflow_forecast::compute_forecast;
use crate::client::MonarchClient;
use crate::error::MonarchError;
use crate::financial_overview::compute_overview;
use crate::goals::Goals;
use crate::net_worth_trend::compute_trend;
use crate::progress_vs_goals::compute_progress;
use crate::recurring_scan::compute_scan;
use crate::spending_report::compute_spending_report;
use crate::triage::{
    build_category_suggestion_map, parse_raw_changes, partition_changeset, propose_changes,
};
use rmcp::schemars;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Deserialize;
use serde_json::json;

/// Input parameters for the `net_worth_trend` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NetWorthTrendParams {
    /// Number of months of history to include (1–24).
    pub months: u32,
}

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

    #[tool(
        description = "Return a snapshot of the household's current financial position: \
        net worth, month-over-month change, this-month cash flow (income/spending/net), \
        and balances by account type. Start every advising session here."
    )]
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

    #[tool(
        description = "Break down spending for a period by category, compare against \
        budget and the prior period, surface anomalies and over-budget flags."
    )]
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

    #[tool(
        description = "Identify uncategorized transactions and suggest category/tags/notes \
        based on the household's own history. Returns a proposed changeset for review — \
        nothing is written until apply_changeset is called."
    )]
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

    #[tool(
        description = "Apply an approved changeset, updating only category, tags, and notes. \
        Any other field (amount, account, merchant, date, or unknown fields) is forbidden — \
        entries containing them are rejected and reported back with the original transaction id. \
        The set of transaction ids is never altered."
    )]
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

    #[tool(
        description = "Project the household's month-end cash position from current account \
        balances, income and spending so far this period, and scheduled recurring charges. \
        Flags a shortfall when upcoming bills are on track to exceed available funds."
    )]
    async fn cashflow_forecast(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_forecast(&client).await {
            Ok(forecast) => serde_json::to_value(&forecast)
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

    #[tool(
        description = "Show net worth month-by-month over a requested period, broken down \
        by account type (depository, brokerage, credit, loan, etc.), with the biggest \
        single mover and a total assets-versus-liabilities split."
    )]
    async fn net_worth_trend(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(params): Parameters<NetWorthTrendParams>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_trend(&client, params.months).await {
            Ok(trend) => serde_json::to_value(&trend)
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

    #[tool(
        description = "Scan recurring charges for amount drift ('creeping' subscriptions \
        whose price has quietly changed) and list upcoming renewals due this period. \
        Stable subscriptions are reported but not flagged."
    )]
    async fn recurring_scan(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_scan(&client).await {
            Ok(scan) => serde_json::to_value(&scan)
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

    #[tool(
        description = "Measure actual finances against the household's remembered goals \
        (savings rate, emergency-fund runway, debt payoff). Reports each goal as \
        on-track, drifting, or off, with the lever to pull."
    )]
    async fn progress_vs_goals(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_progress(&client).await {
            Ok(progress) => serde_json::to_value(&progress)
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
}

impl Default for MonarchTools {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Date-range helpers
// ---------------------------------------------------------------------------

/// Returns (start, end) for the current calendar month as ISO-8601 strings.
fn current_month_range() -> (String, String) {
    // Use time crate-free approach: chrono is not a dep, so compute from
    // the system clock via std. We format YYYY-MM-01 for start and
    // YYYY-MM-{last_day} for end. Since we don't have chrono, we use a
    // simple approach: end = today and start = first of this month.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // seconds since epoch → rough calendar arithmetic
    let secs_per_day = 86_400u64;
    let days_since_epoch = now / secs_per_day;
    // 2000-01-01 was day 10_957 since epoch (Unix epoch = 1970-01-01)
    // Use a reference: 2024-01-01 = 19723 days since epoch
    // Simpler: ask the OS via formatted date string if available, else hard-code today.
    //
    // We use a helper that formats dates without chrono.
    let (year, month, _day) = days_to_ymd(days_since_epoch as i64);

    let start = format!("{year:04}-{month:02}-01");
    // For end, use the first of next month minus 1 day
    let last_day = days_in_month(year, month);
    let end = format!("{year:04}-{month:02}-{last_day:02}");
    (start, end)
}

/// Returns (start, end) for the prior calendar month.
fn prior_month_range() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days_since_epoch = (now / 86_400) as i64;
    let (mut year, mut month, _) = days_to_ymd(days_since_epoch);
    if month == 1 {
        year -= 1;
        month = 12;
    } else {
        month -= 1;
    }
    let last_day = days_in_month(year, month);
    let start = format!("{year:04}-{month:02}-01");
    let end = format!("{year:04}-{month:02}-{last_day:02}");
    (start, end)
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Proleptic Gregorian algorithm from Wikipedia
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

// ---------------------------------------------------------------------------
// Data fetching helpers — isolated so tool handlers stay readable
// ---------------------------------------------------------------------------

async fn fetch_and_compute(
    client: &MonarchClient,
) -> Result<crate::financial_overview::OverviewResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let (pri_start, pri_end) = prior_month_range();

    let (accounts, cashflow, history) = tokio::try_join!(
        client.get_accounts(),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
        client.get_net_worth_history(&pri_start, &pri_end),
    )?;
    Ok(compute_overview(&accounts, &cashflow, &history))
}

async fn fetch_and_compute_spending(
    client: &MonarchClient,
) -> Result<crate::spending_report::SpendingReport, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let (pri_start, pri_end) = prior_month_range();

    let (transactions, budgets, cashflow) = tokio::try_join!(
        client.get_transactions(&cur_start, &cur_end, 500),
        client.get_budgets(&cur_start, &cur_end),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
    )?;
    Ok(compute_spending_report(&transactions, &budgets, &cashflow))
}

async fn fetch_and_compute_triage(
    client: &MonarchClient,
) -> Result<crate::triage::TriageResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let (all_transactions, uncategorized) = tokio::try_join!(
        client.get_transactions(&cur_start, &cur_end, 500),
        client.get_transactions_needing_review(),
    )?;
    let suggestion_map = build_category_suggestion_map(&all_transactions);
    Ok(propose_changes(&uncategorized, &suggestion_map))
}

async fn fetch_and_compute_progress(
    client: &MonarchClient,
) -> Result<crate::progress_vs_goals::GoalsProgress, MonarchError> {
    let goals = Goals::load_from_env().map_err(|e| MonarchError::Internal(e.to_string()))?;
    let (cur_start, cur_end) = current_month_range();
    let (pri_start, pri_end) = prior_month_range();

    let (accounts, cashflow) = tokio::try_join!(
        client.get_accounts(),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
    )?;
    Ok(compute_progress(&goals, &accounts, &cashflow))
}

async fn fetch_and_compute_scan(
    client: &MonarchClient,
) -> Result<crate::recurring_scan::ScanResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let items = client.get_recurring_for_scan(&cur_start, &cur_end).await?;
    Ok(compute_scan(&items))
}

async fn fetch_and_compute_forecast(
    client: &MonarchClient,
) -> Result<crate::cashflow_forecast::ForecastResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();

    let (accounts, recurring) = tokio::try_join!(
        client.get_accounts(),
        client.get_recurring(&cur_start, &cur_end),
    )?;

    // Sum liquid (depository) account balances as the available cash position.
    // Credit/loan balances are negative and would distort the projection.
    let current_balance: f64 = accounts
        .iter()
        .filter(|a| a.account_type.name == "depository")
        .map(|a| a.current_balance)
        .sum();

    Ok(compute_forecast(current_balance, &recurring))
}

async fn fetch_and_compute_trend(
    client: &MonarchClient,
    months: u32,
) -> Result<crate::net_worth_trend::TrendResult, MonarchError> {
    // Build a start date `months` months back from today (first of that month).
    let start_date = months_ago_start(months);
    let snapshots = client.get_snapshots_by_account_type(&start_date).await?;
    Ok(compute_trend(&snapshots))
}

/// Compute the ISO date for the first day of the month that is `n` months before today.
///
/// Uses only integer arithmetic on the Unix epoch — no external date library needed.
fn months_ago_start(n: u32) -> String {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now_secs / 86_400) as i64;
    let (mut year, mut month, _) = epoch_days_to_ymd(today_days);

    for _ in 0..n {
        if month == 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
    }
    format!("{year:04}-{month:02}-01")
}

/// Convert a count of days since Unix epoch to (year, month, day).
///
/// Uses the Gregorian calendar algorithm from Howard Hinnant's date library.
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

async fn apply_approved_changeset(
    client: &MonarchClient,
    raw_changes: Vec<serde_json::Value>,
) -> Result<crate::triage::ApplyResult, MonarchError> {
    // Parse entries strictly: unknown fields and wrong types become RejectedChange
    // entries with the real transaction id preserved, never silent no-ops.
    let entries = parse_raw_changes(raw_changes);

    // Partition into allowed and forbidden entries — forbidden ones never reach the API.
    let (cur_start, cur_end) = current_month_range();
    let all_transactions = client.get_transactions(&cur_start, &cur_end, 500).await?;
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
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "monarch-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Monarch Money budgeting advisor. Tools: financial_overview, \
             spending_report, triage_uncategorized, progress_vs_goals, \
             cashflow_forecast, net_worth_trend, recurring_scan."
                    .to_string(),
            )
    }
}
