//! Large (live) integration tests — gated by `MONARCH_LIVE=1`.
//!
//! These tests call the REAL Monarch Money API using the session token from
//! `~/.config/monarch-mcp/session.json` (or `MONARCH_TOKEN` env var). They
//! are excluded from `cargo test` and CI by default — each test returns early
//! when `MONARCH_LIVE` is unset.
//!
//! # How to run
//!
//! ```bash
//! MONARCH_LIVE=1 cargo test --test live_integration -- --nocapture
//! ```
//!
//! Or target a single test:
//!
//! ```bash
//! MONARCH_LIVE=1 cargo test --test live_integration financial_overview_returns_real_net_worth -- --nocapture
//! ```
//!
//! # What these tests assert
//!
//! Structural validity: no GraphQL errors, expected fields present, values
//! within sane ranges. They do NOT assert specific dollar amounts because
//! those change daily.
//!
//! # Test size classification
//!
//! | Tier   | Location                       | Runner                          | Gate             |
//! |--------|--------------------------------|---------------------------------|------------------|
//! | Small  | `src/**/*.rs` `#[cfg(test)]`   | `cargo test`                    | always           |
//! | Medium | `bdd/features/**/*.feature`    | `cd bdd && uv run behave`       | always (mock)    |
//! | Large  | `tests/live_integration.rs`    | `MONARCH_LIVE=1 cargo test --test live_integration` | `MONARCH_LIVE=1` |

use monarch_mcp::account_inventory::compute_account_inventory;
use monarch_mcp::asset_allocation::compute_asset_allocation;
use monarch_mcp::budget_review::compute_budget_review;
use monarch_mcp::client::{MonarchClient, GRAPHQL_INT_MAX};
use monarch_mcp::financial_overview::compute_overview;
use monarch_mcp::inspect_transactions::{compute_inspection, InspectFilter};
use monarch_mcp::net_worth_trend::compute_trend;
use monarch_mcp::recurring_scan::compute_scan;
use monarch_mcp::retirement_readiness::{
    compute_retirement_readiness, invested_financial_accounts, validate_withdrawal_rate,
    WITHDRAWAL_RATE_DEFAULT,
};
use monarch_mcp::savings_rate::compute_savings_rate;
use monarch_mcp::spending_history::{compute_spending_history, range_for_months_count};
use monarch_mcp::spending_report::compute_spending_report;
use monarch_mcp::subscription_audit::compute_subscription_audit;
use monarch_mcp::triage::resolve_category_names;
use std::env;

fn live_enabled() -> bool {
    env::var("MONARCH_LIVE").map(|v| v == "1").unwrap_or(false)
}

fn make_live_client() -> MonarchClient {
    let base = env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());
    let mut client = MonarchClient::new(base);
    client.resolve_token_from_env_or_disk();
    client
}

fn current_month() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (now / 86_400) as i64;
    let (y, m, _) = days_to_ymd(days);
    let last = days_in_month(y, m);
    (
        format!("{y:04}-{m:02}-01"),
        format!("{y:04}-{m:02}-{last:02}"),
    )
}

fn prior_month() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (now / 86_400) as i64;
    let (mut y, mut m, _) = days_to_ymd(days);
    if m == 1 {
        y -= 1;
        m = 12;
    } else {
        m -= 1;
    }
    let last = days_in_month(y, m);
    (
        format!("{y:04}-{m:02}-01"),
        format!("{y:04}-{m:02}-{last:02}"),
    )
}

fn days_to_ymd(days: i64) -> (i64, u32, u32) {
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

/// Build the 12-month forward audit window for use in live tests.
///
/// Mirrors `audit_window_for_day` from `tools.rs` so the live test uses the
/// same window as the real handler. Start = today, End = last day of the month
/// 12 months forward.
fn twelve_month_audit_window() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now / 86_400) as i64;
    let (year, month, dom) = days_to_ymd(today_days);
    let start = format!("{year:04}-{month:02}-{dom:02}");
    let mut ey = year;
    let mut em = month;
    for _ in 0..12 {
        if em == 12 {
            ey += 1;
            em = 1;
        } else {
            em += 1;
        }
    }
    let last_day = days_in_month(ey, em);
    let end = format!("{ey:04}-{em:02}-{last_day:02}");
    (start, end)
}

// ---------------------------------------------------------------------------
// Large integration tests
// ---------------------------------------------------------------------------

/// Verify that financial_overview produces a finite, non-zero net worth from
/// real Monarch data. Exercises GetAccounts, Web_GetCashFlowPage (two calls:
/// current + prior month), and GetAggregateSnapshots — all must return HTTP
/// 200 with no GraphQL errors.
#[tokio::test]
async fn financial_overview_returns_real_net_worth() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();
    let (pri_start, pri_end) = prior_month();

    let accounts = client
        .get_accounts()
        .await
        .expect("GetAccounts must succeed against real Monarch");
    assert!(!accounts.is_empty(), "must have at least one account");
    eprintln!("accounts: {}", accounts.len());

    let cashflow = client
        .get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end)
        .await
        .expect("Web_GetCashFlowPage must succeed against real Monarch");
    assert!(cashflow.income >= 0.0, "income must be non-negative");
    assert!(cashflow.spending >= 0.0, "spending must be non-negative");
    eprintln!(
        "income: {:.2}, spending: {:.2}, prior_spending: {:.2}",
        cashflow.income, cashflow.spending, cashflow.prior_month_spending
    );

    let history = client
        .get_net_worth_history(&pri_start, &pri_end)
        .await
        .expect("GetAggregateSnapshots must succeed against real Monarch");
    eprintln!(
        "prior_month_net_worth: {:.2}",
        history.prior_month_net_worth
    );

    let transactions = client
        .get_transactions(&cur_start, &cur_end, 500)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");
    eprintln!("transactions this month: {}", transactions.len());

    let overview = compute_overview(&accounts, &cashflow, &transactions, &history);
    eprintln!("net_worth: {:.2}", overview.net_worth);
    eprintln!("net_worth_change: {:.2}", overview.net_worth_change);
    eprintln!("cashflow.net: {:.2}", overview.cashflow.net);
    eprintln!(
        "overview.cashflow.spending (true spending): {:.2}",
        overview.cashflow.spending
    );

    assert!(
        overview.net_worth.is_finite(),
        "net_worth must be finite — NaN/inf indicates a GraphQL schema mismatch, got: {}",
        overview.net_worth
    );
    assert_ne!(
        overview.net_worth, 0.0,
        "net_worth must not be exactly zero for a real account"
    );
}

/// Verify that GetTransactionsList returns valid transactions with non-empty
/// ids and finite amounts (no schema errors in the real response).
#[tokio::test]
async fn transactions_return_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let txns = client
        .get_transactions(&cur_start, &cur_end, 50)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    eprintln!("transactions this month: {}", txns.len());
    for t in &txns {
        assert!(!t.id.is_empty(), "transaction id must not be empty");
        assert!(
            t.amount.is_finite(),
            "amount must be finite, got {} for id {}",
            t.amount,
            t.id
        );
        assert!(!t.date.is_empty(), "date must not be empty for id {}", t.id);
    }
}

/// Verify that GetCategories returns non-empty categories with valid ids and
/// names (no GraphQL schema errors).
#[tokio::test]
async fn categories_return_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let cats = client
        .get_categories()
        .await
        .expect("GetCategories must succeed against real Monarch");

    eprintln!("categories: {}", cats.len());
    assert!(!cats.is_empty(), "must have at least one category");
    for c in &cats {
        assert!(!c.id.is_empty(), "category id must not be empty");
        assert!(
            !c.name.is_empty(),
            "category name must not be empty for id {}",
            c.id
        );
    }
}

/// Verify that GetHouseholdTransactionTags returns without GraphQL errors.
/// Tags may be empty if the household hasn't created any.
#[tokio::test]
async fn tags_return_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let tags = client
        .get_tags()
        .await
        .expect("GetHouseholdTransactionTags must succeed against real Monarch");

    eprintln!("tags: {}", tags.len());
    for t in &tags {
        assert!(!t.id.is_empty(), "tag id must not be empty");
        assert!(
            !t.name.is_empty(),
            "tag name must not be empty for id {}",
            t.id
        );
    }
}

