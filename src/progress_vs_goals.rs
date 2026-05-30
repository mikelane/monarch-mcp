//! Pure computation for the `progress_vs_goals` tool.
//!
//! Measures actual finances against the household's stored goals and classifies
//! each as `on track`, `drifting`, or `off` using a single shared banding
//! function. Only goals the household has actually set are reported.

use crate::client::{Account, Cashflow};
use crate::goals::Goals;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// The result payload returned as JSON by `progress_vs_goals`.
/// Only fields for goals that have been set are present.
#[derive(Debug, Serialize, PartialEq)]
pub struct GoalsProgress {
    /// Savings-rate goal progress — present only when the goal is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub savings_rate: Option<GoalStatus>,

    /// Emergency-fund runway goal progress — present only when the goal is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emergency_fund: Option<GoalStatus>,
}

/// Classification of a single goal.
#[derive(Debug, Serialize, PartialEq)]
pub struct GoalStatus {
    /// One of `"on track"`, `"drifting"`, or `"off"`.
    pub status: String,
}

// ---------------------------------------------------------------------------
// Banding — shared classifier for all goal types
// ---------------------------------------------------------------------------

/// Classify `actual` against `goal` using the standard three-band rule:
///
/// - `"on track"`: actual ≥ goal
/// - `"off"`:      actual < goal / 2
/// - `"drifting"`: anything in between (goal/2 ≤ actual < goal)
pub fn classify_goal(actual: f64, goal: f64) -> &'static str {
    if actual >= goal {
        "on track"
    } else if actual < goal / 2.0 {
        "off"
    } else {
        "drifting"
    }
}

// ---------------------------------------------------------------------------
// Actuals computation helpers
// ---------------------------------------------------------------------------

/// Compute the savings rate as a percentage from cashflow data.
/// savings_rate = (income - spending) / income * 100
/// Returns 0.0 when income is zero to avoid division by zero.
pub fn actual_savings_rate_pct(cashflow: &Cashflow) -> f64 {
    if cashflow.income == 0.0 {
        return 0.0;
    }
    (cashflow.income - cashflow.spending) / cashflow.income * 100.0
}

/// Compute months of expenses covered by savings-type accounts.
/// reserves_months = total_savings_balance / monthly_spending
/// Returns 0.0 when monthly spending is zero.
pub fn actual_reserve_months(accounts: &[Account], cashflow: &Cashflow) -> f64 {
    if cashflow.spending == 0.0 {
        return 0.0;
    }
    let savings_balance: f64 = accounts
        .iter()
        .filter(|a| a.account_type.name == "savings")
        .map(|a| a.current_balance)
        .sum();
    savings_balance / cashflow.spending
}

// ---------------------------------------------------------------------------
// Pure aggregation — no I/O
// ---------------------------------------------------------------------------

