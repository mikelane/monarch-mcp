//! Adversarial QA (Gate 3) for the `inspect_transactions` compound tool.
//!
//! These tests exercise `compute_inspection` / `matches_filter` / the summary
//! arithmetic through the crate's public API. They are deliberately hostile:
//! boundary values, empty-string filters, sign-convention edges, substring
//! false-positives, and the refund/charge split.
//!
//! The original headline finding was the **empty-string filter**: a `category`
//! (or `merchant`) filter of `""` matched EVERY transaction because substring
//! containment of the empty string is always true, presenting a whole-account
//! dump as a single-category inspection. That bug is now **fixed** — an empty
//! or whitespace-only needle is coerced to "no constraint" inside
//! `matches_filter` (see the `needle.is_empty()` guard in
//! `src/inspect_transactions.rs`). The empty/whitespace-filter tests below pin
//! the fixed contract: `Some("")` is transparent and behaves exactly like
//! `None`, so a regression that re-introduced the whole-account dump (or the
//! opposite wrong fix — returning zero results) would fail here.

use monarch_mcp::client::{Category, Transaction};
use monarch_mcp::inspect_transactions::{compute_inspection, InspectFilter};

fn txn(id: &str, merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
    Transaction {
        id: id.to_string(),
        amount,
        date: date.to_string(),
        merchant_name: merchant.to_string(),
        category: Category {
            name: category.to_string(),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

// ---------------------------------------------------------------------------
// Contract: empty/whitespace filter ≡ no filter on that dimension.
//
// The fix is NOT to return zero results — that would be a different bug.
// The fix is that `Some("")` is transparent: it behaves exactly like `None`,
// so the caller gets the honest no-filter view rather than a misleading
// "scoped" result that is actually the whole account dressed up as a single
// category inspection.
// ---------------------------------------------------------------------------

#[test]
fn empty_string_category_filter_is_equivalent_to_no_filter() {
    let txns = vec![
        txn("t1", "Petco", -12000.0, "Pets", "2026-05-10"),
        txn("t2", "CVS", -3300.0, "Medical", "2026-05-12"),
        txn("t3", "Direct Deposit", 5000.0, "Income", "2026-05-01"),
    ];
    // `Some("")` must be transparent — same result as the explicit no-filter.
    let with_empty = compute_inspection(
        &txns,
        &InspectFilter {
            category: Some(String::new()),
            merchant: None,
        },
    );
    let with_none = compute_inspection(&txns, &InspectFilter::default());

    assert_eq!(
        with_empty.summary, with_none.summary,
        "empty-string category filter should be transparent (≡ no filter), \
         but summary differed"
    );
    assert_eq!(
        with_empty.transactions, with_none.transactions,
        "empty-string category filter should be transparent (≡ no filter), \
         but transaction list differed"
    );
}

#[test]
fn empty_string_merchant_filter_is_equivalent_to_no_filter() {
    let txns = vec![
        txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
        txn("t2", "CVS", -50.0, "Medical", "2026-05-11"),
    ];
    let with_empty = compute_inspection(
        &txns,
        &InspectFilter {
            category: None,
            merchant: Some(String::new()),
        },
    );
    let with_none = compute_inspection(&txns, &InspectFilter::default());

    assert_eq!(
        with_empty.summary, with_none.summary,
        "empty-string merchant filter should be transparent (≡ no filter)"
    );
    assert_eq!(
        with_empty.transactions, with_none.transactions,
        "empty-string merchant filter should be transparent (≡ no filter)"
    );
}

#[test]
fn whitespace_only_category_filter_is_equivalent_to_no_filter() {
    // Whitespace-only ("  ") must coerce to no filter just like empty string.
    let txns = vec![
        txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
        txn("t2", "CVS", -50.0, "Medical", "2026-05-11"),
    ];
    let with_ws = compute_inspection(
        &txns,
        &InspectFilter {
            category: Some("  ".to_string()),
            merchant: None,
        },
    );
    let with_none = compute_inspection(&txns, &InspectFilter::default());

    assert_eq!(with_ws.summary, with_none.summary);
    assert_eq!(with_ws.transactions, with_none.transactions);
}

// ---------------------------------------------------------------------------
// CLEAN-confirmations (these are expected to PASS — they pin correct behaviour).
// ---------------------------------------------------------------------------

#[test]
fn refund_and_charge_split_is_exact() {
    let txns = vec![
        txn("t1", "Vet", -100.0, "Pets", "2026-05-10"),
        txn("t2", "Vet Refund", 30.0, "Pets", "2026-05-15"),
    ];
    let r = compute_inspection(&txns, &InspectFilter::default());
    assert_eq!(r.summary.total_count, 2);
    assert!((r.summary.total_outflow - 100.0).abs() < 1e-9);
    assert!((r.summary.total_inflow - 30.0).abs() < 1e-9);
    assert!((r.summary.net_amount - (-70.0)).abs() < 1e-9);
}

#[test]
fn zero_amount_is_counted_but_does_not_distort_split() {
    let txns = vec![
        txn("t1", "Adjustment", 0.0, "General", "2026-05-10"),
        txn("t2", "Petco", -100.0, "Pets", "2026-05-11"),
        txn("t3", "Refund", 25.0, "Pets", "2026-05-12"),
    ];
    let r = compute_inspection(&txns, &InspectFilter::default());
    assert_eq!(r.summary.total_count, 3);
    assert!((r.summary.total_inflow - 25.0).abs() < 1e-9);
    assert!((r.summary.total_outflow - 100.0).abs() < 1e-9);
    // The zero-amount line must not perturb the net: 25 - 100 = -75.
    assert!((r.summary.net_amount - (-75.0)).abs() < 1e-9);
}

#[test]
fn refund_only_category_shows_inflow_and_zero_outflow() {
    let txns = vec![
        txn("t1", "Refund A", 40.0, "Returns", "2026-05-10"),
        txn("t2", "Refund B", 60.0, "Returns", "2026-05-11"),
    ];
    let r = compute_inspection(&txns, &InspectFilter::default());
    assert_eq!(r.summary.total_outflow, 0.0);
    assert!((r.summary.total_inflow - 100.0).abs() < 1e-9);
}

// SUSPICIOUS-confirmation: substring match yields cross-merchant false
// positives ("app" matches both "Apple" and "Appliance Store"). Documented as
// substring semantics, so this PASSES; pinned so a future exact-match change is
// a conscious decision, not a silent regression.
#[test]
fn substring_filter_matches_unrelated_merchants_sharing_a_prefix() {
    let txns = vec![
        txn("t1", "Apple", -999.0, "Electronics", "2026-05-10"),
        txn("t2", "Appliance Store", -50.0, "Home", "2026-05-11"),
        txn("t3", "Petco", -20.0, "Pets", "2026-05-12"),
    ];
    let filter = InspectFilter {
        category: None,
        merchant: Some("app".to_string()),
    };
    let r = compute_inspection(&txns, &filter);
    assert_eq!(r.summary.total_count, 2);
}
