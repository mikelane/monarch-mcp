//! Adversarial QA (Gate 3) penetration tests for `account_inventory` (issue #50).
//!
//! These tests exercise the PUBLIC compute API (`compute_account_inventory`)
//! against reachable Monarch input shapes that the existing suite did not cover.
//! Each `bug_*` test asserts a documented contract (ADR 0009 / struct doc
//! comments) and is expected to FAIL against HEAD 75e0064, proving the finding.
//!
//! Written by adversarial-qa. Lives in the worktree under test per the QA
//! contract ("No Bug Without a Failing Test").

use monarch_mcp::account_inventory::compute_account_inventory;
use monarch_mcp::client::{Account, AccountSubtype, AccountType};

fn account(type_name: &str, subtype_name: Option<&str>, balance: f64, is_hidden: bool) -> Account {
    Account {
        id: format!("{type_name}-{balance}"),
        display_name: format!("{type_name} account"),
        current_balance: balance,
        balance_was_null: false,
        account_type: AccountType {
            name: type_name.to_string(),
        },
        subtype: subtype_name.map(|n| AccountSubtype {
            name: n.to_string(),
            display: n.to_string(),
        }),
        is_hidden,
    }
}

/// BUG B (reconciliation): rollup `net_worth` must equal the sum of ALL signed
/// account balances (true economic net worth). When a liabilities account
/// carries a POSITIVE balance (an overpaid credit card — a genuine asset),
/// `compute_rollup` does `total_liabilities = liability_total.abs()`, flipping
/// the +500 into a -500 net-worth contribution. Reported net worth is $1000 off.
///
/// Reachable: real Monarch sends positive `currentBalance` for credit-balance
/// cards (statement credits, returns posted after payoff).
#[test]
fn bug_b_overpaid_credit_card_breaks_net_worth_reconciliation() {
    let accounts = vec![
        account("depository", Some("checking"), 10_000.0, false),
        account("credit", Some("credit_card"), 500.0, false), // overpaid → positive
    ];
    let inv = compute_account_inventory(&accounts);

    let true_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    assert!((true_net_worth - 10_500.0).abs() < 0.01, "fixture sanity");

    assert!(
        (inv.rollup.net_worth - true_net_worth).abs() < 0.01,
        "reported net_worth ({}) must equal true net worth ({}); abs()-ing a \
         positive liability-bucket total double-counts the credit as debt",
        inv.rollup.net_worth,
        true_net_worth
    );
}

/// BUG C (reconciliation): an asset-typed account with a NEGATIVE balance
/// (overdrawn checking, margin/short brokerage position) is summed into
/// `total_assets` as a negative number, so `total_assets` no longer matches its
/// documented meaning ("Sum of all positive balances (assets)").
#[test]
fn bug_c_overdrawn_checking_corrupts_total_assets() {
    let accounts = vec![
        account("depository", Some("checking"), -200.0, false), // overdrawn
        account("brokerage", Some("roth"), 100_000.0, false),
    ];
    let inv = compute_account_inventory(&accounts);

    assert!(
        (inv.rollup.total_assets - 100_000.0).abs() < 0.01,
        "total_assets ({}) must be the sum of POSITIVE balances (100000), not \
         contaminated by the overdrawn -200 cash account",
        inv.rollup.total_assets
    );
}

/// BUG A (null-balance flag wired): ADR 0009 line 87 and the issue spec
/// require an account whose `currentBalance` was `null` (coerced to 0.0) to be
/// surfaced with `balance_unknown: true`. After the fix, `Account.balance_was_null`
/// carries the null-ness from the client and `build_entry` reads it correctly.
#[test]
fn bug_a_null_balance_account_is_flagged_unknown() {
    // Simulate what the client now emits for a null currentBalance: 0.0 + balance_was_null=true.
    let was_null = Account {
        id: "depository-null".to_string(),
        display_name: "depository account".to_string(),
        current_balance: 0.0,
        balance_was_null: true,
        account_type: monarch_mcp::client::AccountType {
            name: "depository".to_string(),
        },
        subtype: Some(monarch_mcp::client::AccountSubtype {
            name: "savings".to_string(),
            display: "savings".to_string(),
        }),
        is_hidden: false,
    };
    let inv = compute_account_inventory(&[was_null]);
    let entry = &inv.buckets["cash"].accounts[0];

    assert!(
        entry.balance_unknown,
        "ADR 0009 line 87 requires balance_unknown:true for a null/unsynced balance"
    );
    assert!(
        (entry.balance - 0.0).abs() < f64::EPSILON,
        "null balance must coerce to 0.0, got {}",
        entry.balance
    );
}
