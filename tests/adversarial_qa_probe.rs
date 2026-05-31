//! Adversarial QA finding (Gate 3, issue #25 Work Unit D).
//!
//! Proves the `possible_reversals` double-pairing bug: a single refund is paired
//! to MULTIPLE charges because `find_possible_reversals` breaks the inner loop
//! after the first matching refund but never marks that refund as consumed. The
//! outer charge loop then re-uses the same refund for the next charge.
//!
//! Reachable with ordinary data (e.g. a duplicate recurring vet/subscription
//! charge with a single refund). Does NOT affect `total_spent` (reversals are
//! surfaced, not netted) — the impact is a misleading advisory `possible_reversals`
//! list that double-counts a single refund.

use monarch_mcp::client::{Cashflow, Category, Transaction};
use monarch_mcp::spending_report::compute_spending_report;

fn expense(merchant: &str, amount: f64, date: &str) -> Transaction {
    Transaction {
        id: format!("{merchant}-{amount}-{date}"),
        amount,
        date: date.to_string(),
        merchant_name: merchant.to_string(),
        category: Category {
            name: "Pets".to_string(),
            group_type: Some("expense".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

fn zero_cashflow() -> Cashflow {
    Cashflow {
        income: 0.0,
        spending: 0.0,
        prior_month_spending: 0.0,
    }
}

/// A single refund can reverse at most ONE charge. Two -$850 charges plus a
/// single +$850 refund must not surface two reversal pairs both citing that
/// one refund.
#[test]
fn single_refund_must_not_pair_to_two_charges() {
    let txns = vec![
        expense("Banfield", -850.0, "2026-05-01"),
        expense("Banfield", -850.0, "2026-05-02"),
        expense("Banfield", 850.0, "2026-05-05"), // only ONE refund
    ];
    let report = compute_spending_report(&txns, &[], &zero_cashflow());
    let pairs_to_same_refund = report
        .possible_reversals
        .iter()
        .filter(|r| r.refund_date == "2026-05-05")
        .count();
    assert!(
        pairs_to_same_refund <= 1,
        "a single refund (2026-05-05) was paired to {} charges — double-counted: {:?}",
        pairs_to_same_refund,
        report.possible_reversals
    );
}