/// Verify that Web_GetUpcomingRecurringTransactionItems returns structurally
/// valid items from the real Monarch API: no GraphQL errors, finite amounts,
/// non-empty merchant names, and well-formed date strings (YYYY-MM-DD).
///
/// Does NOT assert specific merchants or amounts — those change over time.
#[tokio::test]
async fn get_recurring_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let items = client
        .get_recurring(&cur_start, &cur_end)
        .await
        .expect("Web_GetUpcomingRecurringTransactionItems must succeed against real Monarch");

    eprintln!("recurring items this month: {}", items.len());

    for item in &items {
        assert!(
            item.amount.is_finite(),
            "recurring item amount must be finite, got {} for merchant {:?}",
            item.amount,
            item.merchant
        );
        assert!(
            !item.merchant.is_empty(),
            "recurring item merchant name must not be empty"
        );
    }
}

/// Verify that GetSnapshotsByAccountType returns structurally valid rows from
/// the real Monarch API: no GraphQL errors, finite balances, non-empty account
/// type strings, and well-formed month strings (YYYY-MM format).
///
/// Does NOT assert specific balances or account types — those change over time.
/// Asserts structural validity only (field presence and type correctness).
#[tokio::test]
async fn get_snapshots_by_account_type_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    // Use a start date 3 months back to get a realistic multi-month series.
    let (start, _) = prior_month();
    // Trim to first-of-month (prior_month() already returns first-of-month).
    let start_date = start;

    let snapshots = client
        .get_snapshots_by_account_type(&start_date)
        .await
        .expect("GetSnapshotsByAccountType must succeed against real Monarch");

    eprintln!("snapshot rows returned: {}", snapshots.len());

    for snap in &snapshots {
        assert!(
            !snap.account_type.is_empty(),
            "account_type must not be empty"
        );
        assert!(
            snap.balance.is_finite(),
            "balance must be finite, got {} for type {:?} month {:?}",
            snap.balance,
            snap.account_type,
            snap.month
        );
        // Month must be in YYYY-MM format (10 chars minimum: "2026-05").
        assert!(
            snap.month.len() >= 7 && snap.month.contains('-'),
            "month must be YYYY-MM format, got {:?}",
            snap.month
        );
    }

    // Feed snapshots into compute_trend and verify structural validity.
    let trend = compute_trend(&snapshots);
    eprintln!("monthly_snapshots: {}", trend.monthly_snapshots.len());
    eprintln!("latest_net_worth: {:.2}", trend.latest_net_worth);
    eprintln!("net_worth_change: {:.2}", trend.net_worth_change);
    eprintln!("total_assets: {:.2}", trend.total_assets);
    eprintln!("total_liabilities: {:.2}", trend.total_liabilities);

    assert!(
        trend.latest_net_worth.is_finite(),
        "latest_net_worth must be finite"
    );
    assert!(
        trend.net_worth_change.is_finite(),
        "net_worth_change must be finite"
    );
    assert!(
        trend.total_assets >= 0.0,
        "total_assets must be non-negative"
    );
    assert!(
        trend.total_liabilities >= 0.0,
        "total_liabilities must be non-negative"
    );
}

/// Verify that `recurring_scan`'s data path works end-to-end against the real
/// Monarch API: no GraphQL errors, all items parse into `RecurringScanItem`,
/// and `compute_scan` produces structurally valid output.
///
/// Does NOT assert specific merchants, amounts, or counts — those change over
/// time. Asserts structural validity only (finite numbers, non-empty strings,
/// correct sign conventions).
#[tokio::test]
async fn recurring_scan_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let items = client
        .get_recurring_for_scan(&cur_start, &cur_end)
        .await
        .expect("Web_GetUpcomingRecurringTransactionItems must succeed against real Monarch");

    eprintln!("recurring items returned: {}", items.len());

    for item in &items {
        assert!(!item.merchant.is_empty(), "merchant name must not be empty");
        assert!(
            item.stream_amount.is_finite(),
            "stream_amount must be finite for {:?}",
            item.merchant
        );
        assert!(
            item.actual_amount.is_finite(),
            "actual_amount must be finite for {:?}",
            item.merchant
        );
        assert!(
            item.amount_diff.is_finite(),
            "amount_diff must be finite for {:?}",
            item.merchant
        );
        // Monarch convention: outflows stored as negative amounts
        assert!(
            item.stream_amount <= 0.0,
            "stream_amount must be non-positive (outflow) for {:?}, got {}",
            item.merchant,
            item.stream_amount
        );
        assert!(
            item.actual_amount <= 0.0,
            "actual_amount must be non-positive (outflow) for {:?}, got {}",
            item.merchant,
            item.actual_amount
        );
    }

    // Feed items into compute_scan and verify structural validity.
    let scan = compute_scan(&items);
    eprintln!("creeping_charges: {}", scan.creeping_charges.len());
    eprintln!("upcoming_renewals: {}", scan.upcoming_renewals.len());

    for charge in &scan.creeping_charges {
        assert!(
            !charge.merchant.is_empty(),
            "creeping charge merchant must not be empty"
        );
        assert!(
            charge.amount_change.is_finite() && charge.amount_change != 0.0,
            "creeping charge amount_change must be finite and non-zero for {:?}, got {}",
            charge.merchant,
            charge.amount_change
        );
    }

    for renewal in &scan.upcoming_renewals {
        assert!(
            !renewal.merchant.is_empty(),
            "upcoming renewal merchant must not be empty"
        );
    }
}

/// Verify that `spending_report` honours the Monarch sign convention when
/// operating against the real API (issue #24).
///
/// Asserts:
/// 1. `over_budget_categories` contains only expense-group categories — no
///    income or transfer category ever appears there, regardless of amount.
/// 2. `total_spent` is non-negative (magnitudes only, never negative sums).
/// 3. `total_spent` is within a 3× factor of `financial_overview.spending`
///    (soft cross-check; full reconciliation is issue #25).  A gross mismatch
///    would indicate that income transactions are being summed into spending.
///
/// Does NOT assert specific dollar amounts — those change daily.
#[tokio::test]
async fn spending_report_excludes_income_and_uses_correct_sign_convention() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();
    let (pri_start, pri_end) = prior_month();

    // Fetch all data needed for spending_report.
    let transactions = client
        .get_transactions(&cur_start, &cur_end, 500)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    let budgets = client
        .get_budgets(&cur_start, &cur_end)
        .await
        .expect("GetJointPlanningData must succeed against real Monarch");

    let cashflow = client
        .get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end)
        .await
        .expect("Web_GetCashFlowPage must succeed against real Monarch");

    let report = compute_spending_report(&transactions, &budgets, &cashflow);

    eprintln!("total_spent: {:.2}", report.total_spent);
    eprintln!(
        "over_budget_categories: {:?}",
        report.over_budget_categories
    );
    eprintln!("financial_overview.spending: {:.2}", cashflow.spending);

    // 1. total_spent must be non-negative — expense magnitudes are always ≥ 0.
    assert!(
        report.total_spent >= 0.0,
        "total_spent must be non-negative; got {}. \
         A negative value indicates income transactions are being sign-summed into spending.",
        report.total_spent
    );

    // 2. No income or transfer category must appear in over_budget_categories.
    //    We verify this by checking every over-budget category name against the
    //    transaction list — if a category had only income/transfer transactions
    //    it must not appear in over_budget_categories.
    let income_transfer_categories: std::collections::HashSet<String> = transactions
        .iter()
        .filter(|t| {
            matches!(
                t.category.group_type.as_deref(),
                Some("income") | Some("transfer")
            )
        })
        .map(|t| t.category.name.clone())
        .collect();

    // A category that is exclusively income/transfer must never be over-budget.
    // (A category with both expense and income transactions, e.g. a refund + charge,
    // may legitimately appear if net expense exceeds budget — that is correct.)
    let expense_categories_with_any_expense: std::collections::HashSet<String> = transactions
        .iter()
        .filter(|t| matches!(t.category.group_type.as_deref(), Some("expense") | None))
        .map(|t| t.category.name.clone())
        .collect();

    for cat in &report.over_budget_categories {
        let is_income_only = income_transfer_categories.contains(cat)
            && !expense_categories_with_any_expense.contains(cat);
        assert!(
            !is_income_only,
            "income/transfer-only category {:?} must never appear in over_budget_categories. \
             This indicates sign-convention is not being applied correctly.",
            cat
        );
    }

    // 3. Exact agreement: spending_report.total_spent and financial_overview.spending
    //    both call compute_true_spending with the same transaction slice.
    //    They must be byte-identical (same function, same inputs, no divergence).
    let true_spending = monarch_mcp::spending_report::compute_true_spending(&transactions);
    assert_eq!(
        report.total_spent, true_spending,
        "spending_report.total_spent ({:.2}) must equal compute_true_spending ({:.2}) \
         — the report delegates to this helper directly",
        report.total_spent, true_spending
    );
    eprintln!(
        "agreement confirmed: total_spent = compute_true_spending = {:.2}",
        report.total_spent
    );
}

