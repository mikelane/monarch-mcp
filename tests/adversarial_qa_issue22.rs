//! Adversarial QA (Gate 3) for issue-22 — graceful missing-goals handling.
//!
//! These tests exercise the PUBLIC API (`monarch_mcp::goals` and
//! `monarch_mcp::progress_vs_goals`) as a black box. They were written by the
//! adversarial-qa agent to PROVE bugs found while attacking the issue-22 change.
//! A failing test here is a confirmed finding, not speculation.

use monarch_mcp::client::Cashflow;
use monarch_mcp::goals::{DebtPayoffGoal, Goals};
use monarch_mcp::progress_vs_goals::compute_progress;

fn cashflow(income: f64, spending: f64) -> Cashflow {
    Cashflow {
        income,
        spending,
        prior_month_spending: 0.0,
    }
}

fn debt_payoff_only_goals() -> Goals {
    Goals {
        savings_rate: None,
        emergency_fund: None,
        debt_payoff: Some(DebtPayoffGoal {
            target_date: "2027-12-01".to_string(),
            monthly_payment: Some(500.0),
        }),
    }
}

// ---------------------------------------------------------------------------
// BUG 1: debt_payoff-only config yields the silent empty payload the change
//        was explicitly meant to eliminate.
//
// compute_progress treats `debt_payoff.is_some()` as "goals are configured", so
// `no_goals_configured` is false and NO guidance is emitted. But the output type
// has no debt_payoff field, so savings_rate=None, emergency_fund=None,
// guidance=None. The user who configured a debt-payoff goal gets nothing.
// ---------------------------------------------------------------------------

#[test]
fn debt_payoff_only_config_is_not_a_silent_empty_payload() {
    let goals = debt_payoff_only_goals();
    let result = compute_progress(&goals, &[], &cashflow(10000.0, 8000.0));
    let has_any_content = result.savings_rate.is_some()
        || result.emergency_fund.is_some()
        || result.guidance.is_some();
    assert!(
        has_any_content,
        "debt_payoff-only config yields a silent empty payload \
         (no savings_rate, no emergency_fund, no guidance): {result:?}"
    );
}

#[test]
fn debt_payoff_only_config_does_not_serialize_to_empty_object() {
    // The user-facing JSON. Every field has skip_serializing_if = Option::is_none,
    // so a debt_payoff-only config serializes to literally "{}".
    let goals = debt_payoff_only_goals();
    let result = compute_progress(&goals, &[], &cashflow(10000.0, 8000.0));
    let json = serde_json::to_string(&result).unwrap();
    assert_ne!(
        json, "{}",
        "debt_payoff-only config serializes to an empty JSON object — \
         the agent/user sees nothing about their configured goal"
    );
}
