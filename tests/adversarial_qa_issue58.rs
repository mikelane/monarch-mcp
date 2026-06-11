//! Adversarial QA (Gate 3) probes for the issue-58 spending_history polish PR.
//!
//! Covers three changes shipped on branch `issue-58-spending-history-polish`:
//!   - #58 (fix): `find_outliers` adds `merchant` as a final sort tiebreaker.
//!   - #57 (refactor): rename test-only `parse_iso_to_epoch_day` -> `parse_date_for_test`.
//!   - #59 (chore): rewrite `subtract_months` from an O(n) loop to O(1) arithmetic.
//!
//! Tests WITHOUT `#[ignore]` document behavior that is CLEAN (defensive proofs
//! that a suspected attack vector is actually handled) and run in the normal
//! suite to stay green.
//!
//! Any test WITH `#[ignore]` documents a CONFIRMED defect; run them with:
//!
//! ```bash
//! cargo test --test adversarial_qa_issue58 -- --ignored
//! ```
//!
//! As of this run there are ZERO confirmed bugs, so there are no `#[ignore]`d
//! tests. The full file is part of the green suite and proves the three changes
//! preserve the documented invariants.

use monarch_mcp::client::{Category, Transaction};
use monarch_mcp::spending_history::{compute_spending_history, range_for_months_count};

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