/// Compute goal progress from already-fetched Monarch data and loaded goals.
pub fn compute_progress(goals: &Goals, accounts: &[Account], cashflow: &Cashflow) -> GoalsProgress {
    let savings_rate = goals.savings_rate.as_ref().map(|g| {
        let actual = actual_savings_rate_pct(cashflow);
        GoalStatus {
            status: classify_goal(actual, g.target_percent).to_string(),
        }
    });

    let emergency_fund = goals.emergency_fund.as_ref().map(|g| {
        let actual = actual_reserve_months(accounts, cashflow);
        GoalStatus {
            status: classify_goal(actual, g.target_months).to_string(),
        }
    });

    GoalsProgress {
        savings_rate,
        emergency_fund,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED → GREEN → TRIANGULATE → GREEN → REFACTOR
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{AccountType, Cashflow};
    use crate::goals::{EmergencyFundGoal, Goals, SavingsRateGoal};

    // -----------------------------------------------------------------------
    // Classifier banding — boundary tests
    // -----------------------------------------------------------------------

    // 9a RED: exactly at goal → on track
    #[test]
    fn classify_at_goal_is_on_track() {
        assert_eq!(classify_goal(20.0, 20.0), "on track");
    }

    // 9a RED: above goal → on track
    #[test]
    fn classify_above_goal_is_on_track() {
        assert_eq!(classify_goal(25.0, 20.0), "on track");
    }

    // 9c TRIANGULATE: exactly at half → drifting (not off)
    #[test]
    fn classify_at_half_goal_is_drifting() {
        assert_eq!(classify_goal(10.0, 20.0), "drifting");
    }

    // 9c TRIANGULATE: just below half → off
    #[test]
    fn classify_just_below_half_is_off() {
        // 9.9 < 20/2=10 → off
        assert_eq!(classify_goal(9.9, 20.0), "off");
    }

    // 9c TRIANGULATE: midrange → drifting
    #[test]
    fn classify_midrange_is_drifting() {
        assert_eq!(classify_goal(17.0, 20.0), "drifting");
    }

    // 9c TRIANGULATE: far below half → off
    #[test]
    fn classify_far_below_half_is_off() {
        assert_eq!(classify_goal(6.0, 20.0), "off");
    }

    // 9c TRIANGULATE: zero → off (unless goal is also zero)
    #[test]
    fn classify_zero_actual_is_off() {
        assert_eq!(classify_goal(0.0, 20.0), "off");
    }

    // 9c TRIANGULATE: emergency-fund banding (6-month target)
    #[test]
    fn classify_emergency_fund_at_goal_is_on_track() {
        assert_eq!(classify_goal(6.0, 6.0), "on track");
    }

    #[test]
    fn classify_emergency_fund_above_goal_is_on_track() {
        assert_eq!(classify_goal(7.0, 6.0), "on track");
    }

    #[test]
    fn classify_emergency_fund_drifting() {
        assert_eq!(classify_goal(4.0, 6.0), "drifting");
    }

    #[test]
    fn classify_emergency_fund_off() {
        assert_eq!(classify_goal(2.0, 6.0), "off");
    }

    // -----------------------------------------------------------------------
    // Savings-rate computation
    // -----------------------------------------------------------------------

    fn cashflow(income: f64, spending: f64) -> Cashflow {
        Cashflow { income, spending, prior_month_spending: 0.0 }
    }

    #[test]
    fn savings_rate_pct_is_correct() {
        // (10000 - 7500) / 10000 * 100 = 25%
        assert_eq!(actual_savings_rate_pct(&cashflow(10000.0, 7500.0)), 25.0);
    }

    #[test]
    fn savings_rate_pct_zero_income_returns_zero() {
        assert_eq!(actual_savings_rate_pct(&cashflow(0.0, 0.0)), 0.0);
    }

    #[test]
    fn savings_rate_pct_full_spending_returns_zero() {
        // All income spent → 0% savings
        assert_eq!(actual_savings_rate_pct(&cashflow(10000.0, 10000.0)), 0.0);
    }

    // -----------------------------------------------------------------------
    // Reserve months computation
    // -----------------------------------------------------------------------

    fn savings_account(balance: f64) -> Account {
        Account {
            id: "s1".to_string(),
            display_name: "Emergency Fund".to_string(),
            current_balance: balance,
            account_type: AccountType { name: "savings".to_string() },
        }
    }

    fn checking_account(balance: f64) -> Account {
        Account {
            id: "c1".to_string(),
            display_name: "Checking".to_string(),
            current_balance: balance,
            account_type: AccountType { name: "checking".to_string() },
        }
    }

    #[test]
    fn reserve_months_divides_savings_by_spending() {
        // 30000 savings / 5000 spending = 6 months
        let accounts = vec![savings_account(30000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 6.0);
    }

    #[test]
    fn reserve_months_ignores_non_savings_accounts() {
        // Only savings accounts count; checking is excluded
        let accounts = vec![savings_account(30000.0), checking_account(10000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 6.0);
    }

    #[test]
    fn reserve_months_zero_spending_returns_zero() {
        let accounts = vec![savings_account(30000.0)];
        let cf = cashflow(0.0, 0.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 0.0);
    }

    #[test]
    fn reserve_months_no_savings_accounts_returns_zero() {
        let accounts = vec![checking_account(10000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 0.0);
    }

    // -----------------------------------------------------------------------
    // compute_progress — full integration of goals + actuals
    // -----------------------------------------------------------------------

    fn goals_with_savings_rate(pct: f64) -> Goals {
        Goals {
            savings_rate: Some(SavingsRateGoal { target_percent: pct }),
            emergency_fund: None,
            debt_payoff: None,
        }
    }

    fn goals_with_emergency_fund(months: f64) -> Goals {
        Goals {
            savings_rate: None,
            emergency_fund: Some(EmergencyFundGoal { target_months: months }),
            debt_payoff: None,
        }
    }

    fn empty_goals() -> Goals {
        Goals { savings_rate: None, emergency_fund: None, debt_payoff: None }
    }

    #[test]
    fn savings_rate_on_track_when_actual_exceeds_goal() {
        let goals = goals_with_savings_rate(20.0);
        // income=10000, spending=7500 → 25% actual, goal 20% → on track
        let cf = cashflow(10000.0, 7500.0);
        let result = compute_progress(&goals, &[], &cf);
        assert_eq!(result.savings_rate.unwrap().status, "on track");
        assert!(result.emergency_fund.is_none());
    }

    #[test]
    fn savings_rate_drifting_when_between_half_and_goal() {
        let goals = goals_with_savings_rate(20.0);
        // 17% actual, goal 20% → drifting
        let cf = cashflow(10000.0, 8300.0);
        let result = compute_progress(&goals, &[], &cf);
        assert_eq!(result.savings_rate.unwrap().status, "drifting");
    }

    #[test]
    fn savings_rate_off_when_below_half_goal() {
        let goals = goals_with_savings_rate(20.0);
        // 6% actual (spending=9400), goal 20% → off
        let cf = cashflow(10000.0, 9400.0);
        let result = compute_progress(&goals, &[], &cf);
        assert_eq!(result.savings_rate.unwrap().status, "off");
    }

    #[test]
    fn emergency_fund_on_track_when_reserves_cover_goal() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(42000.0)]; // 42000/5000 = 8.4 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf);
        assert_eq!(result.emergency_fund.unwrap().status, "on track");
    }

    #[test]
    fn emergency_fund_drifting_when_between_half_and_goal() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(20000.0)]; // 20000/5000 = 4 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf);
        assert_eq!(result.emergency_fund.unwrap().status, "drifting");
    }

    #[test]
    fn emergency_fund_off_when_below_half_target() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(10000.0)]; // 10000/5000 = 2 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf);
        assert_eq!(result.emergency_fund.unwrap().status, "off");
    }

    #[test]
    fn unset_goal_is_not_reported() {
        // No goals set at all → both are None
        let goals = empty_goals();
        let result = compute_progress(&goals, &[], &cashflow(10000.0, 8000.0));
        assert!(result.savings_rate.is_none());
        assert!(result.emergency_fund.is_none());
    }

    #[test]
    fn unset_savings_rate_absent_even_when_emergency_fund_present() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(30000.0)];
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf);
        assert!(result.savings_rate.is_none(), "savings_rate should be absent when goal not set");
        assert!(result.emergency_fund.is_some());
    }
}
