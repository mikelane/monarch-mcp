//! Pure aggregation logic for the `budget_review` tool.
//!
//! Computes mid-month budget pacing per expense category: given how many days
//! have elapsed in the month, are we spending faster or slower than a straight-
//! line burn rate would predict?
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_budget_review`].
//!
//! See ADR 0013 for the pace taxonomy, tolerance choice, and income/transfer
//! exclusion rationale.

use crate::client::{Budget, Transaction};
use crate::spending_report::transaction_spend_magnitude;
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Tolerance (in percentage points) applied symmetrically around the straight-
/// line pace fraction. A category is `on_track` when:
///   pace_fraction - PACE_TOLERANCE_PP ≤ percent_spent ≤ pace_fraction + PACE_TOLERANCE_PP
///
/// 10 pp gives a ±10% band around the expected burn rate. For example, on day
/// 15 of 30 (50% pace), a category is `on_track` when 40%–60% of its budget
/// has been spent.
///
/// See ADR 0013 for the full rationale.
pub const PACE_TOLERANCE_PP: f64 = 10.0;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Pace status for a single expense category relative to a straight-line burn.
#[derive(Debug, Serialize, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum PaceStatus {
    /// Spending is below the lower tolerance bound: spent% < pace - tolerance.
    Under,
    /// Spending is within the tolerance band around the pace fraction.
    OnTrack,
    /// Spending exceeds the upper tolerance bound but has not yet exceeded
    /// the full budget: spent% > pace + tolerance AND spent < budget.
    Over,
    /// Spending has met or exceeded the full budget amount.
    OverBudget,
}

/// Pacing result for one budgeted expense category.
#[derive(Debug, Serialize, PartialEq)]
pub struct CategoryPacing {
    /// Budget magnitude (positive, in the currency of the account).
    pub budget: f64,
    /// Actual spend so far this month (positive magnitude).
    pub spent: f64,
    /// `budget − spent` (may be negative when over budget).
    pub remaining: f64,
    /// `(spent / budget) * 100`, rounded to the nearest whole percent.
    /// `None` when budget is zero (avoids division-by-zero).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_spent: Option<i64>,
    /// Pace signal relative to a straight-line burn rate.
    pub pace_status: PaceStatus,
}

/// Rollup counters across all paced categories.
#[derive(Debug, Serialize, PartialEq)]
pub struct PaceRollup {
    /// Sum of all expense budget magnitudes (positive).
    pub total_budget: f64,
    /// Sum of all expense spending so far (positive).
    pub total_spent: f64,
    /// Number of categories with `pace_status == OnTrack`.
    pub on_track_count: usize,
    /// Number of categories with `pace_status == Over`.
    pub over_count: usize,
    /// Number of categories with `pace_status == OverBudget`.
    pub over_budget_count: usize,
    /// Number of categories with `pace_status == Under`.
    pub under_count: usize,
}

/// The full budget review payload.
#[derive(Debug, Serialize, PartialEq)]
pub struct BudgetReview {
    /// Per-category pacing data (expense categories only).
    pub by_category: HashMap<String, CategoryPacing>,
    /// Rollup across all paced categories.
    pub rollup: PaceRollup,
}

// ---------------------------------------------------------------------------
// Pure computation — no I/O
// ---------------------------------------------------------------------------

/// Compute the budget review from already-fetched Monarch data.
///
/// `today_day_of_month` is the 1-based day within the current month (1..=31).
/// `days_in_month` is the total number of days in the current month (28..=31).
///
/// Both parameters are expected to come from the caller who obtains "today" via
/// `today_epoch_day()` (honoring `MONARCH_NOW`) and derives the YMD triple from
/// `epoch_days_to_ymd`. Keeping them as parameters preserves hermeticity.
pub fn compute_budget_review(
    budgets: &[Budget],
    transactions: &[Transaction],
    today_day_of_month: u32,
    days_in_month: u32,
) -> BudgetReview {
    let spending_by_category = aggregate_expense_spending(transactions);
    let by_category = build_category_pacings(
        budgets,
        &spending_by_category,
        today_day_of_month,
        days_in_month,
    );
    let rollup = compute_rollup(&by_category);
    BudgetReview {
        by_category,
        rollup,
    }
}

/// Sum expense magnitudes per category, reusing `transaction_spend_magnitude`
/// from `spending_report` for consistency.
///
/// Income and transfer categories contribute zero magnitude and are excluded
/// from the map entirely (see ADR 0013).
fn aggregate_expense_spending(transactions: &[Transaction]) -> HashMap<String, f64> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for txn in transactions {
        let magnitude = transaction_spend_magnitude(txn);
        // Only expense categories (and unknown-group fallback) appear in the map.
        // Income and transfer categories are excluded from pacing entirely.
        if matches!(txn.category.group_type.as_deref(), Some("expense") | None) {
            *totals.entry(txn.category.name.clone()).or_insert(0.0) += magnitude;
        }
    }
    totals
}

/// Build per-category pacing entries for all budgeted expense categories.
///
/// In production, `Budget.category.group_type` is always `None` because the
/// `GetJointPlanningData` join path does not fetch group_type (ADR 0013). The
/// exclusion of income/transfer categories is therefore handled via the
/// `aggregate_expense_spending` map: income and transfer transactions contribute
/// zero magnitude and never appear in that map, so their budget categories
/// naturally show `spent = 0` and pace as `Under`. We additionally respect an
/// explicit `Some("income") | Some("transfer")` group_type when it IS present
/// (e.g. in unit tests or future API changes) to keep behavior consistent.
///
/// A budgeted category with zero spending is included (spent = 0.0).
/// A category with spending but no budget is not included (no pacing reference).
fn build_category_pacings(
    budgets: &[Budget],
    spending: &HashMap<String, f64>,
    today_day_of_month: u32,
    days_in_month: u32,
) -> HashMap<String, CategoryPacing> {
    budgets
        .iter()
        .filter_map(|b| {
            // When group_type is explicitly income or transfer, exclude from pacing.
            // In production group_type is None for all budgets (the join path omits it).
            // For None, we include the category and let the spending map drive the result
            // (income/transfer transactions contribute zero via transaction_spend_magnitude,
            // so they naturally produce spent=0 and appear as Under or OnTrack).
            if matches!(
                b.category.group_type.as_deref(),
                Some("income") | Some("transfer")
            ) {
                return None;
            }

            let budget_mag = b.amount.abs();
            // Zero-budget categories: include with OverBudget if any spend,
            // otherwise Under (vacuously). This matches spending_report behavior.
            let spent = *spending.get(&b.category.name).unwrap_or(&0.0);
            let remaining = budget_mag - spent;
            let percent_spent = compute_percent_spent(spent, budget_mag);
            let pace_status = classify_pace(
                spent,
                budget_mag,
                percent_spent,
                today_day_of_month,
                days_in_month,
            );

            Some((
                b.category.name.clone(),
                CategoryPacing {
                    budget: budget_mag,
                    spent,
                    remaining,
                    percent_spent,
                    pace_status,
                },
            ))
        })
        .collect()
}

/// Compute `(spent / budget_mag) * 100` rounded to the nearest whole percent.
///
/// Returns `None` when `budget_mag` is zero to avoid division-by-zero.
fn compute_percent_spent(spent: f64, budget_mag: f64) -> Option<i64> {
    if budget_mag == 0.0 {
        return None;
    }
    Some(((spent / budget_mag) * 100.0).round() as i64)
}

/// Classify the pace status for a category.
///
/// Taxonomy (in priority order):
/// 1. `OverBudget` — spent ≥ budget magnitude (regardless of time elapsed).
/// 2. `Over` — percent_spent > pace_fraction_pct + PACE_TOLERANCE_PP.
/// 3. `Under` — percent_spent < pace_fraction_pct - PACE_TOLERANCE_PP.
/// 4. `OnTrack` — within the tolerance band.
///
/// `pace_fraction_pct` = `(today_day_of_month / days_in_month) * 100`.
/// On day 1 of 30 this is 3.33%; on day 15 of 30 it is 50%; on day 30 it is 100%.
///
/// For zero-budget categories `percent_spent` is `None`; the category is
/// classified `OverBudget` if any spend exists (matches `spending_report`
/// over-budget logic for zero-budget), `Under` if no spend.
fn classify_pace(
    spent: f64,
    budget_mag: f64,
    percent_spent: Option<i64>,
    today_day_of_month: u32,
    days_in_month: u32,
) -> PaceStatus {
    // OverBudget: spending meets or exceeds the budget magnitude.
    if spent >= budget_mag && budget_mag > 0.0 {
        return PaceStatus::OverBudget;
    }
    // Zero-budget special case: any spend → OverBudget; no spend → Under.
    if budget_mag == 0.0 {
        return if spent > 0.0 {
            PaceStatus::OverBudget
        } else {
            PaceStatus::Under
        };
    }

    let percent_f = match percent_spent {
        Some(p) => p as f64,
        None => return PaceStatus::Under, // defensive; budget_mag > 0 already handled
    };

    let pace_pct = (today_day_of_month as f64 / days_in_month as f64) * 100.0;

    if percent_f > pace_pct + PACE_TOLERANCE_PP {
        PaceStatus::Over
    } else if percent_f < pace_pct - PACE_TOLERANCE_PP {
        PaceStatus::Under
    } else {
        PaceStatus::OnTrack
    }
}

/// Compute rollup totals and counts from per-category pacings.
fn compute_rollup(by_category: &HashMap<String, CategoryPacing>) -> PaceRollup {
    let mut total_budget = 0.0;
    let mut total_spent = 0.0;
    let mut on_track_count = 0;
    let mut over_count = 0;
    let mut over_budget_count = 0;
    let mut under_count = 0;

    for pacing in by_category.values() {
        total_budget += pacing.budget;
        total_spent += pacing.spent;
        match pacing.pace_status {
            PaceStatus::OnTrack => on_track_count += 1,
            PaceStatus::Over => over_count += 1,
            PaceStatus::OverBudget => over_budget_count += 1,
            PaceStatus::Under => under_count += 1,
        }
    }

    PaceRollup {
        total_budget,
        total_spent,
        on_track_count,
        over_count,
        over_budget_count,
        under_count,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
//
// Fixtures use the REAL Monarch sign convention:
//   - expense outflows: NEGATIVE amounts (e.g. -360.0 for dining)
//   - income: POSITIVE amounts (e.g. +5000.0 for a paycheck)
//   - refunds landing in expense categories: POSITIVE
//   - expense budgets: NEGATIVE for outflows (e.g. -400.0 for Restaurants)
//     OR positive (historically both appear; we use .abs() consistently)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Budget, Category, Transaction};

    /// Build an expense transaction (negative amount, group_type = "expense").
    fn make_expense_txn(merchant: &str, amount: f64, category: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}"),
            amount,
            date: "2026-05-15".to_string(),
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

    /// Build an income transaction (positive amount, group_type = "income").
    fn make_income_txn(merchant: &str, amount: f64, category: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}"),
            amount,
            date: "2026-05-01".to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("income".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    /// Build a transfer transaction (group_type = "transfer").
    fn make_transfer_txn(merchant: &str, amount: f64, category: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}"),
            amount,
            date: "2026-05-05".to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("transfer".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    /// Build a budget with an expense group_type.
    fn make_expense_budget(category: &str, amount: f64) -> Budget {
        Budget {
            category: Category {
                name: category.to_string(),
                group_type: Some("expense".into()),
            },
            amount,
        }
    }

    /// Build an income budget.
    fn make_income_budget(category: &str, amount: f64) -> Budget {
        Budget {
            category: Category {
                name: category.to_string(),
                group_type: Some("income".into()),
            },
            amount,
        }
    }

    /// Build a transfer budget.
    fn make_transfer_budget(category: &str, amount: f64) -> Budget {
        Budget {
            category: Category {
                name: category.to_string(),
                group_type: Some("transfer".into()),
            },
            amount,
        }
    }

    // -----------------------------------------------------------------------
    // Core pace classification — day 15 of 30 (50% pace)
    // -----------------------------------------------------------------------

    #[test]
    fn category_spent_faster_than_month_elapsed_is_over() {
        // Day 15 of 30 = 50% pace. Restaurants budget 400, spent 360 = 90%.
        // 90% > 50% + 10% tolerance → Over.
        let budgets = vec![make_expense_budget("Restaurants", -400.0)];
        let txns = vec![make_expense_txn("Restaurant Co", -360.0, "Restaurants")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Restaurants").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::Over, "{cat:?}");
        assert_eq!(cat.remaining, 40.0);
        assert_eq!(cat.spent, 360.0);
        assert_eq!(cat.budget, 400.0);
    }

    #[test]
    fn category_tracking_with_calendar_is_on_track() {
        // Day 15 of 30 = 50% pace. Groceries budget 600, spent 290 ≈ 48%.
        // 48% is within [40%, 60%] tolerance band → OnTrack.
        let budgets = vec![make_expense_budget("Groceries", -600.0)];
        let txns = vec![make_expense_txn("Grocery Store", -290.0, "Groceries")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Groceries").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    #[test]
    fn category_already_over_budget_is_flagged_over_budget() {
        // Phone budget 200, spent 210 → spent ≥ budget → OverBudget.
        let budgets = vec![make_expense_budget("Phone", -200.0)];
        let txns = vec![make_expense_txn("Phone Co", -210.0, "Phone")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Phone").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OverBudget, "{cat:?}");
        assert!(
            (cat.remaining - (-10.0)).abs() < 0.001,
            "remaining should be -10, got {}",
            cat.remaining
        );
    }

    #[test]
    fn category_exactly_at_budget_is_over_budget() {
        // Spent exactly equals budget → OverBudget (spent >= budget).
        let budgets = vec![make_expense_budget("Utilities", -300.0)];
        let txns = vec![make_expense_txn("Utility Co", -300.0, "Utilities")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Utilities").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OverBudget, "{cat:?}");
    }

    // -----------------------------------------------------------------------
    // Tolerance edges
    // -----------------------------------------------------------------------

    #[test]
    fn exactly_at_upper_tolerance_edge_is_on_track() {
        // Day 15 of 30 = 50% pace. Upper edge: 50% + 10% = 60%.
        // 60% spent → still OnTrack (boundary is exclusive: > 60% → Over).
        let budgets = vec![make_expense_budget("Dining", -100.0)];
        let txns = vec![make_expense_txn("Diner", -60.0, "Dining")]; // 60%
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Dining").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    #[test]
    fn one_cent_above_upper_tolerance_edge_is_over() {
        // Day 15 of 30 = 50% pace. 61% > 60% → Over.
        let budgets = vec![make_expense_budget("Dining", -100.0)];
        let txns = vec![make_expense_txn("Diner", -61.0, "Dining")]; // 61%
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Dining").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::Over, "{cat:?}");
    }

    #[test]
    fn exactly_at_lower_tolerance_edge_is_on_track() {
        // Day 15 of 30 = 50% pace. Lower edge: 50% - 10% = 40%.
        // 40% spent → OnTrack (boundary is exclusive: < 40% → Under).
        let budgets = vec![make_expense_budget("Dining", -100.0)];
        let txns = vec![make_expense_txn("Diner", -40.0, "Dining")]; // 40%
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Dining").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    #[test]
    fn one_cent_below_lower_tolerance_edge_is_under() {
        // Day 15 of 30 = 50% pace. 39% < 40% → Under.
        let budgets = vec![make_expense_budget("Dining", -100.0)];
        let txns = vec![make_expense_txn("Diner", -39.0, "Dining")]; // 39%
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Dining").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::Under, "{cat:?}");
    }

    // -----------------------------------------------------------------------
    // Day boundary tests (first, last, mid)
    // -----------------------------------------------------------------------

    #[test]
    fn day_1_of_30_pace_is_3_percent() {
        // Day 1 of 30 ≈ 3.33% pace. Lower edge ≈ 3.33% - 10% = -6.67%.
        // Any spend > 0% puts us above the lower bound.
        // Upper edge ≈ 3.33% + 10% = 13.33%.
        // 10% spent → on track (between ~-6.67% and ~13.33%).
        let budgets = vec![make_expense_budget("Groceries", -200.0)];
        let txns = vec![make_expense_txn("Grocery Store", -20.0, "Groceries")]; // 10%
        let result = compute_budget_review(&budgets, &txns, 1, 30);
        let cat = result.by_category.get("Groceries").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    #[test]
    fn day_1_of_30_with_no_spending_is_on_track() {
        // On day 1 with no spending, lower bound is negative, so 0% is on track.
        let budgets = vec![make_expense_budget("Groceries", -200.0)];
        let result = compute_budget_review(&budgets, &[], 1, 30);
        let cat = result.by_category.get("Groceries").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    #[test]
    fn last_day_of_30_with_full_spend_is_on_track() {
        // Day 30 of 30 = 100% pace. 100% spent → at upper tolerance edge.
        // 100% is not > 110% → OnTrack.
        let budgets = vec![make_expense_budget("Rent", -1500.0)];
        let txns = vec![make_expense_txn("Landlord", -1500.0, "Rent")];
        // spent == budget → OverBudget (spent >= budget), not OnTrack.
        // Exact budget hit IS over_budget per the priority rule.
        let result = compute_budget_review(&budgets, &txns, 30, 30);
        let cat = result.by_category.get("Rent").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OverBudget, "{cat:?}");
    }

    #[test]
    fn last_day_of_30_with_99_percent_spend_is_on_track() {
        // Day 30 of 30 = 100% pace. 99% spent → 99% is in [90%, 110%] → OnTrack.
        let budgets = vec![make_expense_budget("Rent", -1500.0)];
        let txns = vec![make_expense_txn("Landlord", -1485.0, "Rent")]; // 99%
        let result = compute_budget_review(&budgets, &txns, 30, 30);
        let cat = result.by_category.get("Rent").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::OnTrack, "{cat:?}");
    }

    // -----------------------------------------------------------------------
    // Income and transfer exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn income_budget_is_excluded_from_pacing() {
        let budgets = vec![
            make_income_budget("Paychecks", 5000.0),
            make_expense_budget("Groceries", -600.0),
        ];
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks"),
            make_expense_txn("Grocery Store", -300.0, "Groceries"),
        ];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        assert!(
            !result.by_category.contains_key("Paychecks"),
            "income category must not appear in pacing: {:?}",
            result.by_category.keys().collect::<Vec<_>>()
        );
        assert!(result.by_category.contains_key("Groceries"));
    }

    #[test]
    fn transfer_budget_is_excluded_from_pacing() {
        let budgets = vec![
            make_transfer_budget("Credit Card Payment", -2000.0),
            make_expense_budget("Dining", -400.0),
        ];
        let txns = vec![
            make_transfer_txn("Chase CC", -2000.0, "Credit Card Payment"),
            make_expense_txn("Diner", -200.0, "Dining"),
        ];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        assert!(
            !result.by_category.contains_key("Credit Card Payment"),
            "transfer category must not appear in pacing"
        );
        assert!(result.by_category.contains_key("Dining"));
    }

    #[test]
    fn income_and_transfer_transactions_do_not_count_as_spend() {
        // Only the expense transaction contributes; income and transfer are excluded.
        let budgets = vec![make_expense_budget("Groceries", -600.0)];
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks"),
            make_transfer_txn("Chase CC", -2000.0, "Credit Card Payment"),
            make_expense_txn("Grocery Store", -300.0, "Groceries"),
        ];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Groceries").unwrap();
        assert_eq!(
            cat.spent, 300.0,
            "spent must be 300, not inflated by income/transfer"
        );
    }

    // -----------------------------------------------------------------------
    // Category with spend but no budget
    // -----------------------------------------------------------------------

    #[test]
    fn category_with_spend_but_no_budget_is_not_in_pacing() {
        // Unbudgeted categories are invisible in budget_review (no pacing reference).
        let budgets = vec![make_expense_budget("Groceries", -600.0)];
        let txns = vec![
            make_expense_txn("Airline", -500.0, "Travel"),
            make_expense_txn("Grocery Store", -300.0, "Groceries"),
        ];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        assert!(
            !result.by_category.contains_key("Travel"),
            "unbudgeted Travel must not appear in pacing"
        );
        assert!(result.by_category.contains_key("Groceries"));
    }

    // -----------------------------------------------------------------------
    // Budgeted category with no spend
    // -----------------------------------------------------------------------

    #[test]
    fn budgeted_category_with_zero_spend_is_included_and_on_track_early() {
        // On day 1 of 30, zero spend with lower bound negative → OnTrack.
        let budgets = vec![make_expense_budget("Gym", -50.0)];
        let result = compute_budget_review(&budgets, &[], 1, 30);
        let cat = result.by_category.get("Gym").unwrap();
        assert_eq!(cat.spent, 0.0);
        assert_eq!(cat.budget, 50.0);
        assert_eq!(cat.remaining, 50.0);
    }

    #[test]
    fn budgeted_category_with_zero_spend_mid_month_is_under() {
        // On day 20 of 30 ≈ 67% pace. Lower edge = 57%. 0% < 57% → Under.
        let budgets = vec![make_expense_budget("Gym", -50.0)];
        let result = compute_budget_review(&budgets, &[], 20, 30);
        let cat = result.by_category.get("Gym").unwrap();
        assert_eq!(cat.pace_status, PaceStatus::Under, "{cat:?}");
    }

    // -----------------------------------------------------------------------
    // MONARCH_NOW-driven pace fraction (day derived from today_day_of_month)
    // -----------------------------------------------------------------------

    #[test]
    fn pace_fraction_changes_with_day_of_month() {
        // Same category and spend, different day: on day 10 vs day 20.
        // Budget 100, spent 60 = 60%.
        // Day 10 of 30: pace = 33%. Upper edge = 43%. 60% > 43% → Over.
        // Day 20 of 30: pace = 67%. Lower edge = 57%. 60% is in [57%, 77%] → OnTrack.
        let budgets = vec![make_expense_budget("Misc", -100.0)];
        let txns = vec![make_expense_txn("Misc Co", -60.0, "Misc")];

        let result_day10 = compute_budget_review(&budgets, &txns, 10, 30);
        let cat_day10 = result_day10.by_category.get("Misc").unwrap();
        assert_eq!(
            cat_day10.pace_status,
            PaceStatus::Over,
            "day 10: {cat_day10:?}"
        );

        let result_day20 = compute_budget_review(&budgets, &txns, 20, 30);
        let cat_day20 = result_day20.by_category.get("Misc").unwrap();
        assert_eq!(
            cat_day20.pace_status,
            PaceStatus::OnTrack,
            "day 20: {cat_day20:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Percent_spent field
    // -----------------------------------------------------------------------

    #[test]
    fn percent_spent_rounds_correctly() {
        // 360 / 400 = 90%
        let budgets = vec![make_expense_budget("Restaurants", -400.0)];
        let txns = vec![make_expense_txn("Restaurant Co", -360.0, "Restaurants")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Restaurants").unwrap();
        assert_eq!(cat.percent_spent, Some(90));
    }

    #[test]
    fn percent_spent_is_none_for_zero_budget() {
        let budgets = vec![make_expense_budget("Misc", 0.0)];
        let result = compute_budget_review(&budgets, &[], 15, 30);
        let cat = result.by_category.get("Misc").unwrap();
        assert_eq!(cat.percent_spent, None);
    }

    // -----------------------------------------------------------------------
    // Rollup counts
    // -----------------------------------------------------------------------

    #[test]
    fn rollup_counts_are_correct() {
        // Day 15 of 30 (50% pace, tolerance ±10 → on track: [40%, 60%]).
        // Restaurants: 360/400 = 90% → Over
        // Groceries:   290/600 = 48% → OnTrack
        // Phone:       210/200 → OverBudget
        // Gym:         0/50 = 0% → Under
        let budgets = vec![
            make_expense_budget("Restaurants", -400.0),
            make_expense_budget("Groceries", -600.0),
            make_expense_budget("Phone", -200.0),
            make_expense_budget("Gym", -50.0),
        ];
        let txns = vec![
            make_expense_txn("Restaurant Co", -360.0, "Restaurants"),
            make_expense_txn("Grocery Store", -290.0, "Groceries"),
            make_expense_txn("Phone Co", -210.0, "Phone"),
        ];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let rollup = &result.rollup;
        assert_eq!(rollup.over_count, 1, "over: {:?}", rollup);
        assert_eq!(rollup.on_track_count, 1, "on_track: {:?}", rollup);
        assert_eq!(rollup.over_budget_count, 1, "over_budget: {:?}", rollup);
        assert_eq!(rollup.under_count, 1, "under: {:?}", rollup);
        assert_eq!(rollup.total_budget, 1250.0, "total_budget: {:?}", rollup);
        assert!(
            (rollup.total_spent - 860.0).abs() < 0.001,
            "total_spent: {:?}",
            rollup
        );
    }

    // -----------------------------------------------------------------------
    // Negative budget (loan-repayment style) — .abs() ensures consistency.
    // -----------------------------------------------------------------------

    #[test]
    fn negative_budget_magnitude_is_used_correctly() {
        // Budget -1280 (loan). Spent 1000. Magnitude 1000/1280 = 78%.
        // Day 15/30 = 50% pace. 78% > 60% upper edge → Over.
        let budgets = vec![make_expense_budget("Loan Repayment", -1280.0)];
        let txns = vec![make_expense_txn("Loan Co", -1000.0, "Loan Repayment")];
        let result = compute_budget_review(&budgets, &txns, 15, 30);
        let cat = result.by_category.get("Loan Repayment").unwrap();
        assert_eq!(cat.budget, 1280.0, "budget should be the magnitude");
        assert_eq!(cat.percent_spent, Some(78));
        assert_eq!(cat.pace_status, PaceStatus::Over, "{cat:?}");
    }
}
