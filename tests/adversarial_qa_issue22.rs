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
fn debt_payoff_only_config_yields_guidance_and_no_phantom_goals() {
    let goals = debt_payoff_only_goals();
    let result = compute_progress(&goals, &[], &cashflow(10000.0, 8000.0));
    // The only acceptable content for a debt-only config is guidance: the user
    // set no savings_rate or emergency_fund goal, so those must stay absent
    // (anything else would be a phantom goal the user never configured).
    assert!(
        result.savings_rate.is_none(),
        "savings_rate must be absent — user never set it: {result:?}"
    );
    assert!(
        result.emergency_fund.is_none(),
        "emergency_fund must be absent — user never set it: {result:?}"
    );
    let guidance = result.guidance.as_deref().unwrap_or("");
    assert!(
        guidance.contains("debt") || guidance.contains("#27"),
        "debt-only config must yield guidance referencing debt-payoff (or #27), \
         not a silent empty payload: {result:?}"
    );
}

#[test]
fn debt_payoff_only_config_serializes_only_a_guidance_field() {
    // The user-facing JSON. Every field has skip_serializing_if = Option::is_none,
    // so a debt_payoff-only config must serialize to exactly a guidance field —
    // not "{}" (silent empty), and not a stray savings_rate/emergency_fund the
    // user never set. Parse the JSON and assert the precise shape.
    let goals = debt_payoff_only_goals();
    let result = compute_progress(&goals, &[], &cashflow(10000.0, 8000.0));
    let json: serde_json::Value = serde_json::to_value(&result).unwrap();

    let obj = json.as_object().expect("payload must be a JSON object");
    assert!(
        !obj.is_empty(),
        "debt_payoff-only config serialized to an empty object — \
         the agent/user sees nothing about their configured goal"
    );
    assert_eq!(
        obj.keys().collect::<Vec<_>>(),
        vec!["guidance"],
        "debt_payoff-only config must serialize to exactly a guidance field, got: {json}"
    );
    let guidance = obj["guidance"].as_str().expect("guidance must be a string");
    assert!(
        guidance.contains("debt") || guidance.contains("#27"),
        "guidance for a debt-only config must reference debt-payoff (or #27), got: {guidance:?}"
    );
}