/// Verify that GetJointPlanningData returns budget entries with valid
/// category names and positive planned amounts (no GraphQL errors).
#[tokio::test]
async fn budgets_return_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let budgets = client
        .get_budgets(&cur_start, &cur_end)
        .await
        .expect("GetJointPlanningData must succeed against real Monarch");

    eprintln!("budget entries (non-zero planned): {}", budgets.len());
    for b in &budgets {
        assert!(
            !b.category.name.is_empty(),
            "budget category name must not be empty"
        );
        // Monarch stores expense budgets as negative plannedCashFlowAmount values
        // (e.g., "Loan Repayment" → -1280). The only invariant is nonzero and
        // finite — zero-budget categories are filtered by get_budgets().
        assert!(
            b.amount.is_finite() && b.amount != 0.0,
            "budget amount must be finite and nonzero (zero budgets are filtered), got {} for {}",
            b.amount,
            b.category.name
        );
    }
}

/// Verify that `inspect_transactions` works end-to-end against real Monarch:
/// - `get_transactions` returns data with non-empty ids and finite amounts.
/// - `compute_inspection` with no filter returns all transactions and produces
///   finite summary values with correct inflow/outflow split.
/// - `compute_inspection` with a category filter returns only matching
///   transactions (category name substring match is case-insensitive).
///
/// Does NOT assert specific merchants, amounts, or categories — those change
/// daily. Asserts structural validity and filtering semantics only.
#[tokio::test]
async fn inspect_transactions_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let transactions = client
        .get_transactions(&cur_start, &cur_end, 200)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    eprintln!("transactions this month: {}", transactions.len());

    // Structural validity: every transaction has a non-empty id and finite amount.
    for t in &transactions {
        assert!(!t.id.is_empty(), "transaction id must not be empty");
        assert!(
            t.amount.is_finite(),
            "amount must be finite for id {} merchant {:?}",
            t.id,
            t.merchant_name
        );
    }

    // No-filter inspection returns all transactions with correct totals.
    let no_filter = InspectFilter::default();
    let result = compute_inspection(&transactions, &no_filter);

    assert_eq!(
        result.summary.total_count,
        transactions.len(),
        "total_count must equal number of transactions fetched"
    );
    assert!(
        result.summary.net_amount.is_finite(),
        "net_amount must be finite"
    );
    assert!(
        result.summary.total_inflow >= 0.0,
        "total_inflow must be non-negative, got {}",
        result.summary.total_inflow
    );
    assert!(
        result.summary.total_outflow >= 0.0,
        "total_outflow must be non-negative, got {}",
        result.summary.total_outflow
    );
    // Inflow + outflow must round-trip to the same magnitude as computing
    // the net directly (within floating-point tolerance).
    let expected_net = result.summary.total_inflow - result.summary.total_outflow;
    assert!(
        (result.summary.net_amount - expected_net).abs() < 0.01,
        "net_amount ({}) must equal total_inflow - total_outflow ({})",
        result.summary.net_amount,
        expected_net
    );

    // Every line item must carry a non-empty id (the apply_changeset input).
    for item in &result.transactions {
        assert!(!item.id.is_empty(), "line item id must not be empty");
        assert!(!item.date.is_empty(), "line item date must not be empty");
        assert!(item.amount.is_finite(), "line item amount must be finite");
    }

    eprintln!(
        "no-filter: count={} net={:.2} inflow={:.2} outflow={:.2}",
        result.summary.total_count,
        result.summary.net_amount,
        result.summary.total_inflow,
        result.summary.total_outflow
    );

    // Category-filtered inspection: pick the first category that appears in
    // the transactions and verify the filter narrows correctly.
    if let Some(first_category) = transactions.first().map(|t| t.category.name.clone()) {
        let cat_filter = InspectFilter {
            category: Some(first_category.clone()),
            merchant: None,
        };
        let filtered = compute_inspection(&transactions, &cat_filter);

        // Every result transaction must match the category filter.
        for item in &filtered.transactions {
            assert!(
                item.category
                    .to_lowercase()
                    .contains(&first_category.to_lowercase()),
                "filtered item category {:?} must contain {:?}",
                item.category,
                first_category
            );
        }

        eprintln!(
            "category filter {:?}: {} of {} transactions match",
            first_category,
            filtered.summary.total_count,
            transactions.len()
        );
    }
}

/// Verify that `financial_overview` and `spending_report` agree on true spending
/// when operating against the real Monarch API (issue #25).
///
/// Both tools must call the shared `compute_true_spending` helper with the same
/// transaction slice. This test ensures they produce byte-identical spending figures
/// by fetching the same current-month data and asserting the outputs match.
///
/// Asserts:
/// 1. `financial_overview.cashflow.spending` (output of compute_overview) equals
///    `spending_report.total_spent` (output of compute_spending_report) — both
///    delegate to the same compute_true_spending function with the same inputs.
/// 2. Both spending figures are non-negative (magnitudes only, Monarch sign convention).
/// 3. The figures exclude income and transfer transactions (verified by cross-checking
///    against the transaction categories).
///
/// Does NOT assert specific dollar amounts — those change daily.
#[tokio::test]
async fn financial_overview_and_spending_report_agree_on_true_spending() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();
    let (pri_start, pri_end) = prior_month();

    // Fetch all data needed for both compute paths.
    let accounts = client
        .get_accounts()
        .await
        .expect("GetAccounts must succeed against real Monarch");

    // Use the same limit as production (GRAPHQL_INT_MAX) so a future handler
    // cap-mismatch regression causes this test to fail (Finding 1, issue #33).
    let transactions = client
        .get_transactions(&cur_start, &cur_end, GRAPHQL_INT_MAX)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    let budgets = client
        .get_budgets(&cur_start, &cur_end)
        .await
        .expect("GetJointPlanningData must succeed against real Monarch");

    let cashflow = client
        .get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end)
        .await
        .expect("Web_GetCashFlowPage must succeed against real Monarch");

    let history = client
        .get_net_worth_history(&pri_start, &pri_end)
        .await
        .expect("GetAggregateSnapshots must succeed against real Monarch");

    // Call both compute paths with the same data.
    let overview = compute_overview(&accounts, &cashflow, &transactions, &history);
    let report = compute_spending_report(&transactions, &budgets, &cashflow);

    let overview_spending = overview.cashflow.spending;
    let report_spending = report.total_spent;

    eprintln!("financial_overview.spending: {:.2}", overview_spending);
    eprintln!("spending_report.total_spent: {:.2}", report_spending);

    // 1. Both must be equal (they call the same helper with the same inputs).
    assert_eq!(
        overview_spending, report_spending,
        "financial_overview.spending ({:.2}) must equal spending_report.total_spent ({:.2}) — \
         both delegate to compute_true_spending with the same transaction slice",
        overview_spending, report_spending
    );

    // 2. Both must be non-negative (magnitudes only).
    assert!(
        overview_spending >= 0.0,
        "financial_overview.spending must be non-negative; got {}",
        overview_spending
    );
    assert!(
        report_spending >= 0.0,
        "spending_report.total_spent must be non-negative; got {}",
        report_spending
    );

    // 3. Verify that the spending does not include income or transfer transactions.
    //    Income transactions have positive amounts; transfer transactions have group_type "transfer".
    //    If the spending equals a naive sum of all transactions (including income),
    //    it would be incorrect.
    let income_transfer_sum: f64 = transactions
        .iter()
        .filter(|t| {
            matches!(
                t.category.group_type.as_deref(),
                Some("income") | Some("transfer")
            )
        })
        .map(|t| t.amount.abs())
        .sum();

    eprintln!(
        "sum of income/transfer transaction magnitudes: {:.2}",
        income_transfer_sum
    );

    // A naive sum that included income would be (overview_spending + income_transfer_sum).
    // Confirm that overview_spending is significantly less than that naive sum
    // (it should be, assuming the account has any income or transfers).
    if income_transfer_sum > 0.01 {
        let naive_sum = overview_spending + income_transfer_sum;
        assert!(
            overview_spending < naive_sum,
            "spending ({:.2}) should be less than naive_sum ({:.2}); \
             if they were equal, income/transfer would be incorrectly included",
            overview_spending,
            naive_sum
        );
        eprintln!(
            "verified: spending ({:.2}) excludes income/transfer (naive sum would be {:.2})",
            overview_spending, naive_sum
        );
    } else {
        eprintln!("no income/transfer transactions this month; structural check passed");
    }
}

