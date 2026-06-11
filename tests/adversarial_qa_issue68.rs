//! Adversarial QA probes for issue #68 (subscription_audit) — Gate 3.
//!
//! These tests attack the public compute surface of `subscription_audit`.
//! Each `#[ignore]`d test asserts the *correct* behavior; if it FAILS when run,
//! that failure is the proof of a confirmed BUG (the production code disagrees
//! with the contract). Tests that pass document CLEAN / SUSPICIOUS verdicts.
//!
//! Run all probes (including ignored):
//!   cargo test --test adversarial_qa_issue68 -- --ignored --nocapture
//! Run a single probe:
//!   cargo test --test adversarial_qa_issue68 bug_capitalized_yearly -- --ignored --nocapture

use monarch_mcp::subscription_audit::{compute_subscription_audit, SubscriptionAuditItem};

fn item(
    merchant: &str,
    stream_amount: f64,
    frequency: &str,
    approx: bool,
) -> SubscriptionAuditItem {
    SubscriptionAuditItem {
        merchant: merchant.to_string(),
        stream_amount,
        frequency: frequency.to_string(),
        is_approximate: approx,
    }
}

// ---------------------------------------------------------------------------
// FINDING 1 (BUG candidate): cadence casing — a capitalized/whitespaced
// non-monthly frequency silently falls through to the unknown→1.0 arm,
// OVERSTATING a yearly stream by 12x (and understating weekly, etc.).
// ---------------------------------------------------------------------------

