//! Tool registry — registers the four compound tool names for `tools/list`.

use crate::account_inventory::compute_account_inventory;
use crate::cashflow_forecast::compute_forecast;
use crate::client::MonarchClient;
use crate::error::MonarchError;
use crate::financial_overview::compute_overview;
use crate::goals::Goals;
use crate::inspect_transactions::{blank_to_none, compute_inspection, InspectFilter};
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

/// Input parameters for the `inspect_transactions` tool.
///
/// All fields are optional. Omitting a field means "no filter on that dimension".
/// If neither `start_date` nor `end_date` is provided, the current calendar month
/// is used. To re-categorize a transaction found here, pass its `id` to
/// `apply_changeset`:
///
/// ```text
/// Step 1: inspect_transactions(category="Pets")
///   → [{id:"txn-abc", merchant:"Petco", amount:-12000.00, ...}, ...]
///
/// Step 2: apply_changeset(changes=[{id:"txn-abc", category:"Veterinary"}])
///   → applies the recategorization
/// ```
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InspectTransactionsParams {
    /// Filter to transactions whose category name contains this substring
    /// (case-insensitive). Example: "Pets" matches "Pets", "Pet Supplies".
    pub category: Option<String>,
    /// Filter to transactions whose merchant name contains this substring
    /// (case-insensitive). Example: "Petco" matches "Petco Store #42".
    pub merchant: Option<String>,
    /// Start of the date range (ISO-8601, e.g. "2026-04-01").
    /// Defaults to the first day of the current month when omitted.
    pub start_date: Option<String>,
    /// End of the date range (ISO-8601, e.g. "2026-04-30").
    /// Defaults to the last day of the current month when omitted.
    pub end_date: Option<String>,
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
        description = "Drill into transactions for a date range, optionally narrowed by \
        category and/or merchant. Returns every matching transaction including its \
        **id** (required by apply_changeset to re-categorize), plus a compound summary \
        with total count, net amount, and an inflow-vs-outflow split so refunds are \
        visible alongside charges.\n\n\
        Two-step re-categorization workflow:\n\
        1. Call inspect_transactions(category=\"Pets\") to see all Pets transactions \
           with their ids and amounts.\n\
        2. Call apply_changeset(changes=[{\"id\": \"<id>\", \"category\": \"Veterinary\"}]) \
           to correct any mis-categorized entry.\n\n\
        This tool is read-only: it never modifies data. Use it to diagnose anomalies \
        (e.g. a $12k Pets spike or a $3.3k Medical charge) or to surface ids for \
        already-categorized transactions that need correction via apply_changeset."
    )]
    async fn inspect_transactions(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(params): Parameters<InspectTransactionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_inspection(&client, params).await {
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
        description = "List all accounts grouped into retirement-planning buckets: \
        tax_advantaged (401k, Roth IRA, HSA), taxable_brokerage, cash (depository), \
        other_assets (vehicles), and liabilities (credit cards, loans). \
        Each account shows its balance, Monarch type/subtype, hidden flag, and whether \
        its subtype was recognized. Includes a net-worth rollup for cross-checking \
        against financial_overview."
    )]
    async fn account_inventory(
        &self,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
        let mut client = MonarchClient::new(base);
        client.resolve_token_from_env_or_disk();

        let payload = match fetch_and_compute_inventory(&client).await {
            Ok(inventory) => serde_json::to_value(&inventory)
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

/// Convert a civil (year, month, day) triple to days since the Unix epoch.
///
/// Uses the Howard Hinnant days_from_civil algorithm. No input validation —
/// callers must ensure month is 1..=12 and day is valid for the month.
/// `parse_iso_date_to_epoch_day` validates before calling this; `today_epoch_day`
/// obtains a validated date from `chrono::Local`.
fn civil_to_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    // Howard Hinnant civil_from_days inverse: days_from_civil
    // https://howardhinnant.github.io/date_algorithms.html
    let y = if month <= 2 { year - 1 } else { year };
    let m = month as u32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an ISO `YYYY-MM-DD` date string to days since the Unix epoch.
///
/// Returns `None` for non-three-part or non-numeric inputs so callers can
/// fall back gracefully instead of silently using a wrong date.
fn parse_iso_date_to_epoch_day(s: &str) -> Option<i64> {
    let mut parts = s.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    // Reject if a fourth part exists (extra dashes)
    // splitn(3,'-') stops at 3 parts so no extra check needed.

    // Validate ranges before arithmetic to prevent overflow and silent normalization
    // Year must be in 1..=9999 to bound the Hinnant formula and avoid overflow
    if !(1..=9999).contains(&year) {
        return None;
    }
    // Month must be 1..=12
    if !(1..=12).contains(&month) {
        return None;
    }
    // Day must be 1..=days_in_month(year, month as u32)
    let max_day = days_in_month(year, month as u32) as i64;
    if day < 1 || day > max_day {
        return None;
    }

    Some(civil_to_epoch_day(year, month, day))
}

/// Days since the Unix epoch for "today" in the host's local timezone.
///
/// Honors the `MONARCH_NOW` test override (an ISO `YYYY-MM-DD` date) so
/// datetime-dependent behavior is deterministic and hermetic in tests.
/// Production leaves it unset and uses `chrono::Local` so the advisor
/// matches what the user sees running it locally (ADR 0006).
fn today_epoch_day() -> i64 {
    if let Ok(now_override) = std::env::var("MONARCH_NOW") {
        if let Some(day) = parse_iso_date_to_epoch_day(&now_override) {
            return day;
        }
        // Malformed override — fall through to the local clock, never a wrong fixed day
    }
    use chrono::{Datelike, Local};
    let today = Local::now().date_naive();
    civil_to_epoch_day(
        today.year() as i64,
        today.month() as i64,
        today.day() as i64,
    )
}

/// Pure computation of the current-month range for a given epoch day.
///
/// Extracted from `current_month_range` so it can be tested without touching
/// the system clock. Callers pass `today_epoch_day()`.
fn current_month_range_for_day(day: i64) -> (String, String) {
    let (year, month, _) = epoch_days_to_ymd(day);
    let start = format!("{year:04}-{month:02}-01");
    let last_day = days_in_month(year, month);
    let end = format!("{year:04}-{month:02}-{last_day:02}");
    (start, end)
}

/// Pure computation of the prior-month range for a given epoch day.
///
/// Extracted from `prior_month_range` so it can be tested without touching
/// the system clock. Callers pass `today_epoch_day()`.
fn prior_month_range_for_day(day: i64) -> (String, String) {
    let (mut year, mut month, _) = epoch_days_to_ymd(day);
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

/// Returns (start, end) for the current calendar month as ISO-8601 strings.
fn current_month_range() -> (String, String) {
    current_month_range_for_day(today_epoch_day())
}

/// Returns (start, end) for the prior calendar month.
fn prior_month_range() -> (String, String) {
    prior_month_range_for_day(today_epoch_day())
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

/// Fetch all transactions for the current month without a row cap.
///
/// Both `financial_overview` and `spending_report` must consume the same slice
/// so their spending totals agree (issue #25).
///
/// The limit is capped at `i32::MAX` (2,147,483,647) rather than `u32::MAX`
/// (4,294,967,295) because Monarch's GraphQL schema uses a signed `Int` for the
/// limit argument. Passing `u32::MAX` overflows GraphQL's `Int32` range and
/// causes Monarch to return a server-side GraphQL error (issue #47).
/// `i32::MAX` rows is effectively unbounded for any real household.
async fn fetch_current_month_transactions(
    client: &MonarchClient,
    start: &str,
    end: &str,
) -> Result<Vec<crate::client::Transaction>, MonarchError> {
    client.get_transactions(start, end, i32::MAX as u32).await
}

async fn fetch_and_compute(
    client: &MonarchClient,
) -> Result<crate::financial_overview::OverviewResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let (pri_start, pri_end) = prior_month_range();

    // Run the three lightweight requests in parallel, then await the heavy
    // transaction fetch alone (issue #47).
    //
    // Primary fix: fetch_current_month_transactions now uses i32::MAX instead
    // of u32::MAX, which overflowed GraphQL's signed Int32 and caused Monarch
    // to reject the request. The sequential ordering here is belt-and-suspenders:
    // it keeps the large fetch from racing other requests on the same connection.
    let (accounts, cashflow, history) = tokio::try_join!(
        client.get_accounts(),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
        client.get_net_worth_history(&pri_start, &pri_end),
    )?;

    let transactions = fetch_current_month_transactions(client, &cur_start, &cur_end).await?;

    Ok(compute_overview(
        &accounts,
        &cashflow,
        &transactions,
        &history,
    ))
}

async fn fetch_and_compute_spending(
    client: &MonarchClient,
) -> Result<crate::spending_report::SpendingReport, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let (pri_start, pri_end) = prior_month_range();

    // Run the lightweight budget and cashflow fetches in parallel, then await
    // the heavy transaction fetch alone (issue #47).
    //
    // Primary fix: fetch_current_month_transactions now uses i32::MAX instead
    // of u32::MAX (which overflowed GraphQL's signed Int32). The sequential
    // ordering here is belt-and-suspenders against any remaining server-side
    // contention from a large transaction payload.
    let (budgets, cashflow) = tokio::try_join!(
        client.get_budgets(&cur_start, &cur_end),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
    )?;

    let transactions = fetch_current_month_transactions(client, &cur_start, &cur_end).await?;

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
    let today_day = today_epoch_day();
    let (cur_start, cur_end) = current_month_range_for_day(today_day);
    let (pri_start, pri_end) = prior_month_range_for_day(today_day);
    let (today_year, today_month, _) = epoch_days_to_ymd(today_day);

    // Derive YYYY-MM labels for snapshot lookup
    let prior_month_label = &pri_start[..7]; // "YYYY-MM"
    let current_month_label = &cur_start[..7];

    // Fetch snapshots only when a debt-payoff goal is configured to keep the
    // no-debt-goal path's fetch cost unchanged.
    let snapshots = if goals.debt_payoff.is_some() {
        client.get_snapshots_by_account_type(&pri_start).await?
    } else {
        vec![]
    };

    let (accounts, cashflow) = tokio::try_join!(
        client.get_accounts(),
        client.get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end),
    )?;
    Ok(compute_progress(
        &goals,
        &accounts,
        &cashflow,
        &snapshots,
        prior_month_label,
        current_month_label,
        (today_year, today_month),
    ))
}

async fn fetch_and_compute_scan(
    client: &MonarchClient,
) -> Result<crate::recurring_scan::ScanResult, MonarchError> {
    let (cur_start, cur_end) = current_month_range();
    let items = client.get_recurring_for_scan(&cur_start, &cur_end).await?;
    Ok(compute_scan(&items))
}

async fn fetch_and_compute_inspection(
    client: &MonarchClient,
    params: InspectTransactionsParams,
) -> Result<crate::inspect_transactions::InspectionResult, MonarchError> {
    let (default_start, default_end) = current_month_range();

    // Coerce empty/whitespace-only strings to None uniformly across all params
    // so that `start_date=""` falls back to the month default just like
    // omitting it, and `category=""` / `merchant=""` impose no filter constraint.
    let start = blank_to_none(params.start_date).unwrap_or(default_start);
    let end = blank_to_none(params.end_date).unwrap_or(default_end);

    let transactions = client.get_transactions(&start, &end, 500).await?;

    let filter = InspectFilter {
        category: blank_to_none(params.category),
        merchant: blank_to_none(params.merchant),
    };

    Ok(compute_inspection(&transactions, &filter))
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

async fn fetch_and_compute_inventory(
    client: &MonarchClient,
) -> Result<crate::account_inventory::AccountInventory, MonarchError> {
    let accounts = client.get_accounts().await?;
    Ok(compute_account_inventory(&accounts))
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

/// Pure computation of the start of the month `n` months before the given epoch day.
///
/// Extracted from `months_ago_start` so it can be tested without touching the system
/// clock. `n=0` returns the first day of the month containing `day`.
fn months_ago_start_for_day(day: i64, n: u32) -> String {
    let (mut year, mut month, _) = epoch_days_to_ymd(day);
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

/// Compute the ISO date for the first day of the month that is `n` months before today.
///
/// Uses only integer arithmetic on the Unix epoch — no external date library needed.
fn months_ago_start(n: u32) -> String {
    months_ago_start_for_day(today_epoch_day(), n)
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
    let result = partition_changeset(&entries);

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
             spending_report, triage_uncategorized, inspect_transactions, \
             apply_changeset, progress_vs_goals, \
             cashflow_forecast, net_worth_trend, recurring_scan, account_inventory."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build an epoch day from a known date string for test inputs.
    fn day(date: &str) -> i64 {
        parse_iso_date_to_epoch_day(date).unwrap_or_else(|| panic!("bad test date: {date}"))
    }

    // --- civil_to_epoch_day ---

    #[test]
    fn civil_to_epoch_day_agrees_with_parse_for_unix_epoch() {
        // 1970-01-01 is day 0 — both paths must agree
        assert_eq!(civil_to_epoch_day(1970, 1, 1), 0);
        assert_eq!(
            civil_to_epoch_day(1970, 1, 1),
            parse_iso_date_to_epoch_day("1970-01-01").unwrap()
        );
    }

    #[test]
    fn civil_to_epoch_day_agrees_with_parse_for_mid_month() {
        let via_parse = parse_iso_date_to_epoch_day("2026-05-15").unwrap();
        assert_eq!(civil_to_epoch_day(2026, 5, 15), via_parse);
    }

    #[test]
    fn civil_to_epoch_day_agrees_with_parse_for_leap_day() {
        let via_parse = parse_iso_date_to_epoch_day("2024-02-29").unwrap();
        assert_eq!(civil_to_epoch_day(2024, 2, 29), via_parse);
    }

    #[test]
    fn civil_to_epoch_day_agrees_with_parse_for_year_end() {
        let via_parse = parse_iso_date_to_epoch_day("2025-12-31").unwrap();
        assert_eq!(civil_to_epoch_day(2025, 12, 31), via_parse);
    }

    #[test]
    fn civil_to_epoch_day_round_trips_through_epoch_days_to_ymd() {
        // Verify the round-trip identity: epoch_days_to_ymd(civil_to_epoch_day(y,m,d)) == (y,m,d)
        // Uses Local::now() — not hermetic on the date value, but hermetic on the identity.
        use chrono::{Datelike, Local};
        let today = Local::now().date_naive();
        let d = civil_to_epoch_day(
            today.year() as i64,
            today.month() as i64,
            today.day() as i64,
        );
        let (ry, rm, rd) = epoch_days_to_ymd(d);
        assert_eq!(
            (ry, rm, rd),
            (today.year() as i64, today.month(), today.day())
        );
    }

    // --- local timezone regression (issue #36) ---

    #[test]
    fn local_tz_wins_over_utc_at_month_boundary() {
        // 2026-06-01 01:00 UTC == 2026-05-31 18:00 in UTC-07:00.
        // A UTC-based today_epoch_day would compute June; local must compute May.
        use chrono::{Datelike, FixedOffset, TimeZone, Utc};

        let instant = Utc.with_ymd_and_hms(2026, 6, 1, 1, 0, 0).unwrap();
        let local = instant
            .with_timezone(&FixedOffset::west_opt(7 * 3600).unwrap())
            .date_naive();
        assert_eq!(
            (local.year(), local.month(), local.day()),
            (2026, 5, 31),
            "UTC-07 should see May 31, not June 1"
        );

        // Local path gives May range
        let local_day = civil_to_epoch_day(
            local.year() as i64,
            local.month() as i64,
            local.day() as i64,
        );
        assert_eq!(
            current_month_range_for_day(local_day),
            ("2026-05-01".to_string(), "2026-05-31".to_string()),
            "local date must produce May range"
        );

        // UTC path would wrongly give June range — confirming the old bug
        let utc = instant.date_naive();
        let utc_day = civil_to_epoch_day(utc.year() as i64, utc.month() as i64, utc.day() as i64);
        assert_eq!(
            current_month_range_for_day(utc_day),
            ("2026-06-01".to_string(), "2026-06-30".to_string()),
            "UTC date produces June range"
        );
    }

    #[test]
    fn ahead_of_utc_local_rolls_to_next_month() {
        // Symmetric mirror of the west case: an ahead-of-UTC user.
        // 2026-05-31 23:00 UTC == 2026-06-01 13:00 in UTC+14:00 (Line Islands).
        // A UTC-based today_epoch_day would compute May; local must compute June.
        use chrono::{Datelike, FixedOffset, TimeZone, Utc};

        let instant = Utc.with_ymd_and_hms(2026, 5, 31, 23, 0, 0).unwrap();
        let local = instant
            .with_timezone(&FixedOffset::east_opt(14 * 3600).unwrap())
            .date_naive();
        assert_eq!(
            (local.year(), local.month(), local.day()),
            (2026, 6, 1),
            "UTC+14 should see June 1, not May 31"
        );

        // Local path gives June range
        let local_day = civil_to_epoch_day(
            local.year() as i64,
            local.month() as i64,
            local.day() as i64,
        );
        assert_eq!(
            current_month_range_for_day(local_day),
            ("2026-06-01".to_string(), "2026-06-30".to_string()),
            "local date must produce June range"
        );

        // UTC path would wrongly give May range — confirming the symmetric bug
        let utc = instant.date_naive();
        let utc_day = civil_to_epoch_day(utc.year() as i64, utc.month() as i64, utc.day() as i64);
        assert_eq!(
            current_month_range_for_day(utc_day),
            ("2026-05-01".to_string(), "2026-05-31".to_string()),
            "UTC date produces May range"
        );
    }

    // --- parse_iso_date_to_epoch_day ---

    #[test]
    fn parse_iso_date_round_trips_known_epoch() {
        // 1970-01-01 is day 0
        assert_eq!(parse_iso_date_to_epoch_day("1970-01-01"), Some(0));
    }

    #[test]
    fn parse_iso_date_round_trips_2026_05_15() {
        // 2026-05-15: verify epoch_days_to_ymd(parse(...)) == (2026,5,15)
        let d = parse_iso_date_to_epoch_day("2026-05-15").unwrap();
        assert_eq!(epoch_days_to_ymd(d), (2026, 5, 15));
    }

    #[test]
    fn parse_iso_date_round_trips_2024_02_29_leap() {
        let d = parse_iso_date_to_epoch_day("2024-02-29").unwrap();
        assert_eq!(epoch_days_to_ymd(d), (2024, 2, 29));
    }

    #[test]
    fn parse_iso_date_returns_none_for_too_few_parts() {
        assert_eq!(parse_iso_date_to_epoch_day("2026-13"), None);
    }

    #[test]
    fn parse_iso_date_returns_none_for_garbage() {
        assert_eq!(parse_iso_date_to_epoch_day("garbage"), None);
    }

    #[test]
    fn parse_iso_date_rejects_out_of_range_month() {
        // Month > 12 should be rejected, not silently normalized
        assert_eq!(parse_iso_date_to_epoch_day("2026-13-01"), None);
        assert_eq!(parse_iso_date_to_epoch_day("2026-13-40"), None);
    }

    #[test]
    fn parse_iso_date_rejects_zero_month_and_day() {
        // Month 0, day 0 are invalid
        assert_eq!(parse_iso_date_to_epoch_day("2026-00-00"), None);
        assert_eq!(parse_iso_date_to_epoch_day("2026-00-15"), None);
        assert_eq!(parse_iso_date_to_epoch_day("2026-05-00"), None);
    }

    #[test]
    fn parse_iso_date_rejects_day_past_month_end() {
        // February 30 doesn't exist
        assert_eq!(parse_iso_date_to_epoch_day("2026-02-30"), None);
        // April 31 doesn't exist
        assert_eq!(parse_iso_date_to_epoch_day("2026-04-31"), None);
        // June 31 doesn't exist
        assert_eq!(parse_iso_date_to_epoch_day("2026-06-31"), None);
    }

    #[test]
    fn parse_iso_date_year_range_boundaries() {
        // Pin both edges of the accepted year range (1..=9999) so a mutation that
        // widens or shifts the bound is caught: 0 below the floor, 1 at the floor,
        // 9999 at the ceiling, 10000 just past it.
        assert_eq!(parse_iso_date_to_epoch_day("0000-06-15"), None);
        assert!(parse_iso_date_to_epoch_day("0001-06-15").is_some());
        assert!(parse_iso_date_to_epoch_day("9999-06-15").is_some());
        assert_eq!(parse_iso_date_to_epoch_day("10000-06-15"), None);
    }

    #[test]
    fn parse_iso_date_does_not_panic_on_huge_year() {
        // Must return None instead of panicking on overflow
        assert_eq!(
            parse_iso_date_to_epoch_day("9223372036854775807-06-15"),
            None
        );
        assert_eq!(parse_iso_date_to_epoch_day("999999-06-15"), None);
    }

    #[test]
    fn parse_iso_date_accepts_valid_dates() {
        // Sanity check that we still accept legitimate dates
        assert!(parse_iso_date_to_epoch_day("2026-04-30").is_some());
        assert!(parse_iso_date_to_epoch_day("2024-02-29").is_some());
        assert!(parse_iso_date_to_epoch_day("2026-05-31").is_some());
    }

    // --- current_month_range_for_day ---

    #[test]
    fn current_month_range_last_day_of_may_2026() {
        // The exact bug in issue #34: on 2026-05-31 the range must still be May.
        let (start, end) = current_month_range_for_day(day("2026-05-31"));
        assert_eq!(start, "2026-05-01");
        assert_eq!(end, "2026-05-31");
    }

    #[test]
    fn current_month_range_for_mid_month() {
        let (start, end) = current_month_range_for_day(day("2026-05-15"));
        assert_eq!(start, "2026-05-01");
        assert_eq!(end, "2026-05-31");
    }

    #[test]
    fn current_month_range_leap_february() {
        let (start, end) = current_month_range_for_day(day("2024-02-10"));
        assert_eq!(start, "2024-02-01");
        assert_eq!(end, "2024-02-29");
    }

    #[test]
    fn current_month_range_non_leap_february() {
        let (start, end) = current_month_range_for_day(day("2026-02-10"));
        assert_eq!(start, "2026-02-01");
        assert_eq!(end, "2026-02-28");
    }

    // --- prior_month_range_for_day ---

    #[test]
    fn prior_month_range_last_day_of_may_2026() {
        // The exact bug: on 2026-05-31 the prior range must be April, not March.
        let (start, end) = prior_month_range_for_day(day("2026-05-31"));
        assert_eq!(start, "2026-04-01");
        assert_eq!(end, "2026-04-30");
    }

    #[test]
    fn prior_month_range_crosses_year_boundary() {
        let (start, end) = prior_month_range_for_day(day("2026-01-15"));
        assert_eq!(start, "2025-12-01");
        assert_eq!(end, "2025-12-31");
    }

    #[test]
    fn prior_month_range_march_2024_gives_leap_february() {
        let (start, end) = prior_month_range_for_day(day("2024-03-10"));
        assert_eq!(start, "2024-02-01");
        assert_eq!(end, "2024-02-29");
    }

    // --- months_ago_start_for_day ---

    #[test]
    fn months_ago_start_n0_returns_current_month_start() {
        assert_eq!(months_ago_start_for_day(day("2026-05-15"), 0), "2026-05-01");
    }

    #[test]
    fn months_ago_start_n12_crosses_year() {
        assert_eq!(
            months_ago_start_for_day(day("2026-05-15"), 12),
            "2025-05-01"
        );
    }

    #[test]
    fn months_ago_start_n6_from_february_crosses_year() {
        // 2026-02-15 minus 6 months = 2025-08-01
        assert_eq!(months_ago_start_for_day(day("2026-02-15"), 6), "2025-08-01");
    }

    // --- Large (live) regression tests for issue #47 ---
    //
    // These tests live here (inside the crate) so they can call the private
    // `fetch_and_compute` and `fetch_and_compute_spending` functions directly.
    // An external integration test in `tests/` cannot reach private helpers, so
    // it would be forced to re-implement the fetch pattern inline — a copy that
    // could not detect a revert of the `i32::MAX` fix (ADR 0008, testing-seam
    // decision: prefer in-crate `#[cfg(test)]` over exposing orchestration as
    // `pub(crate)` or `pub`).
    //
    // Gated by `MONARCH_LIVE=1` so `cargo test` and CI remain hermetic.
    // Run with:
    //   MONARCH_LIVE=1 cargo test --lib -- tools::tests::financial_overview_concurrent_burst
    //   MONARCH_LIVE=1 cargo test --lib -- tools::tests::spending_report_concurrent_burst

    /// Verify that the financial_overview fetch path succeeds end-to-end against
    /// the real Monarch API (issue #47).
    ///
    /// Calls the production `fetch_and_compute` function directly so that a revert
    /// of the `i32::MAX` limit back to `u32::MAX` inside
    /// `fetch_current_month_transactions` causes this test to fail (the GraphQL
    /// server rejects the out-of-range value with a server-side error).
    ///
    /// RED evidence (before fix): `u32::MAX` produced:
    ///   GraphQL "Something went wrong while processing: None on request_id: None."
    /// GREEN: `i32::MAX as u32` passes, and this test verifies that invariant holds.
    #[tokio::test]
    async fn financial_overview_concurrent_burst_exercises_production_fetch_path() {
        if std::env::var("MONARCH_LIVE")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
            let mut client = crate::client::MonarchClient::new(base);
            client.resolve_token_from_env_or_disk();

            let overview = fetch_and_compute(&client).await.expect(
                "fetch_and_compute must succeed with i32::MAX limit — \
                         if this fails, the i32::MAX fix may have been reverted to u32::MAX",
            );

            // Net-worth fields must be finite — a NaN/Inf would mean the signed-
            // balance sum saw garbage rather than real account data.
            assert!(
                overview.net_worth.is_finite() && overview.net_worth_change.is_finite(),
                "net_worth and net_worth_change must be finite, got {} / {}",
                overview.net_worth,
                overview.net_worth_change,
            );
            // The cashflow block is computed from the transaction slice the
            // i32::MAX fetch returns. Assert the identity net == income − spending
            // holds end-to-end: a partial/garbled transaction response (the failure
            // mode the fix prevents) would break this equality — a non-vacuous check.
            let cf = &overview.cashflow;
            assert!(
                (cf.net - (cf.income - cf.spending)).abs() < 1e-6,
                "cashflow identity net == income − spending must hold: net={}, income={}, spending={}",
                cf.net,
                cf.income,
                cf.spending,
            );
            eprintln!(
                "net_worth: {:.2}, income: {:.2}, spending: {:.2}, net: {:.2}",
                overview.net_worth, cf.income, cf.spending, cf.net,
            );
        } else {
            eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        }
    }

    /// Verify that the spending_report fetch path succeeds end-to-end against
    /// the real Monarch API (issue #47).
    ///
    /// Calls the production `fetch_and_compute_spending` function directly so that
    /// a revert of the `i32::MAX` limit back to `u32::MAX` inside
    /// `fetch_current_month_transactions` causes this test to fail.
    #[tokio::test]
    async fn spending_report_concurrent_burst_exercises_production_fetch_path() {
        if std::env::var("MONARCH_LIVE")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
            let mut client = crate::client::MonarchClient::new(base);
            client.resolve_token_from_env_or_disk();

            let report = fetch_and_compute_spending(&client).await.expect(
                "fetch_and_compute_spending must succeed with i32::MAX limit — \
                         if this fails, the i32::MAX fix may have been reverted to u32::MAX",
            );

            // total_spent and the per-category report magnitudes are both derived
            // from the same fetched transaction slice (i32::MAX fetch) and apply the
            // same income/transfer exclusions, so they MUST sum equal. This is a
            // real internal-consistency invariant — unlike `>= 0.0`, which is a
            // tautology since total_spent is documented as a positive magnitude.
            let category_sum: f64 = report.by_category.values().map(|c| c.spent).sum();
            assert!(
                (report.total_spent - category_sum).abs() < 1e-6,
                "total_spent must equal the sum of per-category magnitudes: \
                 total_spent={}, category_sum={}",
                report.total_spent,
                category_sum,
            );
            eprintln!(
                "total_spent: {:.2}, category_sum: {:.2}",
                report.total_spent, category_sum,
            );
        } else {
            eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        }
    }
}