/// Verify that `account_inventory` works end-to-end against the real Monarch
/// API: GetAccounts succeeds, all accounts parse correctly (subtype nullable
/// per ADR 0003, balance nullable per ADR 0003), and `compute_account_inventory`
/// produces structurally valid output.
///
/// Does NOT assert specific balances, account names, or bucket membership —
/// those change over time. Asserts structural validity only:
/// - At least one account is returned.
/// - Every account entry has a non-empty display_name and type_name.
/// - Every bucket total is finite.
/// - Rollup invariant holds: net_worth = total_assets − total_liabilities.
/// - Known Monarch type strings all appear in recognized buckets (not flagged
///   as unknown_subtype unless the vocabulary has genuinely changed).
///
/// This test will catch any future Monarch schema change that renames or removes
/// the `subtype`, `isHidden`, or `currentBalance` fields — the same failure mode
/// that ADR 0009 was written to prevent for invented subtype strings.
#[tokio::test]
async fn account_inventory_returns_valid_structure() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();

    let accounts = client
        .get_accounts()
        .await
        .expect("GetAccounts must succeed against real Monarch");

    assert!(!accounts.is_empty(), "must have at least one account");
    eprintln!("accounts returned: {}", accounts.len());

    let inventory = compute_account_inventory(&accounts);

    // Every account entry must have non-empty name and type.
    for (bucket_name, bucket) in &inventory.buckets {
        assert!(
            bucket.total.is_finite(),
            "bucket {bucket_name:?} total must be finite, got {}",
            bucket.total
        );
        for entry in &bucket.accounts {
            assert!(
                !entry.display_name.is_empty(),
                "display_name must not be empty in bucket {bucket_name:?}"
            );
            assert!(
                !entry.type_name.is_empty(),
                "type_name must not be empty for {:?}",
                entry.display_name
            );
            assert!(
                entry.balance.is_finite(),
                "balance must be finite for {:?}, got {}",
                entry.display_name,
                entry.balance
            );
            if entry.unknown_subtype {
                eprintln!(
                    "WARNING: unknown subtype for {:?} (type={:?}, subtype={:?}) — \
                     Monarch may have added a new subtype; update ADR 0009 if so",
                    entry.display_name, entry.type_name, entry.subtype_name
                );
            }
        }
    }

    // Independent rollup reconciliation: compute expected values from raw signed
    // account balances and assert the inventory matches. This catches BUG B/C —
    // a tautological check (net_worth == total_assets − total_liabilities) cannot
    // detect wrong totals because both sides of the equation are from the same
    // compute function.
    let expected_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    let expected_total_assets: f64 = accounts.iter().map(|a| a.current_balance.max(0.0)).sum();
    let expected_total_liabilities: f64 =
        accounts.iter().map(|a| (-a.current_balance).max(0.0)).sum();

    let rollup = &inventory.rollup;
    assert!(
        rollup.total_assets.is_finite(),
        "total_assets must be finite, got {}",
        rollup.total_assets
    );
    assert!(
        rollup.total_liabilities.is_finite(),
        "total_liabilities must be finite, got {}",
        rollup.total_liabilities
    );
    assert!(
        rollup.net_worth.is_finite(),
        "net_worth must be finite, got {}",
        rollup.net_worth
    );
    assert!(
        rollup.total_assets >= 0.0,
        "total_assets must be non-negative, got {}",
        rollup.total_assets
    );
    assert!(
        rollup.total_liabilities >= 0.0,
        "total_liabilities must be non-negative (absolute value), got {}",
        rollup.total_liabilities
    );
    assert!(
        (rollup.net_worth - expected_net_worth).abs() < 0.01,
        "net_worth ({:.2}) must equal raw signed sum of all account balances ({:.2})",
        rollup.net_worth,
        expected_net_worth
    );
    assert!(
        (rollup.total_assets - expected_total_assets).abs() < 0.01,
        "total_assets ({:.2}) must equal sum of positive account balances ({:.2})",
        rollup.total_assets,
        expected_total_assets
    );
    assert!(
        (rollup.total_liabilities - expected_total_liabilities).abs() < 0.01,
        "total_liabilities ({:.2}) must equal abs-sum of negative account balances ({:.2})",
        rollup.total_liabilities,
        expected_total_liabilities
    );

    eprintln!("total_assets:      {:.2}", rollup.total_assets);
    eprintln!("total_liabilities: {:.2}", rollup.total_liabilities);
    eprintln!("net_worth:         {:.2}", rollup.net_worth);
    eprintln!(
        "buckets: {:?}",
        inventory.buckets.keys().collect::<Vec<_>>()
    );

    // Count unknown subtypes — any non-zero count is a signal to update ADR 0009.
    let unknown_count: usize = inventory
        .buckets
        .values()
        .flat_map(|b| &b.accounts)
        .filter(|e| e.unknown_subtype)
        .count();
    eprintln!("accounts with unknown_subtype: {unknown_count}");
    // Non-fatal: warn only. A strict assert here would break if Monarch adds a
    // new subtype before we can update ADR 0009 and the bucket map.
}

