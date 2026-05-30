"""Step definitions for cashflow_forecast.feature (@ISSUE-B1)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household has a checking account with a balance of {amount:d} dollars")
def step_checking_balance(context, amount: int):
    context._checking_balance = float(amount)
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                {
                    "name": "Checking",
                    "type": "depository",
                    "currentBalance": float(amount),
                }
            ]
        },
    )


@given("this month {amount:d} dollars of income has already been received")
def step_income_received(context, amount: int):
    current = getattr(context, "_cashflow_b1", {"income": 0.0, "spending": 0.0})
    current["income"] = float(amount)
    context._cashflow_b1 = current
    requests.post(f"{context.mock_base}/configure", json={"cashflow": current})


@given("the following recurring bills are still due this month:")
def step_upcoming_bills(context):
    """Table: merchant | amount"""
    items = []
    for row in context.table:
        items.append(
            {
                "merchant": row["merchant"],
                "stream_amount": float(row["amount"]),
                "actual_amount": float(row["amount"]),
                "frequency": "monthly",
                "is_approximate": False,
                "is_past": False,
            }
        )
    context._upcoming_items = items
    _post_recurring(context)


@given("the following recurring bills were already paid this month:")
def step_past_bills(context):
    """Table: merchant | amount"""
    items = []
    for row in context.table:
        items.append(
            {
                "merchant": row["merchant"],
                "stream_amount": float(row["amount"]),
                "actual_amount": float(row["amount"]),
                "frequency": "monthly",
                "is_approximate": False,
                "is_past": True,
            }
        )
    context._past_items = items
    _post_recurring(context)


@given("there are no recurring bills due this month")
def step_no_bills(context):
    context._upcoming_items = []
    context._past_items = []
    requests.post(f"{context.mock_base}/configure", json={"recurring_items": []})


def _post_recurring(context):
    """Merge past and upcoming items and post to mock."""
    past = getattr(context, "_past_items", [])
    upcoming = getattr(context, "_upcoming_items", [])
    requests.post(
        f"{context.mock_base}/configure",
        json={"recurring_items": past + upcoming},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor runs a cash flow forecast")
def step_run_forecast(context):
    context.forecast_result = call_tool(context, "cashflow_forecast")


# ---------------------------------------------------------------------------
# Then — assertions
# ---------------------------------------------------------------------------


@then("the forecast shows a projected month-end balance of {amount:d} dollars")
def step_assert_projected_balance(context, amount: int):
    result = context.forecast_result
    projected = result.get("projected_month_end_balance")
    assert projected is not None, f"No projected_month_end_balance in result: {result!r}"
    assert abs(projected - float(amount)) < 0.01, (
        f"Expected projected month-end balance {amount}, got {projected}"
    )


@then("the forecast does not flag a shortfall")
def step_assert_no_shortfall(context):
    result = context.forecast_result
    assert not result.get("shortfall"), (
        f"Expected no shortfall flag but got: {result!r}"
    )


@then("the forecast flags a projected shortfall of {amount:d} dollars")
def step_assert_shortfall(context, amount: int):
    result = context.forecast_result
    shortfall = result.get("shortfall")
    assert shortfall, f"Expected a shortfall flag but got: {result!r}"
    shortfall_amount = result.get("shortfall_amount", 0.0)
    assert abs(shortfall_amount - float(amount)) < 0.01, (
        f"Expected shortfall of {amount}, got {shortfall_amount}"
    )


@then("the forecast names the bills driving the shortfall")
def step_assert_shortfall_bills(context):
    result = context.forecast_result
    bills = result.get("shortfall_drivers", [])
    assert bills, f"Expected shortfall_drivers list but got: {result!r}"
