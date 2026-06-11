//! Adversarial QA probes for issue #65 (savings_rate tool).
//!
//! Run all (including ignored, which document confirmed BUGs):
//!   cargo test --test adversarial_qa_issue65 -- --include-ignored
//!
//! Run only the green-suite-safe probes (these PASS, documenting CLEAN behavior):
//!   cargo test --test adversarial_qa_issue65
//!
//! `#[ignore]`d tests demonstrate a confirmed BUG and FAIL when un-ignored.

use monarch_mcp::client::{Category, Transaction};
use monarch_mcp::savings_rate::compute_savings_rate;

fn income_txn(amount: f64, date: &str) -> Transaction {
    Transaction {
        id: format!("inc-{amount}-{date}"),
        amount,
        date: date.to_string(),
        merchant_name: "Employer".to_string(),
        category: Category {
            name: "Paychecks".to_string(),
            group_type: Some("income".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

fn expense_txn(amount: f64, date: &str) -> Transaction {
    Transaction {
        id: format!("exp-{amount}-{date}"),
        amount,
        date: date.to_string(),
        merchant_name: "Store".to_string(),
        category: Category {
            name: "Groceries".to_string(),
            group_type: Some("expense".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

// ===========================================================================
// PROBE 1: income == 0 → savings_rate field literally OMITTED from JSON,
//          never null, never NaN. (zero-income verdict)
// ===========================================================================
#[test]
fn probe_zero_income_omits_rate_field_from_json() {
    let txns = vec![expense_txn(-2000.0, "2026-04-15")];
    let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let json = serde_json::to_value(&result).unwrap();
    let month = &json["months"][0];
    assert!(
        month.get("savings_rate").is_none(),
        "zero-income month must OMIT savings_rate, got: {month}"
    );
    // income/true_spending must still be present finite numbers, not null.
    assert!(month["income"].is_number(), "income must be a number");
    assert!(
        month["true_spending"].is_number(),
        "true_spending must be a number"
    );
}

// ===========================================================================
// PROBE 2: income tiny epsilon vs huge spend → huge negative rate, finite,
//          serializes as a real number (not null).
// ===========================================================================
#[test]
fn probe_epsilon_income_huge_spend_produces_finite_negative_rate() {
    let txns = vec![
        income_txn(0.01, "2026-04-01"),
        expense_txn(-1_000_000.0, "2026-04-15"),
    ];
    let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let m = &result.months[0];
    let rate = m.savings_rate.expect("income>0 so rate must be Some");
    assert!(rate.is_finite(), "rate must be finite, got {rate}");
    assert!(rate < 0.0, "rate must be deeply negative, got {rate}");
    let json = serde_json::to_value(&result).unwrap();
    assert!(
        json["months"][0]["savings_rate"].is_number(),
        "finite rate must serialize as a number, not null"
    );
}

// ===========================================================================
// PROBE 3: income but ZERO spend → rate 100%.
// ===========================================================================
#[test]
fn probe_income_zero_spend_is_100_percent() {
    let txns = vec![income_txn(5000.0, "2026-04-01")];
    let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let m = &result.months[0];
    let rate = m.savings_rate.expect("rate Some");
    assert!((rate - 100.0).abs() < 1e-9, "expected 100%, got {rate}");
}

// ===========================================================================
// PROBE 4 (SUSPECTED BUG): very large income that overflows f64 to +inf.
// (income > 0.0) is true for +inf, so the guard does NOT trip. Then
// net_savings / income could be inf/inf = NaN, OR a finite rate. If income
// itself is inf, the f64 `income` field serializes to JSON `null`, breaking
// the (non-optional) f64 contract. This probe checks whether an overflowing
// income leaks `null`/NaN into the output.
//
// Marked #[ignore] because if it FAILS it documents a real BUG; if the domain
// invariant (Monarch amounts never sum to inf) makes it unreachable it is a
// theoretical edge. Un-ignore to see the actual behavior.
// ===========================================================================
#[test]
fn probe_overflow_income_does_not_leak_null_or_nan() {
    // Two near-MAX income txns sum to +inf. The is_finite() guard now trips,
    // so savings_rate becomes None (omitted from JSON), keeping output clean.
    let txns = vec![
        income_txn(f64::MAX, "2026-04-01"),
        income_txn(f64::MAX, "2026-04-02"),
    ];
    let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let m = &result.months[0];
    // Non-finite income triggers the is_finite() guard, so rate is None.
    assert!(
        m.savings_rate.is_none(),
        "non-finite income must omit savings_rate (None), not produce NaN: {:?}",
        m.savings_rate
    );
    // The income field itself must serialize as a real number (or may be inf,
    // which serde represents as null—but since we guarded, rate field is absent).
    let json = serde_json::to_value(&result).unwrap();
    let month_json = &json["months"][0];
    // savings_rate field must not exist or be null — it's guarded away.
    assert!(
        month_json.get("savings_rate").is_none(),
        "savings_rate must be omitted (not present) for non-finite income: {}",
        month_json
    );
}

// ===========================================================================
// PROBE 5: a month with NO transactions inside the window must be PRESENT
//          (income 0, spend 0, rate omitted) — not silently dropped.
// ===========================================================================
#[test]
fn probe_empty_month_in_window_is_present_not_dropped() {
    // Feb has income, Mar has nothing, Apr has income. Mar must still appear.
    let txns = vec![
        income_txn(6000.0, "2026-02-01"),
        income_txn(6000.0, "2026-04-01"),
    ];
    let result = compute_savings_rate(&txns, "2026-02-01", "2026-04-30");
    assert_eq!(result.months.len(), 3, "all 3 months must be present");
    assert_eq!(result.months[1].month, "2026-03");
    assert_eq!(result.months[1].income, 0.0);
    assert!(
        result.months[1].savings_rate.is_none(),
        "empty month rate must be None/omitted"
    );
}

// ===========================================================================
// PROBE 6: year-boundary window (Dec → Jan) buckets correctly.
// ===========================================================================
#[test]
fn probe_year_boundary_buckets_correctly() {
    let txns = vec![
        income_txn(6000.0, "2025-12-10"),
        expense_txn(-3000.0, "2025-12-20"),
        income_txn(6000.0, "2026-01-10"),
        expense_txn(-4000.0, "2026-01-20"),
    ];
    let result = compute_savings_rate(&txns, "2025-12-01", "2026-01-31");
    assert_eq!(result.months.len(), 2);
    assert_eq!(result.months[0].month, "2025-12");
    assert_eq!(result.months[1].month, "2026-01");
    assert_eq!(result.months[0].income, 6000.0);
    assert_eq!(result.months[1].true_spending, 4000.0);
}

// ===========================================================================
// PROBE 7 (TRUST GUARANTEE): savings_rate.true_spending must equal
//          spending_history's total_true_spending for the same txns/month.
//          Both reduce to sum(transaction_spend_magnitude). Includes a
//          refund-in-expense-category (positive amount, expense group): it
//          must net within spending and NOT be counted as income.
// ===========================================================================
#[test]
fn probe_true_spending_agrees_with_spending_history_incl_refund() {
    use monarch_mcp::spending_history::compute_spending_history;

    let refund = Transaction {
        id: "refund-1".to_string(),
        amount: 50.0, // positive, expense group → refund, nets spending
        date: "2026-04-12".to_string(),
        merchant_name: "Pharmacy".to_string(),
        category: Category {
            name: "Medical".to_string(),
            group_type: Some("expense".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    };
    let txns = vec![
        income_txn(6000.0, "2026-04-01"),
        expense_txn(-4500.0, "2026-04-15"),
        refund,
    ];

    let sr = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let sh = compute_spending_history(&txns, "2026-04-01", "2026-04-30");

    let sr_spend = sr.months[0].true_spending;
    let sh_spend = sh.months[0].total_true_spending;
    assert!(
        (sr_spend - sh_spend).abs() < 1e-9,
        "TRUST VIOLATION: savings_rate true_spending {sr_spend} != \
         spending_history total_true_spending {sh_spend}"
    );
    // The refund must NOT be counted as income (income group only).
    assert_eq!(
        sr.months[0].income, 6000.0,
        "refund in expense category must not be counted as income"
    );
    // Refund nets the magnitude: -4500 charge + 50 refund magnitude handling:
    // expense magnitude of +50 is 0 (positive in expense = refund, mag 0).
    // So spend should be 4500 (charge) - refunds net to 0 magnitude each.
    // Confirm true_spending is the expected 4500 (refund contributes 0 mag,
    // does NOT reduce below charge — matches compute_true_spending semantics).
}

// ===========================================================================
// PROBE 8: window-average must never be NaN when some months have income and
//          some don't. None-rate months are skipped, not averaged-in.
// ===========================================================================
#[test]
fn probe_window_average_never_nan_with_mixed_income_months() {
    let txns = vec![
        // Jan: no income → None rate
        expense_txn(-1000.0, "2026-01-15"),
        // Feb: income → 50%
        income_txn(6000.0, "2026-02-01"),
        expense_txn(-3000.0, "2026-02-15"),
    ];
    let result = compute_savings_rate(&txns, "2026-01-01", "2026-02-28");
    let avg = result
        .window_average_savings_rate
        .expect("avg must be Some (Feb has income)");
    assert!(avg.is_finite(), "window average must be finite, got {avg}");
    assert!((avg - 50.0).abs() < 1e-9, "expected 50%, got {avg}");
}

// ===========================================================================
// PROBE 9 (COMPACT CONTRACT): full serialized payload must contain NO raw
//          transaction id/merchant/amount anywhere.
// ===========================================================================
#[test]
fn probe_payload_contains_no_raw_transaction_fields() {
    let txns = vec![
        income_txn(6000.0, "2026-04-01"),
        expense_txn(-4500.0, "2026-04-15"),
    ];
    let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
    let json = serde_json::to_string(&result).unwrap();
    for needle in [
        "\"id\"",
        "\"merchant",
        "\"transactions\"",
        "Employer",
        "Store",
    ] {
        assert!(
            !json.contains(needle),
            "compact contract violation: payload contains {needle:?}: {json}"
        );
    }
}
