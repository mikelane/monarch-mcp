//! Adversarial QA (Gate 3) probes for the `spending_history` tool (issue #54).
//!
//! These tests target the pure public API of `monarch_mcp::spending_history`.
//! Each `#[ignore]`d test documents a CONFIRMED or SUSPECTED defect found during
//! penetration testing; they are excluded from the green suite so the baseline
//! stays green, and are run explicitly with:
//!
//! ```bash
//! cargo test --test adversarial_qa_issue54 -- --ignored
//! ```
//!
//! Tests WITHOUT `#[ignore]` document behavior that is CLEAN (defensive proof
//! that a suspected attack vector is actually handled) and run in the normal
//! suite.

use monarch_mcp::client::{Category, Transaction};
use monarch_mcp::spending_history::{
    compute_spending_history, is_fixed_category, range_for_months_count,
};

fn expense(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
    Transaction {
        id: format!("{merchant}-{amount}-{date}"),
        amount,
        date: date.to_string(),
        merchant_name: merchant.to_string(),
        category: Category {
            name: category.to_string(),
            group_type: Some("expense".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

// ===========================================================================
// BUG 1 (CONFIRMED): range_for_months_count(_, 0) panics on u32 underflow.
//
// `range_for_months_count` is a PUBLIC function (re-exported and imported by
// tests/live_integration.rs). It computes `subtract_months(.., months - 1)`.
// With months == 0, `months - 1` underflows u32 → panics in debug
// ("attempt to subtract with overflow") and wraps to u32::MAX in release,
// causing a multi-billion-iteration hang in subtract_months's loop.
//
// The tools.rs handler clamps months to 1..=24 before calling, so the MCP
// boundary is currently safe — but the public function itself has no guard,
// and the live-integration test calls it directly. A future caller (or a
// refactor that drops the clamp) reintroduces the panic. The fix is to clamp
// or saturate inside range_for_months_count itself.
// ===========================================================================
#[test]
fn range_for_months_count_zero_does_not_panic() {
    // today = 2026-05-15 (epoch day computed inline below is not needed; any
    // valid epoch day works). 2026-05-15 = 20588 epoch days.
    let today: i64 = 20588;
    // This call panics in debug with "attempt to subtract with overflow".
    let (start, end) = range_for_months_count(today, 0);
    // If it ever returns, the range must at least be well-formed (start <= end).
    assert!(
        start <= end,
        "0-month range must be well-formed, got start={start} end={end}"
    );
}

// ===========================================================================
// BUG 2 (CONFIRMED): is_fixed_category substring matching mis-classifies
// discretionary categories whose names merely CONTAIN a fixed pattern.
//
// "rent" is a substring of many unrelated words. A user category named
// "Apparent Overspending", "Current Subscriptions", or "Concert Rentals"
// is silently bucketed as FIXED. Likewise "Accidental Purchases" matches
// "dental", and "Reinsurance" matches "insurance".
//
// The fixed+discretionary == total invariant still holds (the txn lands in
// exactly one bucket), so this does not break the BDD sum assertion — but the
// SPLIT itself is wrong, which is the whole point of the fixed/discretionary
// feature (retirement-spending analysis). A user reading "fixed: $X" gets a
// number that includes discretionary spend. This is a correctness defect in
// the classification, not just cosmetics.
// ===========================================================================
#[test]
fn is_fixed_category_rejects_substring_false_positives() {
    // None of these are fixed obligations; each merely contains a pattern.
    assert!(
        !is_fixed_category("Apparent Overspending"),
        "'Apparent Overspending' wrongly matched 'rent'"
    );
    assert!(
        !is_fixed_category("Concert Rentals"),
        "'Concert Rentals' wrongly matched 'rent'"
    );
    assert!(
        !is_fixed_category("Accidental Purchases"),
        "'Accidental Purchases' wrongly matched 'dental'"
    );
}

#[test]
fn discretionary_category_leaks_into_fixed_bucket() {
    // "Concert Rentals" is discretionary entertainment, but contains "rent".
    let txns = vec![
        expense("Ticketmaster", -300.0, "Concert Rentals", "2026-03-10"),
        expense("Restaurant", -100.0, "Dining", "2026-03-15"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    let m = &history.months[0];
    // Total is correct ($400), but the SPLIT is wrong: the $300 concert spend
    // is leaked into `fixed` instead of `discretionary`.
    assert_eq!(
        m.split.discretionary, 400.0,
        "All $400 is discretionary; got fixed={} discretionary={}",
        m.split.fixed, m.split.discretionary
    );
    assert_eq!(
        m.split.fixed, 0.0,
        "Nothing here is a fixed obligation; got fixed={}",
        m.split.fixed
    );
}

// ===========================================================================
// CLEAN (defensive proof): the fixed + discretionary == total invariant holds
// even when a category matches NO bucket cleanly — every magnitude>0 txn lands
// in exactly one bucket, so the sum is preserved. This is the BDD's core
// invariant; it survives substring false-positives (only the LABEL is wrong).
// ===========================================================================
#[test]
fn fixed_plus_discretionary_always_equals_total_even_with_odd_categories() {
    let txns = vec![
        expense("A", -300.0, "Concert Rentals", "2026-03-10"), // false-positive fixed
        expense("B", -100.0, "Dining", "2026-03-15"),          // discretionary
        expense("C", -250.0, "Mortgage", "2026-03-01"),        // genuinely fixed
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    let m = &history.months[0];
    assert!(
        (m.split.fixed + m.split.discretionary - m.total_true_spending).abs() < 1e-9,
        "split must always sum to total: fixed={} disc={} total={}",
        m.split.fixed,
        m.split.discretionary,
        m.total_true_spending
    );
}

// ===========================================================================
// CLEAN (defensive proof): a reversed range (start > end) yields a well-formed
// EMPTY history, not a panic. enumerate_months_in_range breaks immediately.
// ===========================================================================
#[test]
fn reversed_range_yields_empty_months_not_panic() {
    let history = compute_spending_history(&[], "2026-04-30", "2026-03-01");
    assert!(
        history.months.is_empty(),
        "reversed range should produce zero months, got {}",
        history.months.len()
    );
}

// ===========================================================================
// CLEAN (defensive proof): income/transfer transactions never appear as
// outliers (magnitude 0 excludes them from the outlier grouping) and a month
// of pure income reports total 0 but is still PRESENT (not omitted).
// ===========================================================================
#[test]
fn pure_income_month_present_with_zero_total_and_no_outliers() {
    let income = Transaction {
        id: "salary".into(),
        amount: 9999.0,
        date: "2026-03-01".into(),
        merchant_name: "Employer".into(),
        category: Category {
            name: "Paychecks".into(),
            group_type: Some("income".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    };
    let history = compute_spending_history(&[income], "2026-03-01", "2026-03-31");
    assert_eq!(history.months.len(), 1);
    assert_eq!(history.months[0].total_true_spending, 0.0);
    assert!(history.months[0].outliers.is_empty());
    assert!(history.months[0].by_category.is_empty());
}

// ===========================================================================
// CLEAN (defensive proof): the outlier threshold is INCLUSIVE at exactly 3x.
// Two $100 peers + one $300 candidate: others_avg = $100, 3x = $300, and the
// candidate ($300) is >= threshold so it IS flagged. Documents the boundary.
// ===========================================================================
#[test]
fn outlier_threshold_is_inclusive_at_exactly_three_x() {
    let txns = vec![
        expense("P1", -100.0, "Dining", "2026-03-05"),
        expense("P2", -100.0, "Dining", "2026-03-10"),
        expense("Spike", -300.0, "Dining", "2026-03-20"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    assert!(
        history.months[0]
            .outliers
            .iter()
            .any(|o| o.merchant == "Spike"),
        "exactly-3x transaction should be flagged (>= threshold)"
    );
}
