//! Adversarial QA (Gate 3) for issue-22 — graceful missing-goals handling.
//!
//! These tests exercise the PUBLIC API (`monarch_mcp::goals` and
//! `monarch_mcp::progress_vs_goals`) as a black box. They were written by the
//! adversarial-qa agent to PROVE bugs found while attacking the issue-22 change.
//! A failing test here is a confirmed finding, not speculation.
//!
//! Updated in issue #27 (ADR 0007): a debt-only config now yields a populated
//! `debt_payoff` field rather than a guidance message — the original BUG 1
//! finding is resolved. Tests updated to assert the new correct behaviour.

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
// RESOLVED (issue #27): debt_payoff-only config now yields a populated
// debt_payoff field and no guidance — no phantom goals, no silent empty payload.
// ---------------------------------------------------------------------------

#[test]
fn debt_payoff_only_config_yields_debt_payoff_and_no_phantom_goals() {
    let goals = debt_payoff_only_goals();
    let result = compute_progress(
        &goals,
        &[],
        &cashflow(10000.0, 8000.0),
        &[],
        "2026-04",
        "2026-05",
        (2026, 5),
    );
    assert!(
        result.savings_rate.is_none(),
        "savings_rate must be absent — user never set it: {result:?}"
    );
    assert!(
        result.emergency_fund.is_none(),
        "emergency_fund must be absent — user never set it: {result:?}"
    );
    assert!(
        result.guidance.is_none(),
        "guidance must be absent when debt_payoff goal is configured: {result:?}"
    );
    assert!(
        result.debt_payoff.is_some(),
        "debt_payoff must be Some when the goal is configured: {result:?}"
    );
}

#[test]
fn debt_payoff_only_config_serializes_debt_payoff_field() {
    // The user-facing JSON must contain a debt_payoff object, not a guidance
    // message and not "{}" (silent empty).
    let goals = debt_payoff_only_goals();
    let result = compute_progress(
        &goals,
        &[],
        &cashflow(10000.0, 8000.0),
        &[],
        "2026-04",
        "2026-05",
        (2026, 5),
    );
    let json: serde_json::Value = serde_json::to_value(&result).unwrap();
    let obj = json.as_object().expect("payload must be a JSON object");
    assert!(
        !obj.is_empty(),
        "debt_payoff-only config serialized to an empty object"
    );
    assert!(
        obj.contains_key("debt_payoff"),
        "debt_payoff-only config must serialize a debt_payoff field, got: {json}"
    );
    assert!(
        !obj.contains_key("guidance"),
        "debt_payoff-only config must NOT have a guidance field, got: {json}"
    );
}