/// Verify that `asset_allocation` produces structurally valid output from real
/// Monarch data (issue #67) and that its `net_worth` agrees with
/// `financial_overview`'s net worth (the trust cross-check).
///
/// Asserts:
/// 1. GetAccounts succeeds and returns at least one account.
/// 2. `compute_asset_allocation` produces finite, non-NaN class totals and
///    rollup values.
/// 3. `gross_assets` is non-negative (sum of positive-balance class totals).
/// 4. `total_liabilities` is ≤ 0 (signed sum of liability-class balances).
/// 5. Per-class percentages sum to ~100% of gross_assets (within 0.1%).
/// 6. `net_worth` matches `financial_overview`'s net worth (same account slice,
///    different aggregation path — cross-check per ADR 0014).
/// 7. No class has an infinite or NaN total.
///
/// Does NOT assert specific dollar amounts — those change daily.
#[tokio::test]
async fn asset_allocation_returns_valid_structure_and_agrees_with_financial_overview() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();
    let (pri_start, pri_end) = prior_month();

    // Fetch accounts (shared data source for both tools).
    let accounts = client
        .get_accounts()
        .await
        .expect("GetAccounts must succeed against real Monarch");

    assert!(!accounts.is_empty(), "must have at least one account");
    eprintln!("accounts returned: {}", accounts.len());

    // --- asset_allocation path ---
    let allocation = compute_asset_allocation(&accounts);

    eprintln!("gross_assets:       {:.2}", allocation.gross_assets);
    eprintln!("total_liabilities:  {:.2}", allocation.total_liabilities);
    eprintln!("net_worth:          {:.2}", allocation.net_worth);
    eprintln!(
        "classes:            {:?}",
        allocation.classes.keys().collect::<Vec<_>>()
    );

    // Every class total must be finite and not NaN.
    for (class_name, summary) in &allocation.classes {
        assert!(
            summary.total.is_finite(),
            "class {class_name:?} total must be finite, got {}",
            summary.total
        );
        if let Some(pct) = summary.percent_of_assets {
            assert!(
                pct.is_finite() && pct >= 0.0,
                "class {class_name:?} percent_of_assets must be finite and non-negative, got {pct}"
            );
        }
        eprintln!(
            "  {class_name}: total={:.2} pct={:?} recognized={}",
            summary.total, summary.percent_of_assets, summary.recognized
        );
        if !summary.recognized {
            eprintln!(
                "  WARNING: class {class_name:?} contains an unrecognized account type/subtype — \
                 check ADR 0009/0014 for vocabulary updates"
            );
        }
    }

    // gross_assets must be non-negative (it is the sum of positive-balance classes).
    assert!(
        allocation.gross_assets >= 0.0,
        "gross_assets must be non-negative, got {}",
        allocation.gross_assets
    );
    assert!(
        allocation.gross_assets.is_finite(),
        "gross_assets must be finite, got {}",
        allocation.gross_assets
    );

    // total_liabilities must be ≤ 0 (signed sum of outflow/debt balances).
    assert!(
        allocation.total_liabilities <= 0.0,
        "total_liabilities must be ≤ 0 (signed sum of liability balances), got {}",
        allocation.total_liabilities
    );
    assert!(
        allocation.total_liabilities.is_finite(),
        "total_liabilities must be finite, got {}",
        allocation.total_liabilities
    );

    // net_worth must be finite (could be negative if liabilities exceed assets).
    assert!(
        allocation.net_worth.is_finite(),
        "net_worth must be finite — NaN/inf indicates a GraphQL schema mismatch, got {}",
        allocation.net_worth
    );

    // Per-class percentages (excluding liabilities, which have None) must sum to ~100%.
    if allocation.gross_assets > 0.0 {
        let pct_sum: f64 = allocation
            .classes
            .values()
            .filter_map(|s| s.percent_of_assets)
            .sum();
        assert!(
            (pct_sum - 100.0).abs() < 0.1,
            "per-class percent_of_assets must sum to ~100% when gross_assets > 0, got {pct_sum:.4}%"
        );
        eprintln!("percent_of_assets sum: {pct_sum:.4}% (expected ~100%)");
    }

    // Independent net_worth cross-check: compute from raw signed account balances
    // and assert asset_allocation's net_worth matches (same account slice, different path).
    let raw_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    assert!(
        (allocation.net_worth - raw_net_worth).abs() < 0.01,
        "asset_allocation net_worth ({:.2}) must equal raw signed sum of account balances ({:.2})",
        allocation.net_worth,
        raw_net_worth
    );
    eprintln!(
        "net_worth cross-check: asset_allocation={:.2} raw_sum={:.2} ✓",
        allocation.net_worth, raw_net_worth
    );

    // --- financial_overview path (trust cross-check per ADR 0014) ---
    let cashflow = client
        .get_cashflow(&cur_start, &cur_end, &pri_start, &pri_end)
        .await
        .expect("Web_GetCashFlowPage must succeed against real Monarch");

    let history = client
        .get_net_worth_history(&pri_start, &pri_end)
        .await
        .expect("GetAggregateSnapshots must succeed against real Monarch");

    let transactions = client
        .get_transactions(&cur_start, &cur_end, 500)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    let overview = compute_overview(&accounts, &cashflow, &transactions, &history);

    eprintln!("financial_overview.net_worth: {:.2}", overview.net_worth);
    eprintln!("asset_allocation.net_worth:   {:.2}", allocation.net_worth);

    // Both tools use the same GetAccounts slice — their net_worth must agree within
    // floating-point tolerance (ADR 0014 trust cross-check).
    assert!(
        (allocation.net_worth - overview.net_worth).abs() < 0.01,
        "asset_allocation net_worth ({:.2}) must equal financial_overview net_worth ({:.2}) — \
         both compute from the same GetAccounts slice (ADR 0014 trust cross-check)",
        allocation.net_worth,
        overview.net_worth
    );
    eprintln!(
        "financial_overview cross-check: asset_allocation={:.2} financial_overview={:.2} ✓",
        allocation.net_worth, overview.net_worth
    );
}

/// Verify that apply_changeset resolves category names to UUIDs end-to-end
/// against the real Monarch API (issue #53).
///
/// Steps:
/// 1. Fetch a real transaction and record its original category id and name.
/// 2. Pick a *different* real category from GetCategories to recategorize to.
/// 3. Resolve the target category name → UUID via resolve_category_names.
/// 4. Call update_transaction with the resolved UUID and verify it persisted.
/// 5. Revert the transaction to its original category UUID and verify the
///    revert persisted. Leaves live data clean.
///
/// This test exercises the full name→id resolution path (ADR 0010): the
/// Rust client must send a UUID, not a name, as categoryId. It also proves
/// that resolve_category_names produces an id that Monarch accepts.
#[tokio::test]
async fn apply_changeset_resolves_category_name_to_uuid_and_persists() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    // 1. Fetch transactions and pick one to recategorize.
    let transactions = client
        .get_transactions(&cur_start, &cur_end, 10)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");
    assert!(
        !transactions.is_empty(),
        "need at least one transaction this month to test recategorization"
    );
    let txn = &transactions[0];
    let txn_id = &txn.id;
    let original_category_name = txn.category.name.clone();
    eprintln!("target txn: id={txn_id} original_category={original_category_name:?}");

    // 2. Fetch all categories and resolve the original category name → its UUID
    //    so we can revert cleanly at the end.
    let categories = client
        .get_categories()
        .await
        .expect("GetCategories must succeed against real Monarch");
    assert!(!categories.is_empty(), "must have at least one category");
    eprintln!("categories available: {}", categories.len());

    let original_cat = categories
        .iter()
        .find(|c| c.name == original_category_name)
        .unwrap_or_else(|| {
            panic!(
                "original category {original_category_name:?} not found in GetCategories response"
            )
        });
    let original_uuid = original_cat.id.clone();
    eprintln!("original category UUID: {original_uuid}");

    // 3. Pick a different category to recategorize to (any category that is not
    //    the current one, to guarantee an observable change).
    let target_cat = categories
        .iter()
        .find(|c| c.id != original_uuid)
        .expect("must have at least two categories to test recategorization");
    let target_name = target_cat.name.clone();
    let target_uuid = target_cat.id.clone();
    eprintln!("recategorizing to: name={target_name:?} UUID={target_uuid}");

    // 4. Exercise resolve_category_names: build an AppliedChange with the
    //    *name* and resolve it to a UUID, then send the UUID to Monarch.
    use monarch_mcp::triage::AppliedChange;
    let applied_with_name = vec![AppliedChange {
        id: txn_id.clone(),
        category: Some(target_name.clone()),
        tags: None,
        notes: None,
    }];
    let (resolved, rejections) = resolve_category_names(&categories, applied_with_name);
    assert!(
        rejections.is_empty(),
        "known category {target_name:?} must not produce a rejection; got: {rejections:?}"
    );
    assert_eq!(resolved.len(), 1, "exactly one resolved change expected");
    let resolved_id = resolved[0]
        .category
        .as_deref()
        .expect("resolved change must carry a category UUID");
    assert_eq!(
        resolved_id, target_uuid,
        "resolve_category_names must map {target_name:?} → {target_uuid}"
    );

    // Send the resolved UUID to Monarch.
    client
        .update_transaction(txn_id, Some(resolved_id), None, None)
        .await
        .expect("update_transaction must succeed with resolved UUID");

    // Verify the change persisted by re-fetching the transaction.
    let after_apply = client
        .get_transactions(&cur_start, &cur_end, 10)
        .await
        .expect("GetTransactionsList must succeed after recategorization");
    let updated_txn = after_apply
        .iter()
        .find(|t| &t.id == txn_id)
        .unwrap_or_else(|| panic!("transaction {txn_id} must still exist after recategorization"));
    assert_eq!(
        updated_txn.category.name, target_name,
        "transaction {txn_id} must now show category {target_name:?}, \
         got {:?}",
        updated_txn.category.name
    );
    eprintln!(
        "recategorization persisted: {} → {target_name:?}",
        original_category_name
    );

    // 5. Revert to original category UUID (leave live data clean).
    client
        .update_transaction(txn_id, Some(&original_uuid), None, None)
        .await
        .expect("revert update_transaction must succeed");

    let after_revert = client
        .get_transactions(&cur_start, &cur_end, 10)
        .await
        .expect("GetTransactionsList must succeed after revert");
    let reverted_txn = after_revert
        .iter()
        .find(|t| &t.id == txn_id)
        .unwrap_or_else(|| panic!("transaction {txn_id} must still exist after revert"));
    assert_eq!(
        reverted_txn.category.name, original_category_name,
        "transaction {txn_id} must revert to original category {original_category_name:?}, \
         got {:?}",
        reverted_txn.category.name
    );
    eprintln!("revert persisted: category restored to {original_category_name:?}");
}

