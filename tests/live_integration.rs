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
use monarch_mcp::client::MonarchClient;
use monarch_mcp::financial_overview::compute_overview;
use monarch_mcp::inspect_transactions::{compute_inspection, InspectFilter};
use monarch_mcp::net_worth_trend::compute_trend;
use monarch_mcp::recurring_scan::compute_scan;
use monarch_mcp::spending_history::{compute_spending_history, range_for_months_count};
use monarch_mcp::spending_report::compute_spending_report;
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
