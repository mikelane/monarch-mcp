"""Step definitions for debt_payoff.feature (@ISSUE-27)."""

from __future__ import annotations

import requests
import tomli_w
from behave import given, then


def _write_goals(context, goals: dict) -> None:
    """Write goals to the scenario's temp TOML file."""
    goals_file = context.goals_file
    with open(goals_file, "wb") as fh:
        tomli_w.dump(goals, fh)


# ---------------------------------------------------------------------------
# Given — configure goals file and mock fixtures
# ---------------------------------------------------------------------------


@given('the household\'s debt-payoff goal targets "{target_date}" with {payment:d} per month')
def step_debt_goal_with_payment(context, target_date: str, payment: int):
    goals = getattr(context, "_goals", {})
    goals["debt_payoff"] = {
        "target_date": target_date,
        "monthly_payment": float(payment),
    }
    context._goals = goals
    _write_goals(context, goals)


@given('the household\'s debt-payoff goal targets "{target_date}" with no monthly payment')
def step_debt_goal_no_payment(context, target_date: str):
    goals = getattr(context, "_goals", {})
    goals["debt_payoff"] = {"target_date": target_date}
    context._goals = goals
    _write_goals(context, goals)


@given("the household has a credit account with a balance of {balance:d} owed")
def step_credit_account(context, balance: int):
    """Configure a single credit account whose Monarch balance is ``-balance``.

    Monarch represents owed amounts as negative balances, so a balance of
    ``balance`` owed is stored as ``-balance``. The scenario's debt accounts
    are replaced wholesale, so each scenario starts from a clean slate.
    """
    accounts = [
        {
            "name": "Credit Card 1",
            "type": "credit",
            "currentBalance": -float(balance),
        }
    ]
    context._debt_accounts = accounts
    requests.post(
        f"{context.mock_base}/configure",
        json={"accounts": accounts},
    )


@given("the household has no debt accounts")
def step_no_debt_accounts(context):
    context._debt_accounts = []
    requests.post(
        f"{context.mock_base}/configure",
        json={"accounts": []},
    )


@given("the household paid down {amount:d} in debt last month")
def step_paid_down_debt(context, amount: int):
    """Configure prior-month and current-month debt snapshots.

    MONARCH_NOW = 2026-05-15, so:
      current_month = "2026-05"
      prior_month   = "2026-04"

    We derive current_owed from the already-configured debt accounts.
    prior_owed = current_owed + amount (household paid down `amount`).
    """
    current_owed = sum(
        -a["currentBalance"]
        for a in getattr(context, "_debt_accounts", [])
        if a["currentBalance"] < 0
    )
    prior_owed = current_owed + float(amount)

    snapshots = [
        {"account_type": "credit", "month": "2026-04", "balance": -prior_owed},
        {"account_type": "credit", "month": "2026-05", "balance": -current_owed},
    ]
    requests.post(
        f"{context.mock_base}/configure",
        json={"snapshots_by_type": snapshots},
    )


# ---------------------------------------------------------------------------
# Then — debt_payoff assertions
# ---------------------------------------------------------------------------


@then('the debt-payoff progress is reported as "{status}"')
def step_assert_debt_payoff_status(context, status: str):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("status")
    assert actual == status, (
        f"Expected debt_payoff status {status!r}, got {actual!r}. "
        f"Full debt_payoff: {dp}"
    )


@then("the debt-payoff total_debt is {expected:g}")
def step_assert_total_debt(context, expected: float):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("total_debt")
    assert abs(actual - expected) < 0.01, (
        f"Expected total_debt {expected}, got {actual}. Full: {dp}"
    )


@then("the months_to_target is {expected:d}")
def step_assert_months_to_target(context, expected: int):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("months_to_target")
    assert actual == expected, (
        f"Expected months_to_target {expected}, got {actual}. Full: {dp}"
    )


@then("the required_monthly is approximately {expected:g}")
def step_assert_required_monthly(context, expected: float):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("required_monthly")
    assert actual is not None, f"Expected required_monthly to be set, got None. Full: {dp}"
    assert abs(actual - expected) < 1.0, (
        f"Expected required_monthly ≈ {expected}, got {actual}. Full: {dp}"
    )


@then('the on_schedule signal is "{expected}"')
def step_assert_on_schedule(context, expected: str):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("on_schedule")
    assert actual == expected, (
        f"Expected on_schedule {expected!r}, got {actual!r}. Full: {dp}"
    )


@then('the on_pace signal is "{expected}"')
def step_assert_on_pace(context, expected: str):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    actual = dp.get("on_pace")
    assert actual == expected, (
        f"Expected on_pace {expected!r}, got {actual!r}. Full: {dp}"
    )


@then('the response contains a "{field}" field')
def step_assert_field_present(context, field: str):
    result = context.progress_result
    assert field in result, (
        f"Expected field {field!r} in response, got keys: {list(result.keys())}"
    )


@then('the response does not contain a "{field}" field')
def step_assert_field_absent(context, field: str):
    result = context.progress_result
    assert field not in result, (
        f"Expected field {field!r} to be absent, but it was present: {result.get(field)!r}"
    )


@then("no on_schedule signal is reported")
def step_assert_no_on_schedule(context):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    assert "on_schedule" not in dp, (
        f"Expected on_schedule to be absent, got: {dp.get('on_schedule')!r}"
    )


@then("no on_pace signal is reported")
def step_assert_no_on_pace(context):
    result = context.progress_result
    dp = result.get("debt_payoff")
    assert dp is not None, f"Expected debt_payoff in result, got: {result}"
    assert "on_pace" not in dp, (
        f"Expected on_pace to be absent, got: {dp.get('on_pace')!r}"
    )