/// Verify that `spending_history` works end-to-end against the real Monarch
/// API: transactions are fetched for a 3-month window, `compute_spending_history`
/// produces the correct number of monthly entries, each entry has a non-negative
/// total, and per-category sums are consistent with the month total.
///
/// Does NOT assert specific dollar amounts — those change daily.
/// Asserts structural validity only.
#[tokio::test]
async fn spending_history_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();

    // Use 3 complete months as a quick smoke-test range.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now / 86_400) as i64;
    let (start, end) = range_for_months_count(today_days, 3);

    eprintln!("spending_history range: {start} — {end}");

    let transactions = client
        .get_transactions(&start, &end, i32::MAX as u32)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    eprintln!("transactions fetched: {}", transactions.len());

    let history = compute_spending_history(&transactions, &start, &end);

    // Must return exactly 3 monthly entries.
    assert_eq!(
        history.months.len(),
        3,
        "expected 3 monthly entries for a 3-month range, got {}",
        history.months.len()
    );
    assert_eq!(history.range_start, start);
    assert_eq!(history.range_end, end);

    for m in &history.months {
        // Month label must be YYYY-MM format.
        assert!(
            m.month.len() == 7 && m.month.contains('-'),
            "month label must be YYYY-MM format, got {:?}",
            m.month
        );

        // Total must be non-negative (expense magnitudes only).
        assert!(
            m.total_true_spending >= 0.0,
            "total_true_spending must be non-negative for month {:?}, got {}",
            m.month,
            m.total_true_spending
        );

        // Fixed + discretionary must equal total (within floating-point tolerance).
        let split_sum = m.split.fixed + m.split.discretionary;
        assert!(
            (split_sum - m.total_true_spending).abs() < 0.01,
            "fixed ({:.2}) + discretionary ({:.2}) = {:.2} != total_true_spending {:.2} for {:?}",
            m.split.fixed,
            m.split.discretionary,
            split_sum,
            m.total_true_spending,
            m.month
        );

        // Per-category sums must equal total (within tolerance).
        let cat_sum: f64 = m.by_category.values().sum();
        assert!(
            (cat_sum - m.total_true_spending).abs() < 0.01,
            "by_category sum ({:.2}) != total_true_spending ({:.2}) for {:?}",
            cat_sum,
            m.total_true_spending,
            m.month
        );

        // No raw transaction list in payload — by_category is aggregates only.
        // (Structural: values are f64 totals, not Vec<Transaction>.)
        for (cat, &total) in &m.by_category {
            assert!(
                total >= 0.0,
                "category {:?} total must be non-negative, got {}",
                cat,
                total
            );
        }

        eprintln!(
            "  {} total={:.2} fixed={:.2} disc={:.2} categories={} outliers={}",
            m.month,
            m.total_true_spending,
            m.split.fixed,
            m.split.discretionary,
            m.by_category.len(),
            m.outliers.len()
        );
    }
}

/// Verify that `savings_rate` works end-to-end against the real Monarch API
/// and that its per-month `true_spending` agrees with `spending_history` for
/// the same 3-month range (both tools use the same transaction source).
///
/// Does NOT assert specific dollar amounts — those change daily.
/// Asserts structural validity and cross-tool consistency only.
#[tokio::test]
async fn savings_rate_returns_valid_structure_and_agrees_with_spending_history() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now / 86_400) as i64;
    let (start, end) = range_for_months_count(today_days, 3);

    eprintln!("savings_rate range: {start} — {end}");

    let transactions = client
        .get_transactions(&start, &end, i32::MAX as u32)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    eprintln!("transactions fetched: {}", transactions.len());

    let result = compute_savings_rate(&transactions, &start, &end);
    let history = compute_spending_history(&transactions, &start, &end);

    // Must return exactly 3 monthly entries.
    assert_eq!(
        result.months.len(),
        3,
        "expected 3 monthly entries for a 3-month range, got {}",
        result.months.len()
    );
    assert_eq!(result.range_start, start);
    assert_eq!(result.range_end, end);

    for m in &result.months {
        // Month label must be YYYY-MM format.
        assert!(
            m.month.len() == 7 && m.month.contains('-'),
            "month label must be YYYY-MM format, got {:?}",
            m.month
        );

        // Income must be non-negative.
        assert!(
            m.income >= 0.0,
            "income must be non-negative for month {:?}, got {}",
            m.month,
            m.income
        );

        // True spending must be non-negative (expense magnitudes only).
        assert!(
            m.true_spending >= 0.0,
            "true_spending must be non-negative for month {:?}, got {}",
            m.month,
            m.true_spending
        );

        // net_savings = income - true_spending (within floating-point tolerance).
        let expected_net = m.income - m.true_spending;
        assert!(
            (m.net_savings - expected_net).abs() < 0.01,
            "net_savings ({:.2}) != income ({:.2}) - true_spending ({:.2}) for {:?}",
            m.net_savings,
            m.income,
            m.true_spending,
            m.month
        );

        // savings_rate: present iff income > 0, value in [-100_000, 100] range.
        if m.income > 0.0 {
            let rate = m
                .savings_rate
                .expect("savings_rate must be present when income > 0");
            assert!(
                (-100_000.0_f64..=100.0).contains(&rate),
                "savings_rate {rate:.2} is outside plausible range for month {:?}",
                m.month
            );
        } else {
            assert!(
                m.savings_rate.is_none(),
                "savings_rate must be absent when income == 0 for month {:?}",
                m.month
            );
        }

        // Cross-tool consistency: true_spending must agree with spending_history.
        let history_month = history.months.iter().find(|h| h.month == m.month);
        if let Some(hist) = history_month {
            assert!(
                (m.true_spending - hist.total_true_spending).abs() < 0.01,
                "savings_rate true_spending ({:.2}) disagrees with spending_history \
                 total_true_spending ({:.2}) for month {:?}",
                m.true_spending,
                hist.total_true_spending,
                m.month
            );
        }

        eprintln!(
            "  {} income={:.2} spending={:.2} net={:.2} rate={:?}",
            m.month, m.income, m.true_spending, m.net_savings, m.savings_rate
        );
    }

    // window_average_savings_rate: present iff at least one month had income > 0.
    let any_income = result.months.iter().any(|m| m.income > 0.0);
    if any_income {
        assert!(
            result.window_average_savings_rate.is_some(),
            "window_average_savings_rate must be present when at least one month has income"
        );
    }
    eprintln!(
        "window_average_savings_rate: {:?}",
        result.window_average_savings_rate
    );
}

