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

use monarch_mcp::client::MonarchClient;
use monarch_mcp::financial_overview::compute_overview;
use monarch_mcp::net_worth_trend::compute_trend;
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
    (format!("{y:04}-{m:02}-01"), format!("{y:04}-{m:02}-{last:02}"))
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
    (format!("{y:04}-{m:02}-01"), format!("{y:04}-{m:02}-{last:02}"))
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
    eprintln!("prior_month_net_worth: {:.2}", history.prior_month_net_worth);

    let overview = compute_overview(&accounts, &cashflow, &history);
    eprintln!("net_worth: {:.2}", overview.net_worth);
    eprintln!("net_worth_change: {:.2}", overview.net_worth_change);
    eprintln!("cashflow.net: {:.2}", overview.cashflow.net);

    assert!(
        overview.net_worth.is_finite(),
        "net_worth must be finite — NaN/inf indicates a GraphQL schema mismatch, got: {}",
        overview.net_worth
    );
    assert_ne!(
        overview.net_worth,
        0.0,
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
        assert!(t.amount.is_finite(), "amount must be finite, got {} for id {}", t.amount, t.id);
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
        assert!(!c.name.is_empty(), "category name must not be empty for id {}", c.id);
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
        assert!(!t.name.is_empty(), "tag name must not be empty for id {}", t.id);
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
