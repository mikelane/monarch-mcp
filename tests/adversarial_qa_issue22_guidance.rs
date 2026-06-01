//! Adversarial QA (Gate 3) — guidance-field correctness + TOML type strictness.

use monarch_mcp::client::Cashflow;
use monarch_mcp::goals::{DebtPayoffGoal, EmergencyFundGoal, Goals, SavingsRateGoal};
use monarch_mcp::progress_vs_goals::compute_progress;

fn cf() -> Cashflow {
    Cashflow {
        income: 10000.0,
        spending: 8000.0,
        prior_month_spending: 0.0,
    }
}

// Guidance must appear ONLY when ALL goals are absent. With savings_rate set,
// guidance must be absent even though emergency_fund & debt_payoff are unset.
#[test]
fn guidance_absent_when_savings_rate_set_others_absent() {
    let goals = Goals {
        savings_rate: Some(SavingsRateGoal {
            target_percent: 20.0,
        }),
        emergency_fund: None,
        debt_payoff: None,
    };
    let result = compute_progress(&goals, &[], &cf(), &[], "2026-04", "2026-05", (2026, 5));
    assert!(result.guidance.is_none());
}

// Mixed: savings_rate + debt_payoff set. guidance still absent.
#[test]
fn guidance_absent_when_savings_rate_and_debt_payoff_set() {
    let goals = Goals {
        savings_rate: Some(SavingsRateGoal {
            target_percent: 20.0,
        }),
        emergency_fund: None,
        debt_payoff: Some(DebtPayoffGoal {
            target_date: "2027-12-01".to_string(),
            monthly_payment: None,
        }),
    };
    let result = compute_progress(&goals, &[], &cf(), &[], "2026-04", "2026-05", (2026, 5));
    assert!(result.guidance.is_none());
    // savings_rate is reported; user gets actionable content.
    assert!(result.savings_rate.is_some());
}

// All three set -> guidance absent.
#[test]
fn guidance_absent_when_all_goals_set() {
    let goals = Goals {
        savings_rate: Some(SavingsRateGoal {
            target_percent: 20.0,
        }),
        emergency_fund: Some(EmergencyFundGoal { target_months: 6.0 }),
        debt_payoff: Some(DebtPayoffGoal {
            target_date: "2027-12-01".to_string(),
            monthly_payment: Some(500.0),
        }),
    };
    let result = compute_progress(&goals, &[], &cf(), &[], "2026-04", "2026-05", (2026, 5));
    assert!(result.guidance.is_none());
}

// TOML with the WRONG TYPE for a known field must error, not silently default.
#[test]
fn wrong_typed_toml_field_is_a_goals_file_error() {
    use monarch_mcp::error::MonarchError;
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    // target_percent should be a float; give it a string.
    write!(f, "[savings_rate]\ntarget_percent = \"twenty\"\n").unwrap();
    let result = Goals::load_from_path(f.path());
    assert!(
        matches!(result, Err(MonarchError::GoalsFile(_))),
        "a type-mismatched TOML field must surface as GoalsFile error, got: {result:?}"
    );
}