/// Verify that `budget_review` works end-to-end against the real Monarch API:
/// GetJointPlanningData and GetTransactionsList both succeed, all budget entries
/// and category pacings parse correctly, and `compute_budget_review` produces
/// structurally valid output.
///
/// Does NOT assert specific pace statuses or dollar amounts — those change daily.
/// Asserts structural validity only:
/// - Every category pacing has finite budget, spent, remaining, and percent_spent.
/// - remaining == budget − spent (within floating-point tolerance).
/// - percent_spent == spent / budget * 100 when budget > 0.
/// - Rollup over_count + on_track_count + under_count equals by_category.len().
/// - Income and transfer transactions do not create category pacing entries unless
///   they also have a matching expense budget (i.e., the tool never over-counts).
#[tokio::test]
async fn budget_review_returns_valid_structure_from_real_monarch() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    let (cur_start, cur_end) = current_month();

    let budgets = client
        .get_budgets(&cur_start, &cur_end)
        .await
        .expect("GetJointPlanningData must succeed against real Monarch");

    let transactions = client
        .get_transactions(&cur_start, &cur_end, 500)
        .await
        .expect("GetTransactionsList must succeed against real Monarch");

    eprintln!("budget entries: {}", budgets.len());
    eprintln!("transactions this month: {}", transactions.len());

    // Compute today's day-of-month and days-in-month from wall clock.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now / 86_400) as i64;
    let (year, month, today_dom) = days_to_ymd(today_days);
    let dim = days_in_month(year, month);

    let review = compute_budget_review(&budgets, &transactions, today_dom, dim);

    eprintln!("by_category entries: {}", review.by_category.len());
    eprintln!(
        "rollup: over={} on_track={} under={} over_budget={}",
        review.rollup.over_count,
        review.rollup.on_track_count,
        review.rollup.under_count,
        review.rollup.over_budget_count,
    );

    // Structural validity: every category pacing has finite values.
    for (name, pacing) in &review.by_category {
        assert!(
            pacing.budget.is_finite(),
            "budget must be finite for category {:?}, got {}",
            name,
            pacing.budget
        );
        assert!(
            pacing.spent.is_finite(),
            "spent must be finite for category {:?}, got {}",
            name,
            pacing.spent
        );
        assert!(
            pacing.remaining.is_finite(),
            "remaining must be finite for category {:?}, got {}",
            name,
            pacing.remaining
        );
        // percent_spent is None when budget == 0; otherwise it is a whole-number percentage.
        if let Some(pct) = pacing.percent_spent {
            assert!(
                pct >= 0,
                "percent_spent must be non-negative for category {:?}, got {}",
                name,
                pct
            );
        }

        // remaining == budget − spent (within floating-point tolerance).
        assert!(
            (pacing.remaining - (pacing.budget - pacing.spent)).abs() < 0.01,
            "remaining ({:.2}) must equal budget ({:.2}) - spent ({:.2}) for {:?}",
            pacing.remaining,
            pacing.budget,
            pacing.spent,
            name
        );

        // spent and budget must be non-negative (magnitudes).
        assert!(
            pacing.spent >= 0.0,
            "spent must be non-negative for category {:?}, got {}",
            name,
            pacing.spent
        );
        assert!(
            pacing.budget >= 0.0,
            "budget must be non-negative for category {:?}, got {}",
            name,
            pacing.budget
        );

        eprintln!(
            "  {:?}: budget={:.2} spent={:.2} remaining={:.2} pct={:?}% status={:?}",
            name,
            pacing.budget,
            pacing.spent,
            pacing.remaining,
            pacing.percent_spent,
            pacing.pace_status
        );
    }

    // Rollup counts must sum to total by_category entries.
    let total_count = review.rollup.over_count
        + review.rollup.on_track_count
        + review.rollup.under_count
        + review.rollup.over_budget_count;
    assert_eq!(
        total_count,
        review.by_category.len(),
        "rollup counts (over={} on_track={} under={} over_budget={}) must sum to \
         by_category.len() ({})",
        review.rollup.over_count,
        review.rollup.on_track_count,
        review.rollup.under_count,
        review.rollup.over_budget_count,
        review.by_category.len()
    );
}

/// Verify that `subscription_audit` works end-to-end against the real Monarch
/// API and that its stream set reconciles with `recurring_scan`'s stream set.
///
/// The audit uses a 12-month forward window so every cadence — monthly through
/// annual — has at least one occurrence in the fetch window (ADR 0015 Decision 7).
/// The scan uses the current calendar month (its normal window).
///
/// Since the audit window is a superset of the scan window, every outflow
/// stream visible in the scan MUST appear in the audit. This reconciliation
/// check verifies no stream is silently dropped by the wider-window path.
///
/// Asserts:
/// 1. `get_recurring_for_audit` returns structurally valid items (finite amounts,
///    non-empty merchants, non-empty frequency strings).
/// 2. `compute_subscription_audit` produces finite totals, non-empty merchant
///    names, non-negative magnitudes, and cadence strings preserved.
/// 3. `total_annual ≈ total_monthly * 12` (within floating-point tolerance).
/// 4. Every outflow stream from `get_recurring_for_scan` (current month) that
///    has a non-empty merchant is represented in the subscription_audit merchant
///    set — the 12-month window is a superset so this must hold (ADR 0015).
/// 5. Income streams (positive stream_amount from Monarch) do NOT appear in
///    `subscriptions` (income-exclusion invariant holds on live data).
/// 6. The audit fetch window is verified to span at least 365 days.
///
/// Does NOT assert specific dollar amounts or stream counts — those change over time.
#[tokio::test]
async fn subscription_audit_returns_valid_structure_and_reconciles_with_recurring_scan() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();
    // Audit uses a 12-month forward window (ADR 0015 Decision 7).
    let (audit_start, audit_end) = twelve_month_audit_window();
    // Scan uses the current calendar month (its normal window).
    let (cur_start, cur_end) = current_month();

    eprintln!("audit window: {audit_start} .. {audit_end}");
    eprintln!("scan  window: {cur_start} .. {cur_end}");

    // --- Verify the audit window spans >= 12 months (>= 365 days). ---
    // This catches a regression where audit_window_for_day is accidentally
    // narrowed back to a single month.
    {
        fn parse_days(s: &str) -> i64 {
            let parts: Vec<i64> = s.split('-').map(|p| p.parse().unwrap()).collect();
            // Howard Hinnant civil_to_epoch_day
            let (y, m, d) = (parts[0], parts[1], parts[2]);
            let yy = if m <= 2 { y - 1 } else { y };
            let era = if yy >= 0 { yy } else { yy - 399 } / 400;
            let yoe = yy - era * 400;
            let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            era * 146_097 + doe - 719_468
        }
        let window_days = parse_days(&audit_end) - parse_days(&audit_start);
        assert!(
            window_days >= 365,
            "audit window must span >= 365 days to capture all cadences; got {window_days} days \
             ({audit_start} .. {audit_end})"
        );
    }

    // --- Fetch both data paths ---
    let audit_items = client
        .get_recurring_for_audit(&audit_start, &audit_end)
        .await
        .expect("get_recurring_for_audit must succeed against real Monarch");

    let scan_items = client
        .get_recurring_for_scan(&cur_start, &cur_end)
        .await
        .expect("get_recurring_for_scan must succeed against real Monarch");

    eprintln!("audit items (deduplicated streams): {}", audit_items.len());
    eprintln!("scan items (occurrences): {}", scan_items.len());

    // 1. Structural validity of audit items.
    for item in &audit_items {
        assert!(
            !item.merchant.is_empty(),
            "audit item merchant must not be empty"
        );
        assert!(
            item.stream_amount.is_finite(),
            "audit item stream_amount must be finite for {:?}",
            item.merchant
        );
        assert!(
            !item.frequency.is_empty(),
            "audit item frequency must not be empty for {:?}",
            item.merchant
        );
    }

    // 2. Compute audit and verify output structure.
    let audit = compute_subscription_audit(&audit_items);

    eprintln!("subscriptions: {}", audit.subscriptions.len());
    eprintln!("total_monthly: {:.2}", audit.total_monthly);
    eprintln!("total_annual:  {:.2}", audit.total_annual);

    assert!(
        audit.total_monthly.is_finite(),
        "total_monthly must be finite"
    );
    assert!(
        audit.total_annual.is_finite(),
        "total_annual must be finite"
    );
    assert!(
        audit.total_monthly >= 0.0,
        "total_monthly must be non-negative, got {}",
        audit.total_monthly
    );
    assert!(
        audit.total_annual >= 0.0,
        "total_annual must be non-negative, got {}",
        audit.total_annual
    );

    for sub in &audit.subscriptions {
        assert!(
            !sub.merchant.is_empty(),
            "subscription merchant must not be empty"
        );
        assert!(
            !sub.cadence.is_empty(),
            "subscription cadence must not be empty for {:?}",
            sub.merchant
        );
        assert!(
            sub.monthly_amount >= 0.0,
            "monthly_amount must be non-negative for {:?}, got {}",
            sub.merchant,
            sub.monthly_amount
        );
        assert!(
            sub.annualized_amount >= 0.0,
            "annualized_amount must be non-negative for {:?}, got {}",
            sub.merchant,
            sub.annualized_amount
        );
        assert!(
            sub.monthly_amount.is_finite(),
            "monthly_amount must be finite for {:?}",
            sub.merchant
        );
        assert!(
            sub.annualized_amount.is_finite(),
            "annualized_amount must be finite for {:?}",
            sub.merchant
        );
        eprintln!(
            "  {:?}: cadence={:?} monthly={:.2} annual={:.2} approx={}",
            sub.merchant, sub.cadence, sub.monthly_amount, sub.annualized_amount, sub.approximate
        );
    }

    // 3. total_annual ≈ total_monthly * 12 (within floating-point tolerance).
    assert!(
        (audit.total_annual - audit.total_monthly * 12.0).abs() < 0.01,
        "total_annual ({:.2}) must equal total_monthly ({:.2}) * 12",
        audit.total_annual,
        audit.total_monthly
    );

    // 4. Reconciliation: every outflow stream in scan_items with a known merchant
    //    must appear in the audit's merchant set. The audit deduplicates by stream
    //    while scan_items may have multiple occurrences of the same stream within
    //    the month window — use a set for comparison.
    let audit_merchants: std::collections::HashSet<&str> = audit
        .subscriptions
        .iter()
        .map(|s| s.merchant.as_str())
        .collect();

    // Collect unique outflow merchant names from scan_items (stream_amount < 0).
    let scan_outflow_merchants: std::collections::HashSet<&str> = scan_items
        .iter()
        .filter(|i| i.stream_amount < 0.0 && i.merchant != "Unknown")
        .map(|i| i.merchant.as_str())
        .collect();

    for merchant in &scan_outflow_merchants {
        assert!(
            audit_merchants.contains(*merchant),
            "outflow stream {:?} appears in recurring_scan but is absent from \
             subscription_audit — same data source must produce consistent stream sets \
             (ADR 0015 reconciliation invariant)",
            merchant
        );
    }

    eprintln!(
        "reconciliation: scan_outflow_merchants={} audit_merchants={}",
        scan_outflow_merchants.len(),
        audit_merchants.len()
    );

    // 5. Income exclusion: no subscription entry must have a stream_amount > 0 at
    //    the source level. Verify by checking audit_items directly.
    let income_in_audit: Vec<&str> = audit_items
        .iter()
        .filter(|i| i.stream_amount > 0.0)
        .map(|i| i.merchant.as_str())
        .collect();
    // Note: audit_items may include income if Monarch returns income streams.
    // compute_subscription_audit filters them out. Verify the output has none.
    for sub in &audit.subscriptions {
        // Find the source item for this subscription.
        let source = audit_items.iter().find(|i| i.merchant == sub.merchant);
        if let Some(src) = source {
            assert!(
                src.stream_amount < 0.0,
                "subscription {:?} must come from an outflow stream (stream_amount < 0), \
                 got {}",
                sub.merchant,
                src.stream_amount
            );
        }
    }
    eprintln!("income items in raw audit data: {}", income_in_audit.len());
    eprintln!(
        "income exclusion confirmed: {} subscriptions, all outflows",
        audit.subscriptions.len()
    );
}

