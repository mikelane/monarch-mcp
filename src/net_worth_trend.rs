//! Net worth trend — month-by-month net worth with per-type deltas and biggest mover.
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_trend`].
//!
//! ## Monarch sign convention
//! Asset types (depository, brokerage) carry positive balances.
//! Liability types (credit, loan) carry **negative** balances.
//! A liability *reduction* (e.g. credit balance going from −10 000 to −7 000)
//! is a **positive** contribution to net worth: net worth went up by 3 000.
//! `type_delta` already captures this correctly because we compute
//! `latest_balance − earliest_balance`; for liabilities that go from −10 000
//! to −7 000 the result is `(−7 000) − (−10 000) = +3 000`.

use std::collections::HashMap;

use serde::Serialize;

// ---------------------------------------------------------------------------
// Public input type — consumed by tools.rs from the client response
// ---------------------------------------------------------------------------

/// One row from `GetSnapshotsByAccountType → snapshotsByAccountType[]`.
#[derive(Debug, Clone)]
pub struct AccountTypeSnapshot {
    /// Lowercase account type string from Monarch, e.g. `"depository"`, `"credit"`.
    pub account_type: String,
    /// `"YYYY-MM"` — end-of-month snapshot for this calendar month.
    pub month: String,
    /// Total balance across all accounts of this type. Negative for liabilities.
    pub balance: f64,
}

// ---------------------------------------------------------------------------
// Output types — must match what the BDD Then-steps assert on
// ---------------------------------------------------------------------------

/// The result payload returned as JSON text inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct TrendResult {
    /// One entry per calendar month in the requested period, sorted ascending.
    pub monthly_snapshots: Vec<MonthlyNetWorth>,
    /// Net worth in the most recent month (0.0 when no data).
    pub latest_net_worth: f64,
    /// Change from earliest to latest month (positive = growth, 0.0 for single month).
    pub net_worth_change: f64,
    /// Per-type delta from earliest to latest month.
    /// Key: account_type string. Value: `TypeSummary`.
    pub by_account_type: HashMap<String, TypeSummary>,
    /// The account type with the largest absolute balance change.
    /// `None` when there are fewer than two months of data.
    pub biggest_mover: Option<BiggestMover>,
    /// Sum of positive (asset) balances in the latest month.
    pub total_assets: f64,
    /// Absolute sum of negative (liability) balances in the latest month.
    pub total_liabilities: f64,
}

/// Net worth for one calendar month.
#[derive(Debug, Serialize, PartialEq)]
pub struct MonthlyNetWorth {
    pub month: String,
    pub net_worth: f64,
}

/// Summary for one account type across the period.
#[derive(Debug, Serialize, PartialEq)]
pub struct TypeSummary {
    /// Delta from earliest to latest month.
    /// For liabilities: negative-to-less-negative → positive value (debt paydown = growth).
    pub change: f64,
}

/// The account type that moved the most (largest absolute change).
#[derive(Debug, Serialize, PartialEq)]
pub struct BiggestMover {
    pub account_type: String,
    /// Absolute-largest signed delta (positive means net-worth-positive direction).
    pub change: f64,
}

// ---------------------------------------------------------------------------
// Aggregation — pure function, no I/O
// ---------------------------------------------------------------------------

/// Compute the trend from a flat list of monthly per-type snapshots.
///
/// The flat list is grouped by `month`, then by `account_type`.
/// Months are sorted ascending so the first entry is the earliest.
pub fn compute_trend(snapshots: &[AccountTypeSnapshot]) -> TrendResult {
    if snapshots.is_empty() {
        return TrendResult {
            monthly_snapshots: vec![],
            latest_net_worth: 0.0,
            net_worth_change: 0.0,
            by_account_type: HashMap::new(),
            biggest_mover: None,
            total_assets: 0.0,
            total_liabilities: 0.0,
        };
    }

    // Collect sorted unique months.
    let mut months: Vec<String> = snapshots.iter().map(|s| s.month.clone()).collect();
    months.sort();
    months.dedup();

    // Build net worth per month: sum all balances in that month.
    let monthly_snapshots: Vec<MonthlyNetWorth> = months
        .iter()
        .map(|m| {
            let net_worth: f64 = snapshots
                .iter()
                .filter(|s| &s.month == m)
                .map(|s| s.balance)
                .sum();
            MonthlyNetWorth {
                month: m.clone(),
                net_worth,
            }
        })
        .collect();

    let latest_net_worth = monthly_snapshots.last().map(|m| m.net_worth).unwrap_or(0.0);

    // Net worth change: latest − earliest (0 when only one month).
    let net_worth_change = if monthly_snapshots.len() >= 2 {
        latest_net_worth - monthly_snapshots[0].net_worth
    } else {
        0.0
    };

    // Per-type delta: latest balance − the type's own first-seen month balance.
    //
    // Semantics: use each account type's first-seen month within the window as its
    // baseline, NOT the overall window's earliest month. This prevents a fabricated
    // full-balance swing for types that only appear mid-window (e.g. a brokerage
    // account opened after the window start). Such a type's change is
    // latest_balance − first_seen_balance = 0 when it appears only in the latest
    // month, accurately reflecting "no measurable movement since it appeared".
    //
    // The overall net_worth_change (latest_total − earliest_total) is unaffected —
    // it is computed directly from monthly_snapshots, not from by_account_type.
    let latest_month = months.last().unwrap();

    let all_types: Vec<String> = {
        let mut ts: Vec<String> = snapshots.iter().map(|s| s.account_type.clone()).collect();
        ts.sort();
        ts.dedup();
        ts
    };

    // For each type, find its first-seen month (earliest month containing a row for
    // this type) and use that month's balance as the baseline.
    let first_seen_month_for = |acct_type: &str| -> &str {
        months
            .iter()
            .find(|m| {
                snapshots
                    .iter()
                    .any(|s| &s.month == *m && s.account_type == acct_type)
            })
            .map(|m| m.as_str())
            .unwrap_or(latest_month.as_str())
    };

    let balance_for = |month: &str, acct_type: &str| -> f64 {
        snapshots
            .iter()
            .filter(|s| s.month == month && s.account_type == acct_type)
            .map(|s| s.balance)
            .sum()
    };

    let by_account_type: HashMap<String, TypeSummary> = all_types
        .iter()
        .map(|t| {
            let baseline_month = first_seen_month_for(t);
            let baseline_bal = balance_for(baseline_month, t);
            let latest_bal = balance_for(latest_month, t);
            let change = latest_bal - baseline_bal;
            (t.clone(), TypeSummary { change })
        })
        .collect();

    // Biggest mover: account type with the largest absolute change.
    // Only meaningful when there are 2+ months.
    // Tie-break by account_type ascending so equal-magnitude moves always
    // produce the same winner regardless of HashMap iteration order.
    let biggest_mover = if months.len() >= 2 {
        by_account_type
            .iter()
            .max_by(|a, b| {
                a.1.change
                    .abs()
                    .partial_cmp(&b.1.change.abs())
                    .unwrap()
                    .then_with(|| b.0.cmp(a.0))
            })
            .map(|(t, s)| BiggestMover {
                account_type: t.clone(),
                change: s.change,
            })
    } else {
        None
    };

    // Asset / liability split from the latest month.
    let latest_rows: Vec<&AccountTypeSnapshot> = snapshots
        .iter()
        .filter(|s| &s.month == latest_month)
        .collect();

    let total_assets: f64 = latest_rows
        .iter()
        .filter(|s| s.balance > 0.0)
        .map(|s| s.balance)
        .sum();
    let total_liabilities: f64 = latest_rows
        .iter()
        .filter(|s| s.balance < 0.0)
        .map(|s| s.balance.abs())
        .sum();

    TrendResult {
        monthly_snapshots,
        latest_net_worth,
        net_worth_change,
        by_account_type,
        biggest_mover,
        total_assets,
        total_liabilities,
    }
}

// ---------------------------------------------------------------------------
// Tests — RED first, then GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(month: &str, account_type: &str, balance: f64) -> AccountTypeSnapshot {
        AccountTypeSnapshot {
            month: month.to_string(),
            account_type: account_type.to_string(),
            balance,
        }
    }

    // 9a RED: three months of data → three monthly data points
    #[test]
    fn three_months_produce_three_data_points() {
        let snapshots = vec![
            snap("2026-03", "depository", 10_000.0),
            snap("2026-03", "brokerage", 50_000.0),
            snap("2026-03", "credit", -5_000.0),
            snap("2026-04", "depository", 11_000.0),
            snap("2026-04", "brokerage", 52_000.0),
            snap("2026-04", "credit", -4_800.0),
            snap("2026-05", "depository", 12_000.0),
            snap("2026-05", "brokerage", 54_000.0),
            snap("2026-05", "credit", -4_600.0),
        ];
        let result = compute_trend(&snapshots);
        assert_eq!(result.monthly_snapshots.len(), 3);
    }

    // 9b GREEN extension: latest net worth is sum of latest month balances
    #[test]
    fn latest_net_worth_is_sum_of_latest_month() {
        let snapshots = vec![
            snap("2026-03", "depository", 10_000.0),
            snap("2026-03", "brokerage", 50_000.0),
            snap("2026-03", "credit", -5_000.0),
            snap("2026-04", "depository", 11_000.0),
            snap("2026-04", "brokerage", 52_000.0),
            snap("2026-04", "credit", -4_800.0),
            snap("2026-05", "depository", 12_000.0),
            snap("2026-05", "brokerage", 54_000.0),
            snap("2026-05", "credit", -4_600.0),
        ];
        let result = compute_trend(&snapshots);
        // 12000 + 54000 + (-4600) = 61400
        assert!((result.latest_net_worth - 61_400.0).abs() < 0.01);
    }

    // 9b GREEN extension: net worth change = latest − earliest
    #[test]
    fn net_worth_change_is_latest_minus_earliest() {
        let snapshots = vec![
            snap("2026-03", "depository", 10_000.0),
            snap("2026-03", "brokerage", 50_000.0),
            snap("2026-03", "credit", -5_000.0),
            snap("2026-04", "depository", 11_000.0),
            snap("2026-04", "brokerage", 52_000.0),
            snap("2026-04", "credit", -4_800.0),
            snap("2026-05", "depository", 12_000.0),
            snap("2026-05", "brokerage", 54_000.0),
            snap("2026-05", "credit", -4_600.0),
        ];
        let result = compute_trend(&snapshots);
        // earliest NW: 10000 + 50000 + (-5000) = 55000
        // latest NW:   12000 + 54000 + (-4600) = 61400
        // change = 61400 - 55000 = 6400
        assert!((result.net_worth_change - 6_400.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: single month → one data point, zero change
    #[test]
    fn single_month_produces_one_data_point_and_zero_change() {
        let snapshots = vec![snap("2026-05", "depository", 20_000.0)];
        let result = compute_trend(&snapshots);
        assert_eq!(result.monthly_snapshots.len(), 1);
        assert!((result.net_worth_change - 0.0).abs() < 0.01);
        assert!((result.latest_net_worth - 20_000.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: empty input → zeros, no data points
    #[test]
    fn empty_snapshots_return_zeros() {
        let result = compute_trend(&[]);
        assert_eq!(result.monthly_snapshots.len(), 0);
        assert!((result.latest_net_worth - 0.0).abs() < 0.01);
        assert!((result.net_worth_change - 0.0).abs() < 0.01);
        assert!(result.biggest_mover.is_none());
    }

    // 9a RED: biggest mover is the type with largest absolute change
    #[test]
    fn biggest_mover_is_largest_absolute_change() {
        let snapshots = vec![
            snap("2026-04", "depository", 10_000.0),
            snap("2026-04", "brokerage", 40_000.0),
            snap("2026-04", "credit", -3_000.0),
            snap("2026-05", "depository", 10_200.0),
            snap("2026-05", "brokerage", 45_000.0),
            snap("2026-05", "credit", -2_900.0),
        ];
        let result = compute_trend(&snapshots);
        let mover = result.biggest_mover.expect("must have a biggest mover");
        assert_eq!(mover.account_type, "brokerage");
        assert!((mover.change - 5_000.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: liability reduction is positive contribution
    #[test]
    fn liability_reduction_is_positive_change() {
        // credit goes from -10000 to -7000: net worth grew by +3000
        let snapshots = vec![
            snap("2026-04", "credit", -10_000.0),
            snap("2026-05", "credit", -7_000.0),
        ];
        let result = compute_trend(&snapshots);
        let credit = result
            .by_account_type
            .get("credit")
            .expect("credit must be present");
        assert!(
            (credit.change - 3_000.0).abs() < 0.01,
            "got {}",
            credit.change
        );
    }

    // 9a RED: asset/liability split from latest month
    #[test]
    fn asset_liability_split_from_latest_month() {
        let snapshots = vec![
            snap("2026-05", "depository", 15_000.0),
            snap("2026-05", "brokerage", 60_000.0),
            snap("2026-05", "credit", -8_000.0),
            snap("2026-05", "loan", -20_000.0),
        ];
        let result = compute_trend(&snapshots);
        assert!((result.total_assets - 75_000.0).abs() < 0.01);
        assert!((result.total_liabilities - 28_000.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: brokerage change in by_account_type matches
    #[test]
    fn by_account_type_reports_per_type_change() {
        let snapshots = vec![
            snap("2026-04", "depository", 10_000.0),
            snap("2026-04", "brokerage", 40_000.0),
            snap("2026-05", "depository", 10_200.0),
            snap("2026-05", "brokerage", 45_000.0),
        ];
        let result = compute_trend(&snapshots);
        let brokerage = result
            .by_account_type
            .get("brokerage")
            .expect("brokerage must be present");
        assert!((brokerage.change - 5_000.0).abs() < 0.01);
        let depository = result
            .by_account_type
            .get("depository")
            .expect("depository must be present");
        assert!((depository.change - 200.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: single month has no biggest_mover
    #[test]
    fn single_month_has_no_biggest_mover() {
        let snapshots = vec![snap("2026-05", "depository", 20_000.0)];
        let result = compute_trend(&snapshots);
        // Single month: biggest_mover is None because there's no delta
        assert!(result.biggest_mover.is_none());
    }
}
