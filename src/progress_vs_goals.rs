//! Pure computation for the `progress_vs_goals` tool.
//!
//! Measures actual finances against the household's stored goals and classifies
//! each as `on track`, `drifting`, or `off` using a single shared banding
//! function. Only goals the household has actually set are reported.

use crate::client::{Account, Cashflow};
use crate::goals::Goals;
use crate::net_worth_trend::AccountTypeSnapshot;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// The result payload returned as JSON by `progress_vs_goals`.
/// Only fields for goals that have been set are present.
/// `guidance` is set when no goals of any kind are configured.
/// This prevents the payload from ever serializing to a silent empty object `{}`.
#[derive(Debug, Serialize, PartialEq)]
pub struct GoalsProgress {
    /// Savings-rate goal progress — present only when the goal is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub savings_rate: Option<GoalStatus>,

    /// Emergency-fund runway goal progress — present only when the goal is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emergency_fund: Option<GoalStatus>,

    /// Debt-payoff progress — present only when the goal is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debt_payoff: Option<DebtPayoffStatus>,

    /// Set when no goals of any kind are configured — explains next steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Classification of a single goal.
#[derive(Debug, Serialize, PartialEq)]
pub struct GoalStatus {
    /// One of `"on track"`, `"drifting"`, or `"off"`.
    pub status: String,
}

/// Enriched debt-payoff progress combining plan-based and pace-based signals.
#[derive(Debug, Serialize, PartialEq)]
pub struct DebtPayoffStatus {
    /// Overall worst-of status: `"on track"`, `"drifting"`, `"off"`, or `"unknown"`.
    pub status: String,
    /// Total debt owed across all credit and loan accounts (positive magnitude).
    pub total_debt: f64,
    /// Whole months from today to the target payoff date. May be ≤ 0 if past.
    /// `None` when `target_date` is malformed (not parseable as `YYYY-MM…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub months_to_target: Option<i64>,
    /// Monthly payment required to clear total_debt by the target date.
    /// None when months_to_target ≤ 0 or total_debt == 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_monthly: Option<f64>,
    /// The household's planned monthly payment from the goal config.
    /// None when monthly_payment is not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_monthly: Option<f64>,
    /// Plan-based signal: is the planned payment sufficient?
    /// None when planned_monthly or required_monthly is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_schedule: Option<String>,
    /// Actual debt reduction last month (prior_debt − current_debt).
    /// None when no prior-month snapshot is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_paydown: Option<f64>,
    /// Pace-based signal: did the household actually pay down enough?
    /// None when actual_paydown or planned_monthly is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_pace: Option<String>,
}

/// Account types that represent debt liabilities.
const DEBT_TYPES: &[&str] = &["credit", "loan"];

/// Compute whole months from (today_year, today_month) to the leading YYYY-MM of target_date.
///
/// Returns `Some(months)` when `target_date` has a parseable `YYYY-MM` prefix with a valid
/// year and a month in `1..=12`. Returns `None` for any malformed input (empty string,
/// year-only, non-numeric parts, month out of range).
///
/// Returns a negative value when the target date is in the past.
/// Only the year and month portions of target_date are used; the day is ignored.
pub fn months_to_target_from_date(
    target_date: &str,
    today_year: i64,
    today_month: u32,
) -> Option<i64> {
    let mut parts = target_date.splitn(3, '-');
    let target_year: i64 = parts.next()?.parse().ok()?;
    let target_month: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&target_month) {
        return None;
    }
    Some((target_year - today_year) * 12 + target_month - today_month as i64)
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

/// Rank status strings for worst-of blending: higher = worse.
fn status_rank(status: &str) -> u8 {
    match status {
        "on track" => 0,
        "drifting" => 1,
        _ => 2, // "off" and anything unexpected
    }
}

/// Compute enriched debt-payoff progress from all inputs.
///
/// `today_year`/`today_month` come from the caller's date seam so this
/// function stays hermetic (no clock reads). `prior_month` and
/// `current_month` are `"YYYY-MM"` strings used to look up snapshot rows.
#[allow(clippy::too_many_arguments)]
pub fn compute_debt_payoff(
    goal: &crate::goals::DebtPayoffGoal,
    accounts: &[Account],
    snapshots: &[AccountTypeSnapshot],
    prior_month: &str,
    current_month: &str,
    today_year: i64,
    today_month: u32,
) -> DebtPayoffStatus {
    let total_debt = total_debt_from_accounts(accounts);
    let months_to_target = months_to_target_from_date(&goal.target_date, today_year, today_month);
    let planned_monthly = goal.monthly_payment;

    // Signal 1 — on schedule?
    let required_monthly = if total_debt == 0.0 {
        Some(0.0)
    } else {
        match months_to_target {
            Some(m) if m > 0 => Some(total_debt / m as f64),
            _ => None,
        }
    };

    let on_schedule: Option<String> = match (planned_monthly, required_monthly) {
        (Some(planned), Some(required)) => Some(classify_goal(planned, required).to_string()),
        _ => None,
    };

    // Signal 2 — on pace?
    let actual_paydown = actual_paydown_from_snapshots(snapshots, prior_month, current_month);

    let on_pace: Option<String> = match (actual_paydown, planned_monthly) {
        (Some(paydown), Some(planned)) if paydown > 0.0 => {
            Some(classify_goal(paydown, planned).to_string())
        }
        (Some(_), Some(_)) => Some("off".to_string()), // debt grew or stayed flat
        _ => None,
    };

    // Overall status: worst of computable signals; "unknown" when none.
    // A malformed target_date (months_to_target == None) does NOT force "off" —
    // it falls through to the sub-status blend, yielding "unknown" when no other
    // signal is available.
    let status = if total_debt == 0.0 {
        "on track".to_string()
    } else if matches!(months_to_target, Some(m) if m <= 0) && total_debt > 0.0 {
        // Valid target date has passed with debt remaining — genuinely off
        "off".to_string()
    } else {
        let sub_statuses: Vec<&str> = [on_schedule.as_deref(), on_pace.as_deref()]
            .iter()
            .flatten()
            .copied()
            .collect();
        if sub_statuses.is_empty() {
            "unknown".to_string()
        } else {
            sub_statuses
                .iter()
                .max_by_key(|s| status_rank(s))
                .copied()
                .unwrap_or("unknown")
                .to_string()
        }
    };

    DebtPayoffStatus {
        status,
        total_debt,
        months_to_target,
        required_monthly,
        planned_monthly,
        on_schedule,
        actual_paydown,
        on_pace,
    }
}

/// Compute the savings rate as a percentage from cashflow data.
/// savings_rate = (income - spending) / income * 100
/// Returns 0.0 when income is zero to avoid division by zero.
pub fn actual_savings_rate_pct(cashflow: &Cashflow) -> f64 {
    if cashflow.income == 0.0 {
        return 0.0;
    }
    (cashflow.income - cashflow.spending) / cashflow.income * 100.0
}

/// Compute actual debt paydown from monthly snapshots.
///
/// Sums debt-type balances for prior_month and current_month, converts each to
/// owed magnitude, and returns `prior_owed − current_owed`. Returns `None` when
/// no prior-month snapshot exists (first data point — can't compute a delta).
/// A negative result means debt grew.
pub fn actual_paydown_from_snapshots(
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

/// Sum the owed amount across all debt-type accounts.
///
/// Monarch stores liability balances as negative numbers. Owed per account =
/// `(-current_balance).max(0.0)` so a positive balance (overpaid credit card)
/// contributes 0, not a negative debt.
pub fn total_debt_from_accounts(accounts: &[Account]) -> f64 {
    accounts
        .iter()
        .filter(|a| DEBT_TYPES.contains(&a.account_type.name.as_str()))
        .map(|a| (-a.current_balance).max(0.0))
        .sum()
}

/// Compute months of expenses covered by liquid cash-equivalent accounts.
///
/// `reserves_months = total_liquid_balance / monthly_spending`
/// Returns 0.0 when monthly spending is zero.
///
/// Liquid cash-equivalent types (included):
///   - `"savings"`      — traditional savings / HYSA
///   - `"checking"`     — checking / demand-deposit accounts
///   - `"depository"`   — Monarch's catch-all for bank deposit accounts
///   - `"money_market"` — money-market accounts / funds
///
/// Excluded: `"brokerage"`, `"investment"`, `"retirement"`, `"credit"`, `"loan"`,
/// and any other type not in the above list.  Investment/retirement balances are
/// not immediately liquid; liability balances would distort the runway figure.
///
/// Semantics rationale: an emergency fund is the cash you can tap in 1–3 business
/// days without selling assets or incurring penalties.  Brokerage accounts require
/// a T+2 settlement cycle and market-risk exposure; retirement accounts carry
/// withdrawal penalties.  Only deposit-type accounts qualify.
pub fn actual_reserve_months(accounts: &[Account], cashflow: &Cashflow) -> f64 {
    if cashflow.spending == 0.0 {
        return 0.0;
    }
    const LIQUID_TYPES: &[&str] = &["savings", "checking", "depository", "money_market"];
    let liquid_balance: f64 = accounts
        .iter()
        .filter(|a| LIQUID_TYPES.contains(&a.account_type.name.as_str()))
        .map(|a| a.current_balance)
        .sum();
    liquid_balance / cashflow.spending
}

// ---------------------------------------------------------------------------
// Pure aggregation — no I/O
// ---------------------------------------------------------------------------

/// Compute goal progress from already-fetched Monarch data and loaded goals.
///
/// `snapshots` are monthly per-type debt history; pass an empty slice when no
/// debt-payoff goal is configured to avoid an unnecessary fetch.
/// `prior_month` / `current_month` are `"YYYY-MM"` strings for snapshot lookup.
/// `today` is `(year, month)` from the caller's date seam (honours `MONARCH_NOW`).
///
/// When no goals have been configured, returns a `guidance` message directing
/// the household to create a goals.toml — consistent with the soft re-auth
/// pattern (a successful MCP result whose body explains next steps).
pub fn compute_progress(
    goals: &Goals,
    accounts: &[Account],
    cashflow: &Cashflow,
    snapshots: &[AccountTypeSnapshot],
    prior_month: &str,
    current_month: &str,
    today: (i64, u32),
) -> GoalsProgress {
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

    let debt_payoff = goals.debt_payoff.as_ref().map(|g| {
        compute_debt_payoff(
            g,
            accounts,
            snapshots,
            prior_month,
            current_month,
            today.0,
            today.1,
        )
    });

    // guidance fires only when no goals of any kind are configured, so the
    // payload is never a silent empty object.
    let has_no_goals = savings_rate.is_none() && emergency_fund.is_none() && debt_payoff.is_none();
    let guidance = has_no_goals.then(|| {
        "No goals configured yet. Add a goals.toml file and set \
         MONARCH_GOALS_FILE to its path to start tracking progress."
            .to_string()
    });

    GoalsProgress {
        savings_rate,
        emergency_fund,
        debt_payoff,
        guidance,
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
        Cashflow {
            income,
            spending,
            prior_month_spending: 0.0,
        }
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
            account_type: AccountType {
                name: "savings".to_string(),
            },
        }
    }

    fn checking_account(balance: f64) -> Account {
        Account {
            id: "c1".to_string(),
            display_name: "Checking".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "checking".to_string(),
            },
        }
    }