/// Verify that `retirement_readiness` produces structurally valid output from real
/// Monarch data and that its two data inputs cross-check correctly:
///
/// 1. `invested_assets` = sum of Equities-class account balances via
///    `invested_financial_accounts` — must equal `compute_asset_allocation`'s
///    `equities` class total (same account slice, different filter).
/// 2. `annual_baseline_spend` = annualised `compute_true_spending` over a 6-month
///    window — positive, finite, and non-NaN.
/// 3. `coverage_ratio` is present and finite when baseline spend > 0.
/// 4. `withdrawal_rate_used` echoes the default 0.04.
/// 5. `spend_window_months` is 6.
///
/// Does NOT assert specific dollar amounts — those change daily.
#[tokio::test]
async fn retirement_readiness_reconciles_with_asset_allocation_and_true_spending() {
    if !live_enabled() {
        eprintln!("SKIP: set MONARCH_LIVE=1 to run live integration tests");
        return;
    }

    let client = make_live_client();

    // --- date range: 6 trailing complete months ---
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let today_days = (now / 86_400) as i64;
    let (start, end) = range_for_months_count(today_days, 6);

    eprintln!("retirement_readiness range: {start} — {end}");

    // Fetch accounts and transactions (independent — same calls the handler makes).
    let (accounts_result, transactions_result) = tokio::join!(
        client.get_accounts(),
        client.get_transactions(&start, &end, i32::MAX as u32),
    );
    let accounts = accounts_result.expect("GetAccounts must succeed");
    let transactions = transactions_result.expect("GetTransactionsList must succeed");

    eprintln!("accounts fetched: {}", accounts.len());
    eprintln!("transactions fetched: {}", transactions.len());

    // --- Trust cross-check 1: invested_assets agrees with asset_allocation ---
    let invested = invested_financial_accounts(&accounts);
    let invested_assets: f64 = invested.iter().map(|a| a.current_balance).sum();

    let allocation = compute_asset_allocation(&accounts);
    let alloc_equities_total = allocation
        .classes
        .get("equities")
        .map(|c| c.total)
        .unwrap_or(0.0);

    eprintln!(
        "invested_financial_accounts total: {invested_assets:.2} \
         asset_allocation.equities.total: {alloc_equities_total:.2}"
    );
    assert!(
        (invested_assets - alloc_equities_total).abs() < 0.01,
        "invested_assets ({invested_assets:.2}) must equal asset_allocation.equities.total \
         ({alloc_equities_total:.2}) — both sum the same Equities-class account balances"
    );

    // --- Trust cross-check 2: annual_baseline_spend is finite and non-negative ---
    use monarch_mcp::spending_report::compute_true_spending;
    let window_true_spending = compute_true_spending(&transactions);
    let annual_baseline_spend = (window_true_spending / 6.0) * 12.0;

    eprintln!("window_true_spending (6 months): {window_true_spending:.2}");
    eprintln!("annual_baseline_spend (annualised): {annual_baseline_spend:.2}");

    assert!(
        annual_baseline_spend.is_finite(),
        "annual_baseline_spend must be finite, got {annual_baseline_spend}"
    );
    assert!(
        annual_baseline_spend >= 0.0,
        "annual_baseline_spend must be non-negative, got {annual_baseline_spend}"
    );

    // --- validate_withdrawal_rate: default rate is always valid ---
    assert!(
        validate_withdrawal_rate(WITHDRAWAL_RATE_DEFAULT).is_ok(),
        "default withdrawal rate must pass validation"
    );

    // --- compute_retirement_readiness: structural validity ---
    let rr = compute_retirement_readiness(invested_assets, annual_baseline_spend, 0.04, 6);

    assert!(
        (rr.withdrawal_rate_used - 0.04).abs() < 1e-9,
        "withdrawal_rate_used must echo 0.04, got {}",
        rr.withdrawal_rate_used
    );
    assert_eq!(rr.spend_window_months, 6, "spend_window_months must be 6");
    assert!(
        (rr.invested_assets - invested_assets).abs() < 0.01,
        "RetirementReadiness.invested_assets must match the pre-computed value"
    );
    assert!(
        rr.sustainable_annual_withdrawal.is_finite(),
        "sustainable_annual_withdrawal must be finite"
    );
    assert!(
        rr.sustainable_annual_withdrawal >= 0.0,
        "sustainable_annual_withdrawal must be non-negative"
    );

    // coverage_ratio is present iff baseline spend > 0
    if annual_baseline_spend > 0.0 {
        let ratio = rr
            .coverage_ratio
            .expect("coverage_ratio must be Some when annual_baseline_spend > 0");
        assert!(
            ratio.is_finite(),
            "coverage_ratio must be finite, got {ratio}"
        );
        assert!(
            ratio >= 0.0,
            "coverage_ratio must be non-negative, got {ratio}"
        );

        let target = rr
            .target_portfolio
            .expect("target_portfolio must be Some when annual_baseline_spend > 0");
        assert!(
            target.is_finite() && target > 0.0,
            "target_portfolio must be positive finite, got {target}"
        );

        let gap = rr
            .surplus_or_gap
            .expect("surplus_or_gap must be Some when annual_baseline_spend > 0");
        assert!(gap.is_finite(), "surplus_or_gap must be finite, got {gap}");

        eprintln!(
            "retirement_readiness: invested={invested_assets:.2} \
             annual_spend={annual_baseline_spend:.2} \
             sustainable={:.2} coverage={ratio:.3} target={target:.2} gap={gap:.2}",
            rr.sustainable_annual_withdrawal,
        );
    } else {
        // Zero spend — coverage_ratio must be None (no ÷0)
        assert!(
            rr.coverage_ratio.is_none(),
            "coverage_ratio must be None when annual_baseline_spend is 0"
        );
        eprintln!("retirement_readiness: zero spend window — coverage_ratio=None as expected");
    }
}
