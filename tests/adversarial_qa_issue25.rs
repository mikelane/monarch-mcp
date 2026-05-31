//! Adversarial QA regression tests for issue #25 (true spending agreement).
//!
//! These tests guard against fetch-layer divergence between `financial_overview`
//! and `spending_report`: both tools must sum spending from the SAME transaction
//! slice. A per-tool limit cap (e.g. 100 or 500) in one handler but not the
//! other breaks the invariant for months exceeding that cap.

use monarch_mcp::client::{Cashflow, Category, NetWorthHistory, Transaction};
use monarch_mcp::financial_overview::compute_overview;
use monarch_mcp::spending_report::compute_spending_report;

// ---------------------------------------------------------------------------
// Helper — builds a minimal expense transaction
// ---------------------------------------------------------------------------

fn expense_txn(id: &str, amount: f64) -> Transaction {
    Transaction {
        id: id.to_string(),
        amount,
        date: "2026-05-15".to_string(),
        merchant_name: "Merchant".to_string(),
        category: Category {
            name: "Groceries".to_string(),
            group_type: Some("expense".to_string()),
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

fn zero_history() -> NetWorthHistory {
    NetWorthHistory {
        prior_month_net_worth: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Bug regression: fetch-layer truncation divergence
//
// financial_overview.cashflow.spending must equal spending_report.total_spent
// when both are computed from the SAME transaction slice. This is the pure-
// function side of the contract: if the handlers feed different slices (e.g.
// one capped at 100, the other unlimited), the values diverge.
//
// This test proves the PURE functions agree for 120 transactions, so that any
// future fetch-layer cap regression is caught at the integration level by the
// live test in live_integration.rs.
// ---------------------------------------------------------------------------

#[test]
fn financial_overview_and_spending_report_agree_on_120_transactions() {
    // Seed 120 identical $10.00 expense transactions.
    let txns: Vec<Transaction> = (0..120)
        .map(|i| expense_txn(&format!("txn-{i}"), -10.0))
        .collect();

    let expected_spending = 1200.0_f64; // 120 × $10.00

    let report = compute_spending_report(&txns, &[], &zero_cashflow());
    let overview = compute_overview(&[], &zero_cashflow(), &txns, &zero_history());

    assert_eq!(
        report.total_spent, expected_spending,
        "spending_report must sum all 120 transactions"
    );
    assert_eq!(
        overview.cashflow.spending, expected_spending,
        "financial_overview must sum all 120 transactions"
    );
    assert_eq!(
        overview.cashflow.spending, report.total_spent,
        "financial_overview.cashflow.spending must equal spending_report.total_spent \
         (agreement invariant — fetch-layer must use identical limits)"
    );
}
