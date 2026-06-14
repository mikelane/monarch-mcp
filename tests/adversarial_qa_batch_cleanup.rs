//! Adversarial QA — Gate 3 penetration test for branch `batch-cleanup-post-review`.
//!
//! This batch is four "behavior-identical" cleanup chores (#52, #49, #43, #30).
//! The single highest-risk item is #43(b): `actual_paydown_from_snapshots` was
//! refactored from a two-pass (filter+sum, then any(), then filter+sum) form into
//! a single `fold`. This file proves the refactor is behavior-preserving for the
//! *reachable* input space by re-implementing the OLD two-pass logic as a
//! reference oracle and asserting the new public function agrees with it across a
//! battery of input shapes (empty, only-prior, only-current, no-prior, multiple,
//! arbitrary order, zero/positive/negative balances, non-debt types interleaved).
//!
//! Float results are compared bit-for-bit (`==`), not with an epsilon: both the
//! old and new code accumulate in the same iteration order, so an equivalent
//! refactor must produce identical f64 values. An epsilon comparison would mask a
//! reordering regression — exactly the kind of subtle bug this gate hunts for.
//!
//! NOTE on the one divergence that exists: if `prior_month == current_month`, the
//! old code (two independent filters) counts a matching snapshot toward BOTH sums
//! (net 0), while the new `else if` counts it only toward prior (net prior_owed).
//! That input is UNREACHABLE in production: the sole caller derives prior_month
//! from `prior_month_range_for_day` and current_month from
//! `current_month_range_for_day`, which are always distinct adjacent months. The
//! reference oracle below therefore models the production contract (distinct
//! months) and all cases use distinct months. This test PASSES — the cleanup is
//! genuinely behavior-preserving on every reachable input.

use monarch_mcp::net_worth_trend::AccountTypeSnapshot;
use monarch_mcp::progress_vs_goals::actual_paydown_from_snapshots;

/// Debt types, mirrored from the private `DEBT_TYPES` in `progress_vs_goals.rs`.
/// This is the behavioral contract under test; if the source list changes, this
/// reference must change with it (and that itself would be a behavior change).
const DEBT_TYPES: &[&str] = &["credit", "loan"];

fn snap(account_type: &str, month: &str, balance: f64) -> AccountTypeSnapshot {
    AccountTypeSnapshot {
        account_type: account_type.to_string(),
        month: month.to_string(),
        balance,
    }
}

/// Reference oracle: the OLD two-pass implementation, verbatim in structure.
/// (prior_owed sum) -> (has_prior any) -> early None -> (current_owed sum).
fn reference_two_pass(
    snapshots: &[AccountTypeSnapshot],
    prior_month: &str,
    current_month: &str,
) -> Option<f64> {
    let prior_owed: f64 = snapshots
        .iter()
        .filter(|s| s.month == prior_month && DEBT_TYPES.contains(&s.account_type.as_str()))
        .map(|s| (-s.balance).max(0.0))
        .sum();

    let has_prior = snapshots
        .iter()
        .any(|s| s.month == prior_month && DEBT_TYPES.contains(&s.account_type.as_str()));

    if !has_prior {
        return None;
    }

    let current_owed: f64 = snapshots
        .iter()
        .filter(|s| s.month == current_month && DEBT_TYPES.contains(&s.account_type.as_str()))
        .map(|s| (-s.balance).max(0.0))
        .sum();

    Some(prior_owed - current_owed)
}

/// Assert the new public fold agrees bit-for-bit with the two-pass oracle.
fn assert_equiv(snaps: &[AccountTypeSnapshot], prior: &str, current: &str, label: &str) {
    let new = actual_paydown_from_snapshots(snaps, prior, current);
    let old = reference_two_pass(snaps, prior, current);
    assert_eq!(
        new, old,
        "fold diverged from two-pass oracle for case: {label}"
    );
}

#[test]
fn fold_matches_two_pass_for_empty_slice() {
    assert_equiv(&[], "2026-04", "2026-05", "empty slice -> None");
    assert_eq!(
        actual_paydown_from_snapshots(&[], "2026-04", "2026-05"),
        None
    );
}

#[test]
fn fold_matches_two_pass_when_only_current_month_present() {
    // No prior snapshot -> first data point -> None.
    let snaps = [snap("credit", "2026-05", -5000.0)];
    assert_equiv(&snaps, "2026-04", "2026-05", "only-current -> None");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        None
    );
}

#[test]
fn fold_matches_two_pass_when_only_prior_month_present() {
    // Prior present, current absent -> current_owed == 0 -> Some(prior_owed).
    let snaps = [snap("credit", "2026-04", -7000.0)];
    assert_equiv(&snaps, "2026-04", "2026-05", "only-prior");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(7000.0)
    );
}

#[test]
fn fold_matches_two_pass_for_standard_paydown() {
    let snaps = [
        snap("credit", "2026-04", -7000.0),
        snap("credit", "2026-05", -6000.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "standard paydown of 1000");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(1000.0)
    );
}

