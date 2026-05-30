"""Step definitions for progress_vs_goals.feature (@ISSUE-A7)."""

from __future__ import annotations

import os

import requests
import tomli_w
from behave import given, then, when

from steps.common import call_tool


def _write_goals(context, goals: dict) -> None:
    """Write goals to the scenario's temp TOML file and update env."""
    goals_file = context.goals_file
    with open(goals_file, "wb") as fh:
        tomli_w.dump(goals, fh)


# ---------------------------------------------------------------------------
# Given — configure goals file and mock fixtures
# ---------------------------------------------------------------------------


@given("the household's savings-rate goal is {pct:d} percent")
def step_savings_rate_goal(context, pct: int):
    goals = getattr(context, "_goals", {})
    goals.setdefault("savings_rate", {})["target_percent"] = pct
    context._goals = goals
    _write_goals(context, goals)


@given("the household's actual savings rate is {pct:d} percent")
def step_actual_savings_rate(context, pct: int):
    # Model as cashflow: income=100, savings=pct so rate=pct%
    income = 10000.0
    spending = income * (1 - pct / 100.0)
    requests.post(
        f"{context.mock_base}/configure",
        json={"cashflow": {"income": income, "spending": spending}},
    )
    context.actual_savings_rate = pct


@given("the household's emergency-fund goal is {months:d} months of expenses")
def step_emergency_fund_goal(context, months: int):
    goals = getattr(context, "_goals", {})
    goals.setdefault("emergency_fund", {})["target_months"] = months
    context._goals = goals
    _write_goals(context, goals)


@given("the household's cash reserves cover {months:d} months of expenses")
def step_cash_reserves(context, months: int):
    # Monthly expenses = 5000; reserves = months * 5000
    monthly_expenses = 5000.0
    reserves = months * monthly_expenses
    # Model reserves as a savings account balance
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                {
                    "name": "Emergency Fund",
                    "type": "savings",
                    "currentBalance": reserves,
                }
            ],
            "cashflow": {"income": monthly_expenses * 1.2, "spending": monthly_expenses},
        },
    )
    context.actual_reserve_months = months


@given("the household has not set a debt-payoff goal")
def step_no_debt_payoff_goal(context):
    goals = getattr(context, "_goals", {})
    # Explicitly ensure no debt_payoff key exists
    goals.pop("debt_payoff", None)
    context._goals = goals
    _write_goals(context, goals)


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor reviews progress versus goals")
def step_review_progress(context):
    context.progress_result = call_tool(context, "progress_vs_goals")


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("the savings-rate goal is reported as {status}")
def step_assert_savings_rate_status(context, status: str):
    result = context.progress_result
    actual_status = result.get("savings_rate", {}).get("status")
    assert actual_status == status, (
        f"Expected savings-rate status {status!r}, got {actual_status!r}. "
        f"Full result: {result}"
    )


@then("the emergency-fund goal is reported as {status}")
def step_assert_emergency_fund_status(context, status: str):
    result = context.progress_result
    actual_status = result.get("emergency_fund", {}).get("status")
    assert actual_status == status, (
        f"Expected emergency-fund status {status!r}, got {actual_status!r}. "
        f"Full result: {result}"
    )


@then("no debt-payoff progress is reported")
def step_assert_no_debt_payoff(context):
    result = context.progress_result
    assert "debt_payoff" not in result, (
        f"Expected no debt_payoff key in result, got: {result!r}"
    )
