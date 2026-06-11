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
// FINDING 12 (cadence coverage): the semiannual (1/6) arm was previously
// untested. $120/semiannual must normalize to $20/month → $240/year.
// A mutation to the 1.0/6.0 factor would otherwise survive.
// ---------------------------------------------------------------------------

#[test]
fn semiannual_cadence_normalizes_to_one_sixth() {
    let items = vec![item("Pass", -120.0, "semiannual", false)];
    let result = compute_subscription_audit(&items);
    assert!(
        (result.subscriptions[0].monthly_amount - 20.0).abs() < 0.01,
        "semiannual $120 must be $20/month, got {}",
        result.subscriptions[0].monthly_amount
    );
    assert!(
        (result.subscriptions[0].annualized_amount - 240.0).abs() < 0.01,
        "semiannual $120 must annualize to $240, got {}",
        result.subscriptions[0].annualized_amount
    );
}

// ---------------------------------------------------------------------------
// FINDING 13 (alias coverage): each documented cadence alias must resolve to
// its canonical factor. A mutation removing an alias from a match arm would
// otherwise survive because no test exercises the alias spelling.
// ---------------------------------------------------------------------------

#[test]
fn annually_alias_matches_yearly_factor() {
    let result = compute_subscription_audit(&[item("NewsCo", -120.0, "annually", false)]);
    assert!(
        (result.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "annually must match yearly (1/12) → $10/month, got {}",
        result.subscriptions[0].monthly_amount
    );
}

#[test]
fn every_two_weeks_alias_matches_biweekly_factor() {
    let result = compute_subscription_audit(&[item("Svc", -100.0, "every_two_weeks", false)]);
    let expected = 100.0 * 26.0 / 12.0;
    assert!(
        (result.subscriptions[0].monthly_amount - expected).abs() < 0.01,
        "every_two_weeks must match biweekly (26/12), got {}",
        result.subscriptions[0].monthly_amount
    );
}

#[test]
fn every_three_months_alias_matches_quarterly_factor() {
    let result = compute_subscription_audit(&[item("Mag", -90.0, "every_three_months", false)]);
    assert!(
        (result.subscriptions[0].monthly_amount - 30.0).abs() < 0.01,
        "every_three_months must match quarterly (1/3) → $30/month, got {}",
        result.subscriptions[0].monthly_amount
    );
}

#[test]
fn twice_a_year_alias_matches_semiannual_factor() {
    let result = compute_subscription_audit(&[item("Pass", -120.0, "twice_a_year", false)]);
    assert!(
        (result.subscriptions[0].monthly_amount - 20.0).abs() < 0.01,
        "twice_a_year must match semiannual (1/6) → $20/month, got {}",
        result.subscriptions[0].monthly_amount
    );
}

// ---------------------------------------------------------------------------
// FINDING 14 (tiebreaker determinism): the secondary cadence tiebreaker
// (.then_with(|| a.cadence.cmp(&b.cadence))) was unpinned. When merchant AND
// annualized cost are equal, cadence ascending decides order. A mutation
// dropping that arm would survive without this test.
// ---------------------------------------------------------------------------

#[test]
fn equal_merchant_and_cost_breaks_tie_by_cadence_ascending() {
    // Both annualize to $120/year for the SAME merchant: "monthly" $10 vs "yearly" $120.
    // Input order puts "yearly" first to prove the sort (not input order) decides.
    let items = vec![
        item("SameCo", -120.0, "yearly", false),
        item("SameCo", -10.0, "monthly", false),
    ];
    let result = compute_subscription_audit(&items);
    assert!(
        (result.subscriptions[0].annualized_amount - result.subscriptions[1].annualized_amount)
            .abs()
            < 0.01,
        "precondition: both streams annualize to the same cost"
    );
    assert_eq!(
        result.subscriptions[0].cadence, "monthly",
        "tie on merchant+cost must break by cadence ascending (monthly < yearly)"
    );
    assert_eq!(result.subscriptions[1].cadence, "yearly");
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

// ===========================================================================
// RUN #2 RE-ATTACK PROBES (HEAD 60c0aa0) — the window, cadence, and sort all
// changed; re-attack the new surface. All of these PASS at the fixed SHA.
// ===========================================================================

// --- Cadence casing: every casing variant must normalize, not fall to 1.0 ---

#[test]
fn reattack_uppercase_yearly_normalizes() {
    let r = compute_subscription_audit(&[item("NewsCo", -120.0, "YEARLY", false)]);
    assert!(
        (r.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "'YEARLY' must normalize to $10/mo, got {}",
        r.subscriptions[0].monthly_amount
    );
}

#[test]
fn reattack_mixedcase_monthly_normalizes() {
    let r = compute_subscription_audit(&[item("Gym", -40.0, "Monthly", false)]);
    assert!(
        (r.subscriptions[0].monthly_amount - 40.0).abs() < 0.01,
        "'Monthly' must stay $40/mo, got {}",
        r.subscriptions[0].monthly_amount
    );
}

#[test]
fn reattack_uppercase_weekly_normalizes() {
    let r = compute_subscription_audit(&[item("Wk", -10.0, "WEEKLY", false)]);
    let expected = 10.0 * 52.0 / 12.0;
    assert!(
        (r.subscriptions[0].monthly_amount - expected).abs() < 0.01,
        "'WEEKLY' must normalize to {expected:.4}/mo, got {}",
        r.subscriptions[0].monthly_amount
    );
}

#[test]
fn reattack_tabs_and_newlines_trimmed() {
    // Embedded surrounding whitespace incl. tab/newline must be trimmed.
    let r = compute_subscription_audit(&[item("NewsCo", -120.0, "\t yearly \n", false)]);
    assert!(
        (r.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
        "tab/newline-padded 'yearly' must normalize to $10/mo, got {}",
        r.subscriptions[0].monthly_amount
    );
}

#[test]
fn reattack_genuinely_unknown_cadence_still_falls_back_to_monthly() {
    // The fallback must remain for truly-unknown strings (not over-eager matching).
    let r = compute_subscription_audit(&[item("Mystery", -25.0, "every_blue_moon", false)]);
    assert!(
        (r.subscriptions[0].monthly_amount - 25.0).abs() < 0.01,
        "unknown cadence must fall back to monthly (1.0), got {}",
        r.subscriptions[0].monthly_amount
    );
}

#[test]
fn reattack_weird_strings_do_not_panic() {
    // Empty, unicode, very long, numeric — none may panic; all → monthly fallback.
    let weird = ["", "  ", "💸", "123", &"x".repeat(10_000), "MoNtHlY-ish"];
    for w in weird {
        let r = compute_subscription_audit(&[item("M", -10.0, w, false)]);
        assert!(
            r.total_monthly.is_finite(),
            "weird cadence {w:?} must not produce non-finite total"
        );
    }
}

// --- Sort: descending-by-annualized unchanged for non-tied cases ---

#[test]
fn reattack_descending_order_unchanged_for_distinct_costs() {
    // Input intentionally out of order; output must be strictly descending.
    let items = vec![
        item("Cheap", -5.0, "monthly", false),    // 60/yr
        item("Pricey", -100.0, "monthly", false), // 1200/yr
        item("Mid", -50.0, "monthly", false),     // 600/yr
    ];
    let r = compute_subscription_audit(&items);
    let order: Vec<&str> = r
        .subscriptions
        .iter()
        .map(|s| s.merchant.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["Pricey", "Mid", "Cheap"],
        "must be descending by annualized"
    );
}

// --- Income exclusion survives the wider, mixed-cadence flow ---

#[test]
fn reattack_income_excluded_in_mixed_cadence_flow() {
    let items = vec![
        item("Employer", 5000.0, "monthly", false), // income → excluded
        item("Bonus", 1000.0, "yearly", false),     // positive yearly income → excluded
        item("Netflix", -15.0, "monthly", false),   // 15/mo
        item("NewsCo", -120.0, "yearly", false),    // 10/mo
        item("Pass", -120.0, "semiannual", false),  // 20/mo
    ];
    let r = compute_subscription_audit(&items);
    assert_eq!(r.subscriptions.len(), 3, "only the 3 outflows survive");
    let names: Vec<&str> = r
        .subscriptions
        .iter()
        .map(|s| s.merchant.as_str())
        .collect();
    assert!(!names.contains(&"Employer") && !names.contains(&"Bonus"));
    // total_monthly = 15 + 10 + 20 = 45; income must NOT leak in.
    assert!(
        (r.total_monthly - 45.0).abs() < 0.01,
        "income must not leak into totals; expected 45.00, got {}",
        r.total_monthly
    );
    assert!((r.total_annual - 540.0).abs() < 0.01);
}

// --- Mid-year price change (wider-window edge): documents the behavior ---
// A stream whose declared amount differs across the window arrives as TWO items
// with different stream_amount. compute keeps BOTH (distinct -> two rows). This
// is the SAFE direction (no silent loss); over-counting one logical sub as two
// rows is visible. Reachability is bounded by the domain invariant that Monarch's
// forward recurringTransactionItems reference ONE current stream definition, so
// all forward occurrences carry a single stream.amount (per-stream). Documented,
// not guarded, per circuit-breaker philosophy.
#[test]
fn reattack_two_amounts_same_merchant_kept_separate_safe_direction() {
    let items = vec![
        item("Spotify", -10.0, "monthly", false),
        item("Spotify", -12.0, "monthly", false), // post-price-increase snapshot
    ];
    let r = compute_subscription_audit(&items);
    assert_eq!(
        r.subscriptions.len(),
        2,
        "distinct amounts are kept separate (no silent loss); totals stay correct"
    );
    assert!(
        (r.total_monthly - 22.0).abs() < 0.01,
        "both amounts counted; expected 22.00, got {}",
        r.total_monthly
    );
}