    fn money_market_account(balance: f64) -> Account {
        Account {
            id: "mm1".to_string(),
            display_name: "HYSA".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "money_market".to_string(),
            },
        }
    }

    fn brokerage_account(balance: f64) -> Account {
        Account {
            id: "b1".to_string(),
            display_name: "Brokerage".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "brokerage".to_string(),
            },
        }
    }

    fn retirement_account(balance: f64) -> Account {
        Account {
            id: "r1".to_string(),
            display_name: "401k".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "retirement".to_string(),
            },
        }
    }

    // 9a RED: money-market should count as liquid reserves
    #[test]
    fn reserve_months_counts_money_market() {
        // 20000 money-market / 5000 spending = 4 months
        let accounts = vec![money_market_account(20000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 4.0);
    }

    // 9a RED: checking should count as liquid reserves
    #[test]
    fn reserve_months_counts_checking() {
        // 10000 checking / 5000 spending = 2 months
        let accounts = vec![checking_account(10000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 2.0);
    }

    // 9a RED: savings + checking + money_market all combined
    #[test]
    fn reserve_months_combines_all_liquid_types() {
        // 30000 + 10000 + 20000 = 60000 / 5000 = 12 months
        let accounts = vec![
            savings_account(30000.0),
            checking_account(10000.0),
            money_market_account(20000.0),
        ];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 12.0);
    }

    // 9c TRIANGULATE: brokerage must NOT count toward reserves
    #[test]
    fn reserve_months_excludes_brokerage() {
        let accounts = vec![savings_account(30000.0), brokerage_account(100000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 6.0);
    }

    // 9c TRIANGULATE: retirement must NOT count toward reserves
    #[test]
    fn reserve_months_excludes_retirement() {
        let accounts = vec![savings_account(30000.0), retirement_account(200000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 6.0);
    }

    #[test]
    fn reserve_months_divides_savings_by_spending() {
        // 30000 savings / 5000 spending = 6 months
        let accounts = vec![savings_account(30000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 6.0);
    }

    #[test]
    fn reserve_months_ignores_non_liquid_accounts() {
        // Only liquid-cash-equivalent types count; brokerage is excluded
        let accounts = vec![savings_account(30000.0), brokerage_account(10000.0)];
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
    fn reserve_months_no_liquid_accounts_returns_zero() {
        // Only non-liquid accounts (brokerage) — reserves = 0
        let accounts = vec![brokerage_account(10000.0)];
        let cf = cashflow(6000.0, 5000.0);
        assert_eq!(actual_reserve_months(&accounts, &cf), 0.0);
    }

    // -----------------------------------------------------------------------
    // compute_progress — full integration of goals + actuals
    // -----------------------------------------------------------------------

    fn goals_with_savings_rate(pct: f64) -> Goals {
        Goals {
            savings_rate: Some(SavingsRateGoal {
                target_percent: pct,
            }),
            emergency_fund: None,
            debt_payoff: None,
        }
    }

    fn goals_with_emergency_fund(months: f64) -> Goals {
        Goals {
            savings_rate: None,
            emergency_fund: Some(EmergencyFundGoal {
                target_months: months,
            }),
            debt_payoff: None,
        }
    }

    fn empty_goals() -> Goals {
        Goals {
            savings_rate: None,
            emergency_fund: None,
            debt_payoff: None,
        }
    }

    #[test]
    fn savings_rate_on_track_when_actual_exceeds_goal() {
        let goals = goals_with_savings_rate(20.0);
        // income=10000, spending=7500 → 25% actual, goal 20% → on track
        let cf = cashflow(10000.0, 7500.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.savings_rate.unwrap().status, "on track");
        assert!(result.emergency_fund.is_none());
    }

    #[test]
    fn savings_rate_drifting_when_between_half_and_goal() {
        let goals = goals_with_savings_rate(20.0);
        // 17% actual, goal 20% → drifting
        let cf = cashflow(10000.0, 8300.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.savings_rate.unwrap().status, "drifting");
    }

    #[test]
    fn savings_rate_off_when_below_half_goal() {
        let goals = goals_with_savings_rate(20.0);
        // 6% actual (spending=9400), goal 20% → off
        let cf = cashflow(10000.0, 9400.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.savings_rate.unwrap().status, "off");
    }

    #[test]
    fn emergency_fund_on_track_when_reserves_cover_goal() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(42000.0)]; // 42000/5000 = 8.4 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.emergency_fund.unwrap().status, "on track");
    }

    #[test]
    fn emergency_fund_drifting_when_between_half_and_goal() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(20000.0)]; // 20000/5000 = 4 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.emergency_fund.unwrap().status, "drifting");
    }

    #[test]
    fn emergency_fund_off_when_below_half_target() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(10000.0)]; // 10000/5000 = 2 months
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert_eq!(result.emergency_fund.unwrap().status, "off");
    }

    #[test]
    fn unset_goal_is_not_reported() {
        // No goals set at all → both are None
        let goals = empty_goals();
        let result = compute_progress(
            &goals,
            &[],
            &cashflow(10000.0, 8000.0),
            &[],
            "2026-04",
            "2026-05",
            (2026, 5),
        );
        assert!(result.savings_rate.is_none());
        assert!(result.emergency_fund.is_none());
    }

    // --- RED: empty goals yields a guidance message, not a silent empty payload ---

    #[test]
    fn empty_goals_returns_guidance_message() {
        let goals = empty_goals();
        let result = compute_progress(
            &goals,
            &[],
            &cashflow(10000.0, 8000.0),
            &[],
            "2026-04",
            "2026-05",
            (2026, 5),
        );
        let guidance = result.guidance.as_deref().unwrap_or("");
        assert!(
            guidance.contains("no goals") || guidance.contains("goals.toml"),
            "expected guidance about missing goals, got: {guidance:?}"
        );
    }

    // --- TRIANGULATE: a goal being set means no guidance message ---

    #[test]
    fn goals_present_means_no_guidance() {
        let goals = goals_with_savings_rate(20.0);
        let cf = cashflow(10000.0, 7500.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert!(
            result.guidance.is_none(),
            "guidance should be absent when goals are configured"
        );
    }

    // --- debt-payoff-only config: debt_payoff present, guidance absent (ADR 0007 #22 guard removal) ---
    // Previously this test asserted that guidance mentioning "#27" was present.
    // With real debt-payoff computation implemented (issue #27), a debt-only
    // config now returns a populated debt_payoff field instead of a guidance message.

    #[test]
    fn debt_payoff_only_config_returns_debt_payoff_not_guidance() {
        use crate::goals::DebtPayoffGoal;
        let goals = Goals {
            savings_rate: None,
            emergency_fund: None,
            debt_payoff: Some(DebtPayoffGoal {
                target_date: "2027-12-01".to_string(),
                monthly_payment: Some(500.0),
            }),
        };
        let cf = cashflow(10000.0, 8000.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert!(
            result.debt_payoff.is_some(),
            "debt_payoff must be Some when the goal is configured"
        );
        assert!(
            result.guidance.is_none(),
            "guidance must be absent when debt_payoff goal is set; got: {:?}",
            result.guidance
        );
    }

    // --- savings-only config: guidance absent (savings_rate is computable) ---

    #[test]
    fn savings_rate_only_config_has_no_guidance() {
        let goals = goals_with_savings_rate(20.0);
        let cf = cashflow(10000.0, 8000.0);
        let result = compute_progress(&goals, &[], &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert!(
            result.guidance.is_none(),
            "guidance must be absent when savings_rate goal is set"
        );
    }

    // -----------------------------------------------------------------------
    // months_to_target_from_date — pure, hermetic, takes (today_year, today_month)
    // -----------------------------------------------------------------------

    // 9a RED: basic months calculation
    #[test]
    fn months_to_target_8_months_away() {
        // MONARCH_NOW = 2026-05-15, target = 2027-01-01
        // (2027 - 2026)*12 + (1 - 5) = 12 - 4 = 8
        assert_eq!(months_to_target_from_date("2027-01-01", 2026, 5), Some(8));
    }

    // 9c TRIANGULATE: target in the same month → 0
    #[test]
    fn months_to_target_same_month_is_zero() {
        assert_eq!(months_to_target_from_date("2026-05-01", 2026, 5), Some(0));
    }

    // 9c TRIANGULATE: target in the past → negative
    #[test]
    fn months_to_target_past_date_is_negative() {
        // target = 2026-04-01, today = 2026-05 → -1
        assert_eq!(months_to_target_from_date("2026-04-01", 2026, 5), Some(-1));
    }

    // 9c TRIANGULATE: year boundary
    #[test]
    fn months_to_target_crosses_year_boundary() {
        // target = 2027-12-01, today = 2026-05 → (1)*12 + 7 = 19
        assert_eq!(months_to_target_from_date("2027-12-01", 2026, 5), Some(19));
    }

    // -----------------------------------------------------------------------
    // months_to_target_from_date — Option return: None for malformed dates
    // -----------------------------------------------------------------------

    // 9a RED: garbage string → None (not a confident wrong answer)
    #[test]
    fn months_to_target_garbage_is_none() {
        assert_eq!(months_to_target_from_date("garbage", 2026, 5), None);
    }

    // 9a RED: empty string → None
    #[test]
    fn months_to_target_empty_string_is_none() {
        assert_eq!(months_to_target_from_date("", 2026, 5), None);
    }

    // 9a RED: year only (no month) → None
    #[test]
    fn months_to_target_year_only_is_none() {
        assert_eq!(months_to_target_from_date("2027", 2026, 5), None);
    }

    // 9a RED: month out of range (13) → None
    #[test]
    fn months_to_target_month_13_is_none() {
        assert_eq!(months_to_target_from_date("2027-13", 2026, 5), None);
    }

    // 9a RED: month zero → None
    #[test]
    fn months_to_target_month_zero_is_none() {
        assert_eq!(months_to_target_from_date("2027-00", 2026, 5), None);
    }

    // 9c TRIANGULATE: valid full date with day → Some(months)
    #[test]
    fn months_to_target_valid_full_date_is_some() {
        // "2027-12-01" from today (2026, 5) → Some(19)
        assert_eq!(months_to_target_from_date("2027-12-01", 2026, 5), Some(19));
    }

    // 9c TRIANGULATE: same month → Some(0)
    #[test]
    fn months_to_target_same_month_some_zero() {
        assert_eq!(months_to_target_from_date("2026-05-01", 2026, 5), Some(0));
    }

    // 9c TRIANGULATE: past valid date → Some(negative)
    #[test]
    fn months_to_target_past_valid_date_some_negative() {
        // "2026-04-01" from today (2026, 5) → Some(-1)
        assert_eq!(months_to_target_from_date("2026-04-01", 2026, 5), Some(-1));
    }

    // -----------------------------------------------------------------------
    // total_debt_from_accounts
    // -----------------------------------------------------------------------

    fn credit_account(balance: f64) -> Account {
        Account {
            id: "cc1".to_string(),
            display_name: "Credit Card".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "credit".to_string(),
            },
        }
    }

    fn loan_account(balance: f64) -> Account {
        Account {
            id: "l1".to_string(),
            display_name: "Car Loan".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "loan".to_string(),
            },
        }
    }

    // 9a RED: credit account balance -5000 → owed 5000
    #[test]
    fn total_debt_from_negative_credit_balance() {
        let accounts = vec![credit_account(-5000.0)];
        assert_eq!(total_debt_from_accounts(&accounts), 5000.0);
    }

    // 9c TRIANGULATE: loan account also counts
    #[test]
    fn total_debt_includes_loan_accounts() {
        let accounts = vec![credit_account(-3000.0), loan_account(-10000.0)];
        assert_eq!(total_debt_from_accounts(&accounts), 13000.0);
    }

    // 9c TRIANGULATE: overpaid credit card (positive balance) contributes 0
    #[test]
    fn total_debt_overpaid_credit_contributes_zero() {
        let accounts = vec![credit_account(50.0)];
        assert_eq!(total_debt_from_accounts(&accounts), 0.0);
    }

    // 9c TRIANGULATE: savings account is excluded
    #[test]
    fn total_debt_excludes_savings_accounts() {
        let accounts = vec![savings_account(30000.0), credit_account(-2000.0)];
        assert_eq!(total_debt_from_accounts(&accounts), 2000.0);
    }

    // 9c TRIANGULATE: no debt accounts → 0
    #[test]
    fn total_debt_no_debt_accounts_is_zero() {
        let accounts = vec![savings_account(10000.0)];
        assert_eq!(total_debt_from_accounts(&accounts), 0.0);
    }

    // -----------------------------------------------------------------------
    // actual_paydown_from_snapshots
    // -----------------------------------------------------------------------

    fn debt_snap(month: &str, balance: f64) -> AccountTypeSnapshot {
        AccountTypeSnapshot {
            account_type: "credit".to_string(),
            month: month.to_string(),
            balance,
        }
    }

    // 9a RED: prior month had more debt → positive paydown
    #[test]
    fn actual_paydown_positive_when_debt_reduced() {
        // prior: -8000 credit → owed 8000; current: -7000 → owed 7000
        // paydown = 8000 - 7000 = 1000
        let prior_month = "2026-04";
        let current_month = "2026-05";
        let snaps = vec![
            debt_snap(prior_month, -8000.0),
            debt_snap(current_month, -7000.0),
        ];
        let result = actual_paydown_from_snapshots(&snaps, prior_month, current_month);
        assert_eq!(result, Some(1000.0));
    }

    // 9c TRIANGULATE: no prior snapshot → None
    #[test]
    fn actual_paydown_none_when_no_prior_snapshot() {
        let snaps = vec![debt_snap("2026-05", -7000.0)];
        let result = actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05");
        assert_eq!(result, None);
    }

    // 9c TRIANGULATE: debt grew → negative paydown
    #[test]
    fn actual_paydown_negative_when_debt_grew() {
        let snaps = vec![debt_snap("2026-04", -7000.0), debt_snap("2026-05", -8000.0)];
        let result = actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05");
        assert_eq!(result, Some(-1000.0));
    }

    // 9c TRIANGULATE: multiple debt types are summed per month
    #[test]
    fn actual_paydown_sums_multiple_debt_types() {
        let snaps = vec![
            AccountTypeSnapshot {
                account_type: "credit".to_string(),
                month: "2026-04".to_string(),
                balance: -3000.0,
            },
            AccountTypeSnapshot {
                account_type: "loan".to_string(),
                month: "2026-04".to_string(),
                balance: -10000.0,
            },
            AccountTypeSnapshot {
                account_type: "credit".to_string(),
                month: "2026-05".to_string(),
                balance: -2500.0,
            },
            AccountTypeSnapshot {
                account_type: "loan".to_string(),
                month: "2026-05".to_string(),
                balance: -9600.0,
            },
        ];
        // prior_owed = 3000 + 10000 = 13000; current_owed = 2500 + 9600 = 12100
        // paydown = 900
        let result = actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05");
        assert!((result.unwrap() - 900.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: non-debt snapshot types are ignored
    #[test]
    fn actual_paydown_ignores_non_debt_snapshots() {
        let snaps = vec![
            AccountTypeSnapshot {
                account_type: "depository".to_string(),
                month: "2026-04".to_string(),
                balance: 10000.0,
            },
            debt_snap("2026-04", -5000.0),
            AccountTypeSnapshot {
                account_type: "depository".to_string(),
                month: "2026-05".to_string(),
                balance: 11000.0,
            },
            debt_snap("2026-05", -4500.0),
        ];
        let result = actual_paydown_from_snapshots(&snaps, "2026-04", "2026-05");
        assert_eq!(result, Some(500.0));
    }

    // -----------------------------------------------------------------------
    // compute_debt_payoff — full integration of all debt signals
    // -----------------------------------------------------------------------

    use crate::goals::DebtPayoffGoal;

    fn debt_goal(target_date: &str, monthly_payment: Option<f64>) -> DebtPayoffGoal {
        DebtPayoffGoal {
            target_date: target_date.to_string(),
            monthly_payment,
        }
    }

    fn prior_and_current_snaps(prior_owed: f64, current_owed: f64) -> Vec<AccountTypeSnapshot> {
        vec![
            AccountTypeSnapshot {
                account_type: "credit".to_string(),
                month: "2026-04".to_string(),
                balance: -prior_owed,
            },
            AccountTypeSnapshot {
                account_type: "credit".to_string(),
                month: "2026-05".to_string(),
                balance: -current_owed,
            },
        ]
    }

    // 9a RED: on track when both signals are on track
    // today = (2026, 5), target = "2027-01-01" → 8 months, total_debt=7000
    // required = 7000/8 = 875; planned = 1000 ≥ 875 → on_schedule=on track
    // paydown = 8000-7000=1000 ≥ 1000 → on_pace=on track
    #[test]
    fn compute_debt_payoff_on_track_when_both_signals_on_track() {
        let goal = debt_goal("2027-01-01", Some(1000.0));
        let accounts = vec![credit_account(-7000.0)];
        let snaps = prior_and_current_snaps(8000.0, 7000.0);
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "on track");
        assert_eq!(result.on_schedule.as_deref(), Some("on track"));
        assert_eq!(result.on_pace.as_deref(), Some("on track"));
    }

    // 9c TRIANGULATE: worst-of blend picks drifting when on_schedule=drifting, on_pace=on track
    // planned=500, required=875: 500 ≥ 875/2=437.5 → drifting (not off)
    // paydown=500 ≥ planned=500 → on_pace=on track
    // worst → drifting
    #[test]
    fn compute_debt_payoff_worst_of_picks_drifting() {
        let goal = debt_goal("2027-01-01", Some(500.0));
        let accounts = vec![credit_account(-7000.0)];
        let snaps = prior_and_current_snaps(7500.0, 7000.0);
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "drifting");
        assert_eq!(result.on_schedule.as_deref(), Some("drifting"));
        assert_eq!(result.on_pace.as_deref(), Some("on track"));
    }

    // 9c TRIANGULATE: worst-of picks off when planned < half required
    // planned=200, required=875: 200 < 875/2=437.5 → off
    // paydown=200 = planned=200 → on_pace=on track
    // worst → off
    #[test]
    fn compute_debt_payoff_worst_of_picks_off() {
        let goal = debt_goal("2027-01-01", Some(200.0));
        let accounts = vec![credit_account(-7000.0)];
        let snaps = prior_and_current_snaps(7200.0, 7000.0);
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "off");
        assert_eq!(result.on_schedule.as_deref(), Some("off"));
        assert_eq!(result.on_pace.as_deref(), Some("on track"));
    }

    // 9c TRIANGULATE: already paid off → on track, required_monthly=Some(0.0)
    #[test]
    fn compute_debt_payoff_paid_off_is_on_track() {
        let goal = debt_goal("2027-01-01", Some(500.0));
        let accounts: Vec<Account> = vec![];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "on track");
        assert_eq!(result.total_debt, 0.0);
        assert_eq!(result.required_monthly, Some(0.0));
    }

    // 9c TRIANGULATE: target date passed with debt → on_schedule=off, status=off
    #[test]
    fn compute_debt_payoff_target_passed_with_debt_is_off() {
        let goal = debt_goal("2026-04-01", Some(500.0));
        let accounts = vec![credit_account(-3000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "off");
        assert!(result.months_to_target.unwrap_or(0) <= 0);
        assert_eq!(result.required_monthly, None);
    }

    // 9c TRIANGULATE: no monthly_payment → on_schedule/on_pace None; status unknown
    #[test]
    fn compute_debt_payoff_no_monthly_payment_yields_unknown() {
        let goal = debt_goal("2027-01-01", None);
        let accounts = vec![credit_account(-7000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "unknown");
        assert!(result.on_schedule.is_none());
        assert!(result.on_pace.is_none());
        // numbers still surface
        assert_eq!(result.total_debt, 7000.0);
        assert_eq!(result.months_to_target, Some(8));
        assert!(result.required_monthly.is_some());
    }

    // 9c TRIANGULATE: no prior snapshot → on_pace None; status driven only by on_schedule
    // planned=500, required=875: 500 ≥ 437.5 → drifting
    #[test]
    fn compute_debt_payoff_no_prior_snapshot_uses_only_on_schedule() {
        let goal = debt_goal("2027-01-01", Some(500.0));
        let accounts = vec![credit_account(-7000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "drifting");
        assert_eq!(result.on_schedule.as_deref(), Some("drifting"));
        assert_eq!(result.on_pace, None);
    }

    // 9c TRIANGULATE: no prior snapshot, planned far below required → off
    // planned=200, required=875: 200 < 437.5 → off
    #[test]
    fn compute_debt_payoff_no_prior_snapshot_off_schedule() {
        let goal = debt_goal("2027-01-01", Some(200.0));
        let accounts = vec![credit_account(-7000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "off");
        assert_eq!(result.on_schedule.as_deref(), Some("off"));
        assert_eq!(result.on_pace, None);
    }

    // 9c TRIANGULATE: debt grew → on_pace=off
    #[test]
    fn compute_debt_payoff_debt_grew_is_off_pace() {
        let goal = debt_goal("2027-01-01", Some(1000.0));
        let accounts = vec![credit_account(-8000.0)];
        let snaps = prior_and_current_snaps(7000.0, 8000.0); // debt grew
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.on_pace.as_deref(), Some("off"));
    }

    // worst-of blend must be driven by on_pace when it is the worse signal.
    // Mirror of the on_schedule-worse case: here on_schedule="on track" but
    // on_pace="off" (debt grew), so overall status must be "off". Guards against
    // a blend that ignores on_pace.
    #[test]
    fn compute_debt_payoff_worst_of_driven_by_on_pace_off() {
        // target 2027-01-01 from (2026,5) = 8 months; debt 8000; required 8000/8=1000;
        // planned 1000 -> on_schedule "on track". Snapshots prior 7000 / current 8000
        // -> paydown -1000 -> on_pace "off".
        let goal = debt_goal("2027-01-01", Some(1000.0));
        let accounts = vec![credit_account(-8000.0)];
        let snaps = prior_and_current_snaps(7000.0, 8000.0);
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.on_schedule.as_deref(), Some("on track"));
        assert_eq!(result.on_pace.as_deref(), Some("off"));
        assert_eq!(
            result.status, "off",
            "worst-of must surface on_pace 'off' over on_schedule 'on track'"
        );
    }

    // -----------------------------------------------------------------------
    // Audit gaps
    // -----------------------------------------------------------------------

    // Audit: malformed target_date → months_to_target_from_date returns None (not a fabricated value)
    // These cases are now covered by the dedicated None tests above; this test just
    // documents the previously-panic-free but wrong behavior is now correctly None.
    #[test]
    fn months_to_target_malformed_dates_are_none() {
        assert_eq!(months_to_target_from_date("not-a-date", 2026, 5), None);
        assert_eq!(months_to_target_from_date("", 2026, 5), None);
        assert_eq!(months_to_target_from_date("2027", 2026, 5), None);
    }

    // Audit: months_to_target == 0 with debt > 0 → status "off" (target this month, still owed)
    #[test]
    fn compute_debt_payoff_target_this_month_with_debt_is_off() {
        // target = "2026-05-01" and today = (2026, 5) → months_to_target = Some(0)
        // 0 is treated like a passed deadline with remaining debt → off
        let goal = debt_goal("2026-05-01", Some(500.0));
        let accounts = vec![credit_account(-3000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "off");
        assert_eq!(result.months_to_target, Some(0));
        assert_eq!(result.required_monthly, None); // can't compute when months=0
    }

    // -----------------------------------------------------------------------
    // Step 2: malformed target_date propagation — None months_to_target
    // -----------------------------------------------------------------------

    // 9a RED: malformed date + debt + planned payment + no snapshot → "unknown", months_to_target absent
    #[test]
    fn compute_debt_payoff_malformed_date_no_snapshot_is_unknown() {
        let goal = debt_goal("garbage", Some(500.0));
        let accounts = vec![credit_account(-5000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(
            result.status, "unknown",
            "malformed date must not produce 'off'"
        );
        assert_eq!(
            result.months_to_target, None,
            "months_to_target must be absent for malformed date"
        );
        assert_eq!(
            result.required_monthly, None,
            "required_monthly must be None when months unknown"
        );
    }

    // 9a RED: empty date string → same as garbage
    #[test]
    fn compute_debt_payoff_empty_date_no_snapshot_is_unknown() {
        let goal = debt_goal("", Some(500.0));
        let accounts = vec![credit_account(-3000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "unknown");
        assert_eq!(result.months_to_target, None);
    }

    // 9a RED: year-only date → same as garbage
    #[test]
    fn compute_debt_payoff_year_only_date_no_snapshot_is_unknown() {
        let goal = debt_goal("2027", Some(500.0));
        let accounts = vec![credit_account(-3000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "unknown");
        assert_eq!(result.months_to_target, None);
    }

    // 9c TRIANGULATE: malformed date + good prior-snapshot paydown → on_pace signal drives status
    #[test]
    fn compute_debt_payoff_malformed_date_with_good_paydown_uses_on_pace() {
        // planned=500, actual paydown=600 ≥ 500 → on_pace="on track"; no on_schedule
        // worst of {on_track} = "on track" (not "off")
        let goal = debt_goal("2027", Some(500.0));
        let accounts = vec![credit_account(-7000.0)];
        let snaps = prior_and_current_snaps(7600.0, 7000.0); // paid down 600
        let result = compute_debt_payoff(&goal, &accounts, &snaps, "2026-04", "2026-05", 2026, 5);
        assert_ne!(
            result.status, "off",
            "malformed date must not force 'off' when pace is good"
        );
        assert_eq!(result.months_to_target, None);
        assert_eq!(result.on_schedule, None);
        assert_eq!(result.on_pace.as_deref(), Some("on track"));
        assert_eq!(result.status, "on track");
    }

    // 9c TRIANGULATE: genuinely past valid date with debt → still "off"
    #[test]
    fn compute_debt_payoff_genuinely_past_valid_date_is_off() {
        // "2026-04-01" is a valid date, months_to_target = Some(-1), debt > 0 → "off"
        let goal = debt_goal("2026-04-01", Some(500.0));
        let accounts = vec![credit_account(-3000.0)];
        let result = compute_debt_payoff(&goal, &accounts, &[], "2026-04", "2026-05", 2026, 5);
        assert_eq!(result.status, "off");
        assert_eq!(result.months_to_target, Some(-1));
    }

    #[test]
    fn unset_savings_rate_absent_even_when_emergency_fund_present() {
        let goals = goals_with_emergency_fund(6.0);
        let accounts = vec![savings_account(30000.0)];
        let cf = cashflow(6000.0, 5000.0);
        let result = compute_progress(&goals, &accounts, &cf, &[], "2026-04", "2026-05", (2026, 5));
        assert!(
            result.savings_rate.is_none(),
            "savings_rate should be absent when goal not set"
        );
        assert!(result.emergency_fund.is_some());
    }
}
