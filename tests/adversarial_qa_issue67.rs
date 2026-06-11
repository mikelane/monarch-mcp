//! Adversarial QA (Gate 3 penetration test) for issue #67 `asset_allocation`.
//!
//! These tests demonstrate confirmed bugs found during adversarial review.
//! They are `#[ignore]`d so they do not break the green suite; each fails when
//! run explicitly, proving the bug.
//!
//! # How to run
//!
//! ```bash
//! cargo test --test adversarial_qa_issue67 -- --ignored --nocapture
//! ```
//!
//! Run a single one:
//!
//! ```bash
//! cargo test --test adversarial_qa_issue67 \
//!   net_worth_diverges_from_financial_overview_with_overdrawn_checking \
//!   -- --ignored --nocapture
//! ```

use monarch_mcp::asset_allocation::compute_asset_allocation;
use monarch_mcp::client::{Account, AccountSubtype, AccountType};
use monarch_mcp::financial_overview::compute_overview;

fn acct(type_name: &str, subtype: Option<&str>, balance: f64) -> Account {
    Account {
        id: format!("{type_name}-{balance}"),
        display_name: format!("{type_name} account"),
        current_balance: balance,
        balance_was_null: false,
        account_type: AccountType {
            name: type_name.to_string(),
        },
        subtype: subtype.map(|n| AccountSubtype {
            name: n.to_string(),
            display: n.to_string(),
        }),
        is_hidden: false,
    }
}

/// BUG 1 — `net_worth` from `asset_allocation` DIVERGES from the signed sum of
/// account balances (and therefore from `financial_overview`) when an asset
/// class has a NET-NEGATIVE total.
///
/// `compute_asset_allocation` computes
/// `gross_assets = Σ class_total.max(0.0)` (excluding liabilities) and
/// `net_worth = gross_assets + total_liabilities`. The `.max(0.0)` clamp drops
/// the negative balance of any *asset* class (e.g. an overdrawn checking
/// account) from `gross_assets`, and that balance is NOT in `total_liabilities`
/// (only the `liabilities` class is). The negative balance therefore vanishes
/// from `net_worth`.
///
/// An overdrawn checking account (`type=depository`, negative balance) is a
/// common, reachable shape. `financial_overview.net_worth` sums raw balances
/// and reports it correctly; `asset_allocation.net_worth` does not. This breaks
/// the ADR 0014 trust cross-check
/// (`asset_allocation.net_worth == financial_overview.net_worth`).
///
/// EXPECTED (correct): net_worth = 100000 + (-2000) = 98000.
/// ACTUAL (buggy):     net_worth = 100000 (the -2000 overdraft is dropped).
#[test]
fn net_worth_diverges_from_financial_overview_with_overdrawn_checking() {
    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        // Overdrawn checking — cash class net is negative.
        acct("depository", Some("checking"), -2_000.0),
    ];

    let alloc = compute_asset_allocation(&accounts);

    // financial_overview's net worth is the signed sum of all balances.
    let raw_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    assert!((raw_net_worth - 98_000.0).abs() < 0.01);

    // ADR 0014 trust cross-check: these MUST agree. They do not.
    assert!(
        (alloc.net_worth - raw_net_worth).abs() < 0.01,
        "BUG: asset_allocation.net_worth ({}) must equal the signed sum of all \
         account balances / financial_overview.net_worth ({}). The negative cash \
         balance was clamped out of gross_assets by `.max(0.0)` and is not in \
         total_liabilities, so it vanished from net_worth.",
        alloc.net_worth,
        raw_net_worth
    );
}

/// BUG 1 (variant) — same divergence proven directly against `compute_overview`,
/// the actual `financial_overview` compute path, not just the raw sum. This is
/// the exact assertion the live integration test makes
/// (`asset_allocation.net_worth == financial_overview.net_worth`), but with an
/// overdrawn-checking account shape the live test's real-data run never
/// exercises — so the green suite gives false confidence.
#[test]
fn net_worth_disagrees_with_compute_overview_when_asset_class_is_negative() {
    use monarch_mcp::client::{Cashflow, NetWorthHistory};

    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        acct("depository", Some("checking"), -2_000.0),
    ];

    let alloc = compute_asset_allocation(&accounts);

    let cashflow = Cashflow {
        income: 0.0,
        spending: 0.0,
        prior_month_spending: 0.0,
    };
    let history = NetWorthHistory {
        prior_month_net_worth: 0.0,
    };
    let overview = compute_overview(&accounts, &cashflow, &[], &history);

    assert!(
        (alloc.net_worth - overview.net_worth).abs() < 0.01,
        "BUG (ADR 0014 trust cross-check): asset_allocation.net_worth ({}) must \
         equal financial_overview.net_worth ({}). They diverge by {} because the \
         negative asset-class total is clamped out of gross_assets.",
        alloc.net_worth,
        overview.net_worth,
        (alloc.net_worth - overview.net_worth).abs()
    );
}

/// BUG 1 (margin variant) — a brokerage account with a NEGATIVE balance (margin
/// loan / short position) is dropped from net_worth entirely. The prompt's
/// attack-surface item #5 asks specifically how a brokerage with a negative
/// balance is handled: it is classified `equities`, then its negative total is
/// clamped to 0 in gross_assets and never re-added, so a -10000 margin balance
/// silently disappears from net_worth.
///
/// EXPECTED: net_worth = 50000 + (-10000) = 40000.
/// ACTUAL:   net_worth = 50000 (margin balance dropped).
#[test]
fn negative_brokerage_margin_balance_is_dropped_from_net_worth() {
    let accounts = vec![
        acct("depository", Some("checking"), 50_000.0),
        // Single brokerage account in a margin/short position.
        acct("brokerage", Some("brokerage"), -10_000.0),
    ];

    let alloc = compute_asset_allocation(&accounts);
    let raw_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    assert!((raw_net_worth - 40_000.0).abs() < 0.01);

    assert!(
        (alloc.net_worth - raw_net_worth).abs() < 0.01,
        "BUG: net_worth ({}) must equal signed sum ({}). The -10000 brokerage \
         margin balance was clamped out of gross_assets and lost.",
        alloc.net_worth,
        raw_net_worth
    );
}