#[test]
fn fold_matches_two_pass_when_debt_grew() {
    let snaps = [
        snap("loan", "2026-04", -3000.0),
        snap("loan", "2026-05", -3500.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "debt grew -> negative");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(-500.0)
    );
}

#[test]
fn fold_matches_two_pass_with_multiple_debt_types_per_month() {
    let snaps = [
        snap("credit", "2026-04", -2000.0),
        snap("loan", "2026-04", -8000.0),
        snap("credit", "2026-05", -1500.0),
        snap("loan", "2026-05", -8000.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "multi-type accumulation");
    // prior 10000 - current 9500 = 500
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(500.0)
    );
}

#[test]
fn fold_matches_two_pass_with_arbitrary_ordering() {
    // Same data as the multi-type case but shuffled — fold must be order-invariant
    // in result, and accumulate in the same order the oracle does.
    let snaps = [
        snap("loan", "2026-05", -8000.0),
        snap("credit", "2026-04", -2000.0),
        snap("loan", "2026-04", -8000.0),
        snap("credit", "2026-05", -1500.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "shuffled ordering");
}

#[test]
fn fold_matches_two_pass_ignores_non_debt_types() {
    // depository / brokerage / retirement must be skipped by both forms.
    let snaps = [
        snap("depository", "2026-04", -50000.0),
        snap("brokerage", "2026-05", -120000.0),
        snap("credit", "2026-04", -4000.0),
        snap("credit", "2026-05", -3000.0),
        snap("retirement", "2026-04", -99999.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "non-debt types ignored");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(1000.0)
    );
}

#[test]
fn fold_matches_two_pass_with_positive_overpaid_balance_clamped() {
    // Overpaid credit card (positive balance) -> magnitude clamps to 0.0.
    let snaps = [
        snap("credit", "2026-04", 250.0), // overpaid -> owed 0
        snap("credit", "2026-05", -1000.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "overpaid prior clamps to 0");
    // prior_owed 0, current_owed 1000 -> -1000, but has_prior is TRUE (a prior
    // debt-type snapshot exists even though its magnitude clamps to 0).
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(-1000.0)
    );
}

#[test]
fn fold_matches_two_pass_with_zero_balances() {
    let snaps = [
        snap("credit", "2026-04", 0.0),
        snap("credit", "2026-05", 0.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "zero balances");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(0.0)
    );
}

#[test]
fn fold_matches_two_pass_with_unrelated_months_present() {
    // Snapshots for months that are neither prior nor current are inert.
    let snaps = [
        snap("credit", "2026-01", -9999.0),
        snap("credit", "2026-04", -5000.0),
        snap("credit", "2026-05", -4000.0),
        snap("credit", "2026-12", -1.0),
    ];
    assert_equiv(&snaps, "2026-04", "2026-05", "unrelated months inert");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05"),
        Some(1000.0)
    );
}

#[test]
fn fold_matches_two_pass_across_year_boundary_months() {
    // Year-boundary derivation (#43a): prior "2025-12", current "2026-01".
    let snaps = [
        snap("loan", "2025-12", -20000.0),
        snap("loan", "2026-01", -19500.0),
    ];
    assert_equiv(&snaps, "2025-12", "2026-01", "year-boundary months");
    assert_eq!(
        actual_paydown_from_snapshots(&snaps, "2025-12", "2026-01"),
        Some(500.0)
    );
}

/// Exhaustive small-space sweep: every combination of {absent, debt@prior,
/// debt@current, non-debt, overpaid} across a few snapshots, asserting the fold
/// equals the oracle on each. This is the brute-force equivalence proof.
#[test]
fn fold_matches_two_pass_exhaustive_small_space() {
    let candidates = [
        None,
        Some(snap("credit", "2026-04", -1000.0)),
        Some(snap("loan", "2026-04", -2000.0)),
        Some(snap("credit", "2026-05", -750.0)),
        Some(snap("loan", "2026-05", -3000.0)),
        Some(snap("depository", "2026-04", -50000.0)),
        Some(snap("credit", "2026-04", 500.0)), // overpaid prior
        Some(snap("credit", "2026-05", 250.0)), // overpaid current
    ];

    // Build all 4-length sequences over the candidate set (8^4 = 4096 cases).
    for a in 0..candidates.len() {
        for b in 0..candidates.len() {
            for c in 0..candidates.len() {
                for d in 0..candidates.len() {
                    let mut snaps: Vec<AccountTypeSnapshot> = Vec::new();
                    for idx in [a, b, c, d] {
                        if let Some(s) = &candidates[idx] {
                            snaps.push(s.clone());
                        }
                    }
                    let new = actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05");
                    let old = reference_two_pass(&snaps, "2026-04", "2026-05");
                    assert_eq!(
                        new, old,
                        "fold diverged from oracle for indices [{a},{b},{c},{d}]"
                    );
                }
            }
        }
    }
}