fn expense_id(id: &str, merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
    Transaction {
        id: id.to_string(),
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
// #59 — subtract_months O(1) equivalence (HIGHEST RISK)
//
// `subtract_months` is private. We prove equivalence two ways:
//   (A) a DIFFERENTIAL property test: reimplement BOTH the old O(n) loop and
//       the new O(1) formula here and brute-force them against each other over
//       a wide grid of (year, month, n). If they ever disagree, this fails.
//   (B) a PUBLIC-SURFACE test: drive `range_for_months_count` (the only caller)
//       across the full reachable input space and assert internal consistency.
// ===========================================================================

/// Faithful copy of the OLD O(n) loop (commit d068ac5, parent of #59).
fn subtract_months_old(year: i64, month: u32, n: u32) -> (i64, u32) {
    let mut y = year;
    let mut m = month;
    for _ in 0..n {
        if m == 1 {
            y -= 1;
            m = 12;
        } else {
            m -= 1;
        }
    }
    (y, m)
}

/// Faithful copy of the NEW O(1) formula (commit 9438b40, #59).
fn subtract_months_new(year: i64, month: u32, n: u32) -> (i64, u32) {
    let total = year * 12 + (month as i64 - 1) - n as i64;
    let new_year = total.div_euclid(12);
    let new_month = (total.rem_euclid(12) + 1) as u32;
    (new_year, new_month)
}

#[test]
fn subtract_months_old_and_new_agree_over_wide_grid() {
    // Years span well beyond any reachable value (negative years included to
    // exercise the div_euclid/rem_euclid negative-total branch the loop also
    // reaches by borrowing). n spans 0..=120 (10 years), far past the clamped
    // reachable max of 23.
    for year in -5i64..=5000 {
        for month in 1u32..=12 {
            for n in 0u32..=120 {
                let old = subtract_months_old(year, month, n);
                let new = subtract_months_new(year, month, n);
                assert_eq!(
                    old, new,
                    "DISAGREE at year={year} month={month} n={n}: old={old:?} new={new:?}"
                );
            }
        }
    }
}

#[test]
fn subtract_months_new_always_yields_valid_1_based_month() {
    // The formula must NEVER produce month 0 or 13 for any input — including
    // the negative-total branch that callers cannot reach but the math must
    // still honor.
    for year in -50i64..=5000 {
        for month in 1u32..=12 {
            for n in 0u32..=400 {
                let (_, m) = subtract_months_new(year, month, n);
                assert!(
                    (1..=12).contains(&m),
                    "month out of range: year={year} month={month} n={n} -> {m}"
                );
            }
        }
    }
}

#[test]
fn subtract_months_negative_total_branch_matches_loop() {
    // Directly exercise the only branch a clamped caller can't reach:
    // total goes negative (n exceeds year*12 + month-1). Old loop borrows past
    // year 0 into negative years; new formula uses div_euclid. They must agree.
    // year=0 month=1 -> total = -1 -> div_euclid(12) = -1, rem_euclid = 11 -> (−1, 12).
    assert_eq!(subtract_months_old(0, 1, 1), subtract_months_new(0, 1, 1));
    assert_eq!(subtract_months_old(0, 1, 13), subtract_months_new(0, 1, 13));
    assert_eq!(
        subtract_months_old(1, 6, 100),
        subtract_months_new(1, 6, 100)
    );
    assert_eq!(subtract_months_new(0, 1, 1), (-1, 12));
}

// --- (B) public-surface consistency across the full REACHABLE input grid ----
//
// Reachable inputs (from resolve_history_range in tools.rs):
//   - today_day from a real current date (here we sample many epoch days)
//   - months clamped to 1..=24, so range_for_months_count sees months in 1..=24
//     and internally calls subtract_months with n=1 and n=months-1 (0..=23).

/// Local epoch-day builder (Howard Hinnant) so we can feed range_for_months_count
/// a wide set of "today" values without importing the private parser.
fn ymd_to_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let m = month as u32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[test]
fn range_for_months_count_is_internally_consistent_over_reachable_grid() {
    // Sample "today" across 30 years and every month; months across the full
    // clamped 1..=24 range. Assert the contract holds for every combination.
    for year in 2000i64..=2030 {
        for tm in 1i64..=12 {
            let today = ymd_to_epoch_day(year, tm, 15);
            for months in 1u32..=24 {
                let (start, end) = range_for_months_count(today, months);

                // start <= end (lexicographic works for zero-padded ISO).
                assert!(start <= end, "start {start} > end {end} (months={months})");

                // start begins on the 1st; both are well-formed YYYY-MM-DD.
                assert!(start.ends_with("-01"), "start not 1st: {start}");
                assert_eq!(start.len(), 10, "start malformed: {start}");
                assert_eq!(end.len(), 10, "end malformed: {end}");

                // Months are valid 1..=12 (catches any 00 / 13 from the formula).
                let start_mm: u32 = start[5..7].parse().unwrap();
                let end_mm: u32 = end[5..7].parse().unwrap();
                assert!((1..=12).contains(&start_mm), "bad start month: {start}");
                assert!((1..=12).contains(&end_mm), "bad end month: {end}");

                // The window spans exactly `months` complete calendar months.
                let start_y: i64 = start[0..4].parse().unwrap();
                let end_y: i64 = end[0..4].parse().unwrap();
                let span = (end_y * 12 + end_mm as i64) - (start_y * 12 + start_mm as i64) + 1;
                assert_eq!(
                    span, months as i64,
                    "expected {months} months, got {span} ({start}..{end})"
                );
            }
        }
    }
}

#[test]
fn range_for_months_count_january_today_borrows_into_prior_year() {
    // month==1 edge through the public surface: today = Jan 2026, 3 months back
    // must land Oct-Dec 2025 (prior month Dec 2025; start = Oct 2025).
    let today = ymd_to_epoch_day(2026, 1, 10);
    let (start, end) = range_for_months_count(today, 3);
    assert_eq!(start, "2025-10-01");
    assert_eq!(end, "2025-12-31");
}

#[test]
fn range_for_months_count_max_24_months_spans_two_full_years() {
    // months=24 (clamp max): today = Jun 2026 -> prior = May 2026 -> 24 months
    // back start = Jun 2024.
    let today = ymd_to_epoch_day(2026, 6, 15);
    let (start, end) = range_for_months_count(today, 24);
    assert_eq!(start, "2024-06-01");
    assert_eq!(end, "2026-05-31");
}

// ===========================================================================
// #58 — outlier sort determinism (tiebreaker on merchant)
// ===========================================================================

/// Build a category where the two FIRST transactions are large equal-amount,
/// equal-date outliers with distinct merchants, anchored by small peers.
fn two_equal_outliers(m1: &str, m2: &str) -> Vec<Transaction> {
    vec![
        expense(m1, -900.0, "Dining", "2026-03-15"),
        expense(m2, -900.0, "Dining", "2026-03-15"),
        expense("Tiny A", -5.0, "Dining", "2026-03-01"),
        expense("Tiny B", -5.0, "Dining", "2026-03-02"),
        expense("Tiny C", -5.0, "Dining", "2026-03-03"),
    ]
}

#[test]
fn outlier_tiebreaker_is_alphabetical_regardless_of_input_order() {
    // Feed the SAME two outliers in BOTH input orders; the tiebreaker must
    // produce identical alphabetical output ("Alpha" before "Zeta") both times.
    // This is the real determinism proof: order independence of the result.
    let forward = compute_spending_history(
        &two_equal_outliers("Alpha Merchant", "Zeta Merchant"),
        "2026-03-01",
        "2026-03-31",
    );
    let reversed = compute_spending_history(
        &two_equal_outliers("Zeta Merchant", "Alpha Merchant"),
        "2026-03-01",
        "2026-03-31",
    );
    let fo = &forward.months[0].outliers;
    let ro = &reversed.months[0].outliers;
    assert_eq!(fo.len(), 2);
    assert_eq!(ro.len(), 2);
    assert_eq!(fo[0].merchant, "Alpha Merchant");
    assert_eq!(fo[1].merchant, "Zeta Merchant");
    // Order-independence: reversed input yields the SAME ordering.
    assert_eq!(ro[0].merchant, "Alpha Merchant");
    assert_eq!(ro[1].merchant, "Zeta Merchant");
}

#[test]
fn outlier_full_tie_including_merchant_is_stable_by_input_order() {
    // Residual case: SAME category/date/amount/merchant, only txn id differs.
    // The sort key cannot distinguish these. Rust's sort_by is STABLE, so they
    // retain input order. This proves there is NO nondeterminism for fixed
    // input — the result is fully determined (id-1 stays before id-2).
    let txns = vec![
        expense_id("id-1", "Same Merchant", -900.0, "Dining", "2026-03-15"),
        expense_id("id-2", "Same Merchant", -900.0, "Dining", "2026-03-15"),
        expense("Tiny A", -5.0, "Dining", "2026-03-01"),
        expense("Tiny B", -5.0, "Dining", "2026-03-02"),
        expense("Tiny C", -5.0, "Dining", "2026-03-03"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    let outliers = &history.months[0].outliers;
    assert_eq!(outliers.len(), 2);
    // Both are "Same Merchant"; the only observable field guaranteeing order is
    // their input position. Stable sort keeps id-1's row first. We assert the
    // merchant is consistent and the count is right (the SpendingOutlier type
    // does not expose id, so determinism here means: same input -> same vec,
    // which we verify by running twice and comparing).
    let again = compute_spending_history(
        &[
            expense_id("id-1", "Same Merchant", -900.0, "Dining", "2026-03-15"),
            expense_id("id-2", "Same Merchant", -900.0, "Dining", "2026-03-15"),
            expense("Tiny A", -5.0, "Dining", "2026-03-01"),
            expense("Tiny B", -5.0, "Dining", "2026-03-02"),
            expense("Tiny C", -5.0, "Dining", "2026-03-03"),
        ],
        "2026-03-01",
        "2026-03-31",
    );
    assert_eq!(outliers, &again.months[0].outliers);
}

#[test]
fn outlier_tiebreaker_does_not_reorder_the_non_tied_common_case() {
    // Regression guard: when category/date/amount already differ, the merchant
    // tiebreaker must NOT change the established (category, date, amount_desc)
    // ordering. Two different categories + different amounts: order is fixed by
    // the primary keys, independent of merchant alphabetics.
    let txns = vec![
        // "Zebra" merchant but category "Aaa" with a big outlier
        expense("Zebra", -600.0, "Aaa", "2026-03-10"),
        expense("z1", -10.0, "Aaa", "2026-03-01"),
        expense("z2", -10.0, "Aaa", "2026-03-02"),
        // "Apple" merchant in category "Bbb"
        expense("Apple", -600.0, "Bbb", "2026-03-10"),
        expense("b1", -10.0, "Bbb", "2026-03-01"),
        expense("b2", -10.0, "Bbb", "2026-03-02"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    let outliers = &history.months[0].outliers;
    assert_eq!(outliers.len(), 2);
    // Category is the PRIMARY key: "Aaa" sorts before "Bbb" even though the
    // "Aaa" outlier's merchant ("Zebra") is alphabetically AFTER "Apple".
    assert_eq!(outliers[0].category, "Aaa");
    assert_eq!(outliers[0].merchant, "Zebra");
    assert_eq!(outliers[1].category, "Bbb");
    assert_eq!(outliers[1].merchant, "Apple");
}

#[test]
fn outlier_ordering_does_not_change_totals_or_split() {
    // The tiebreaker only reorders the outliers vec; totals and the
    // fixed/discretionary split must be identical regardless of input order.
    let a = compute_spending_history(
        &two_equal_outliers("Alpha Merchant", "Zeta Merchant"),
        "2026-03-01",
        "2026-03-31",
    );
    let b = compute_spending_history(
        &two_equal_outliers("Zeta Merchant", "Alpha Merchant"),
        "2026-03-01",
        "2026-03-31",
    );
    let ma = &a.months[0];
    let mb = &b.months[0];
    assert_eq!(ma.total_true_spending, mb.total_true_spending);
    assert_eq!(ma.split, mb.split);
    // And fixed + discretionary == total (invariant preserved).
    assert!((ma.split.fixed + ma.split.discretionary - ma.total_true_spending).abs() < 1e-9);
}

// ===========================================================================
// Unchanged spending_history invariants (regression after the edits)
// ===========================================================================

#[test]
fn compact_output_carries_no_raw_transaction_objects() {
    // The serialized payload must expose aggregates only (month, totals,
    // by_category, split, outliers) — never the raw Transaction list. A
    // regression that leaked transactions would balloon the payload.
    let txns = vec![
        expense("Lender", -2500.0, "Mortgage", "2026-03-01"),
        expense("Cafe", -12.0, "Dining", "2026-03-05"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    let json = serde_json::to_string(&history).unwrap();
    // No transaction-only fields should appear in the output.
    assert!(!json.contains("needs_review"));
    assert!(!json.contains("\"notes\""));
    assert!(!json.contains("\"tags\""));
    assert!(!json.contains("\"id\""));
}

#[test]
fn months_zero_clamps_to_one_and_does_not_panic() {
    // months=0 must clamp to 1 (one prior complete month), never underflow.
    let today = ymd_to_epoch_day(2026, 5, 15);
    let (start, end) = range_for_months_count(today, 0);
    assert_eq!(start, "2026-04-01");
    assert_eq!(end, "2026-04-30");
}

#[test]
fn non_ascii_date_does_not_panic_and_is_dropped() {
    // A transaction with a multi-byte char straddling the 7-byte month window
    // must be silently dropped, never panic on a char-boundary slice.
    let txns = vec![
        expense("Bad", -100.0, "Dining", "2026-0é-15"),
        expense("Good", -50.0, "Dining", "2026-03-15"),
    ];
    let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
    // Only the good transaction counts; no panic.
    let total: f64 = history.months.iter().map(|m| m.total_true_spending).sum();
    assert_eq!(total, 50.0);
}