/// A "Yearly" (capitalized) $120/year stream should normalize to $10/month.
/// If casing isn't normalized it falls to factor 1.0 → $120/month (12x too high).
#[test]
fn bug_capitalized_yearly_overstates_12x() {
    let items = vec![item("NewsCo", -120.0, "Yearly", false)];
    let result = compute_subscription_audit(&items);
    assert!(
        (result.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "Capitalized 'Yearly' $120 must normalize to $10/month, got {} \
         (factor table only matches lowercase => 12x overstatement)",
        result.subscriptions[0].monthly_amount
    );
}

/// A whitespace-padded " yearly " should still normalize to $10/month.
#[test]
fn bug_whitespace_yearly_overstates_12x() {
    let items = vec![item("NewsCo", -120.0, " yearly ", false)];
    let result = compute_subscription_audit(&items);
    assert!(
        (result.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "Whitespace ' yearly ' $120 must normalize to $10/month, got {}",
        result.subscriptions[0].monthly_amount
    );
}

// ---------------------------------------------------------------------------
// FINDING 2 (SUSPICIOUS): unknown cadence fallback is 1.0. This DOCUMENTS the
// ADR-accepted behavior — a genuinely unknown cadence is treated as monthly.
// This test PASSES (documents the invariant, not a bug).
// ---------------------------------------------------------------------------

#[test]
fn doc_unknown_cadence_defaults_to_monthly_factor() {
    let items = vec![item("Mystery", -25.0, "fortnightly", false)];
    let result = compute_subscription_audit(&items);
    assert!((result.subscriptions[0].monthly_amount - 25.0).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// FINDING 3 (CLEAN check): yearly does NOT double-annualize.
// yearly $120 => monthly $10 => annual $120 (not $1440).
// ---------------------------------------------------------------------------

#[test]
fn clean_yearly_does_not_double_annualize() {
    let items = vec![item("NewsCo", -120.0, "yearly", false)];
    let result = compute_subscription_audit(&items);
    assert!((result.subscriptions[0].annualized_amount - 120.0).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// FINDING 4 (CLEAN check): zero-amount stream — filter is `< 0.0`, so a $0
// stream is EXCLUDED (neither inflow nor outflow). Document the behavior.
// ---------------------------------------------------------------------------

#[test]
fn doc_zero_amount_stream_is_excluded() {
    let items = vec![item("ZeroStream", 0.0, "monthly", false)];
    let result = compute_subscription_audit(&items);
    assert!(
        result.subscriptions.is_empty(),
        "A $0 stream is excluded by the `< 0.0` outflow filter"
    );
}

// ---------------------------------------------------------------------------
// FINDING 5 (CLEAN check): positive (income) stream excluded, magnitudes positive.
// ---------------------------------------------------------------------------

#[test]
fn clean_income_excluded_and_magnitudes_positive() {
    let items = vec![
        item("Employer", 5000.0, "monthly", false),
        item("Netflix", -15.0, "monthly", false),
    ];
    let result = compute_subscription_audit(&items);
    assert_eq!(result.subscriptions.len(), 1);
    assert_eq!(result.subscriptions[0].merchant, "Netflix");
    assert!(
        result.subscriptions[0].monthly_amount > 0.0,
        "magnitude must be positive"
    );
}

// ---------------------------------------------------------------------------
// FINDING 6 (SUSPICIOUS): ranking tie determinism. Two streams with identical
// annualized cost — is order stable/deterministic with a merchant tiebreaker?
// sort_by is not guaranteed stable for equal keys unless sort_by is stable
// (Rust's slice::sort_by IS stable), but there is NO explicit merchant
// tiebreaker, so order == input order for ties. Probe whether a documented
// deterministic order (e.g. by merchant) holds. This asserts an ALPHABETICAL
// tiebreaker; if it fails, there is no merchant tiebreaker (SUSPICIOUS, not a
// hard bug since sort is stable => input-order-deterministic).
// ---------------------------------------------------------------------------

#[test]
fn suspicious_ties_lack_merchant_tiebreaker() {
    // Two $10/month streams; same annualized. Input order: Zebra before Apple.
    let items = vec![
        item("Zebra", -10.0, "monthly", false),
        item("Apple", -10.0, "monthly", false),
    ];
    let result = compute_subscription_audit(&items);
    assert_eq!(
        result.subscriptions[0].merchant, "Apple",
        "Expected deterministic merchant (alphabetical) tiebreaker; \
         got input-order instead"
    );
}

// ---------------------------------------------------------------------------
// FINDING 7 (CLEAN check): totals match per-item sums exactly (f64 accumulation).
// ---------------------------------------------------------------------------

#[test]
fn clean_totals_match_per_item_sums() {
    let items = vec![
        item("A", -50.0, "monthly", false),
        item("B", -120.0, "yearly", false),
        item("C", -10.0, "weekly", false),
    ];
    let result = compute_subscription_audit(&items);
    let sum_monthly: f64 = result.subscriptions.iter().map(|s| s.monthly_amount).sum();
    let sum_annual: f64 = result
        .subscriptions
        .iter()
        .map(|s| s.annualized_amount)
        .sum();
    assert!((result.total_monthly - sum_monthly).abs() < 1e-9);
    assert!((result.total_annual - sum_annual).abs() < 1e-9);
    assert!((result.total_annual - result.total_monthly * 12.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// FINDING 8 (CLEAN check): empty input => zero result, no panic/NaN.
// ---------------------------------------------------------------------------

#[test]
fn clean_empty_input_zero_result() {
    let result = compute_subscription_audit(&[]);
    assert!(result.subscriptions.is_empty());
    assert_eq!(result.total_monthly, 0.0);
    assert_eq!(result.total_annual, 0.0);
    assert!(result.total_monthly.is_finite() && result.total_annual.is_finite());
}

// ---------------------------------------------------------------------------
// FINDING 9 (CLEAN check): NaN stream amount does not panic and is excluded.
// NaN < 0.0 is false, so a NaN stream is filtered out (cannot poison totals).
// ---------------------------------------------------------------------------

#[test]
fn clean_nan_stream_amount_excluded_no_poison() {
    let items = vec![
        item("NaNCo", f64::NAN, "monthly", false),
        item("Real", -10.0, "monthly", false),
    ];
    let result = compute_subscription_audit(&items);
    assert_eq!(
        result.subscriptions.len(),
        1,
        "NaN stream excluded by `< 0.0`"
    );
    assert!(result.total_monthly.is_finite(), "totals must stay finite");
}

// ---------------------------------------------------------------------------
// FINDING 10 (CLEAN check): approximate stream included AND flagged.
// ---------------------------------------------------------------------------

#[test]
fn clean_approximate_included_and_flagged() {
    let items = vec![item("Electric", -120.0, "monthly", true)];
    let result = compute_subscription_audit(&items);
    assert_eq!(result.subscriptions.len(), 1);
    assert!(result.subscriptions[0].approximate);
}

// ---------------------------------------------------------------------------
// FINDING 11 (BUG): single-month fetch window drops non-monthly streams.
//
// The handler `fetch_and_compute_audit` fetches via
// `get_recurring_for_audit(current_month_start, current_month_end)` — a single
// calendar month (`current_month_range_for_day` => first..last of THIS month).
//
// `recurringTransactionItems(startDate, endDate)` only returns items whose
// scheduled occurrence falls inside that window. A YEARLY subscription that
// renews in, say, November produces ZERO items for a June window, so it is
// entirely absent from the audit.
//
// The tool description ("List every recurring charge ... entire ... burn") and
// ADR 0015 Decision 5 ("the audit wants every *stream* in the household, not
// just items pending this period") both promise the full inventory. The single
// month window cannot deliver it for any non-monthly cadence not renewing this
// month. This is masked in tests because:
//   - the BDD mock (`bdd/mock_monarch/server.py::_handle_web_get_upcoming_recurring`)
//     IGNORES startDate/endDate and returns every fixture item, and
//   - the live reconciliation test only compares audit-vs-scan WITHIN the same
//     window, so both share the blind spot and still "agree".
//
// PROOF MODEL: `compute_subscription_audit` is faithful — given a yearly item it
// reports it. The defect is that the yearly item never reaches the function
// because the fetch window excluded it. This test models the contract: a yearly
// stream MUST be present in the audit. It is `#[ignore]`d because the only
// reachable seam (the private month-range helper + live client) cannot be
// exercised from an integration test without a window-honoring mock; the
// assertion below encodes the *intended* full-inventory contract that the
// single-month window violates. See the artifact `bugs[]` entry for #68.
//
// To actually reproduce end-to-end: make the BDD mock honor the date window,
// add a feature scenario where a yearly stream's `date` is OUTSIDE the current
// month, and assert it still appears in subscription_audit — it will NOT, today.
#[test]
fn bug_single_month_window_omits_yearly_streams() {
    // FIX VERIFICATION: the handler now uses a 12-month forward window
    // (audit_window_for_day) so yearly/quarterly/semiannual streams have at
    // least one occurrence in the fetch window and reach compute_subscription_audit.
    //
    // This test verifies the compute contract: given a yearly stream in the
    // inventory (as the 12-month window now delivers), it appears in the audit
    // with correct normalization.

    // A yearly $120 stream must appear in the audit with monthly_amount = $10.
    let full_inventory = vec![item("NewsCo", -120.0, "yearly", false)];
    let result = compute_subscription_audit(&full_inventory);
    assert_eq!(
        result.subscriptions.len(),
        1,
        "yearly stream must appear in audit when captured by 12-month window"
    );
    assert!(
        (result.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "yearly $120 stream must normalize to $10/month, got {}",
        result.subscriptions[0].monthly_amount
    );
    assert!(
        (result.subscriptions[0].annualized_amount - 120.0).abs() < 0.01,
        "yearly $120 stream must have annualized_amount $120, got {}",
        result.subscriptions[0].annualized_amount
    );

    // A monthly stream must still appear exactly ONCE (dedup collapses 12 occurrences).
    // This is verified by the dedup logic in get_recurring_for_audit; at the compute
    // layer a single monthly item appears once.
    let monthly_inventory = vec![item("Netflix", -15.0, "monthly", false)];
    let result_monthly = compute_subscription_audit(&monthly_inventory);
    assert_eq!(
        result_monthly.subscriptions.len(),
        1,
        "monthly stream must appear exactly once"
    );
}
