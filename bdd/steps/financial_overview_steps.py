"""Step definitions for financial_overview.feature (@ISSUE-A4)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household holds {amount:d} dollars across asset accounts")
def step_asset_accounts(context, amount: int):
    context.mock_assets = amount
    # Configure a single checking account representing all assets
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                {"name": "Assets", "type": "checking", "currentBalance": float(amount)},
                *getattr(context, "_liability_accounts", []),
            ]
        },
    )
    context._asset_accounts = [
        {"name": "Assets", "type": "checking", "currentBalance": float(amount)}
    ]


@given("the household owes {amount:d} dollars across liability accounts")
def step_liability_accounts(context, amount: int):
    context.mock_liabilities = amount
    assets = getattr(context, "_asset_accounts", [])
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                *assets,
                {
                    "name": "Liabilities",
                    "type": "credit",
                    "currentBalance": -float(amount),
                },
            ]
        },
    )
    context._liability_accounts = [
        {"name": "Liabilities", "type": "credit", "currentBalance": -float(amount)}
    ]


@given("this month the household received {amount:d} dollars of income")
def step_income(context, amount: int):
    current = getattr(context, "_cashflow", {"income": 0.0, "spending": 0.0})
    current["income"] = float(amount)
    context._cashflow = current
    requests.post(
        f"{context.mock_base}/configure",
        json={"cashflow": current},
    )


@given("this month the household spent {amount:d} dollars")
def step_spending(context, amount: int):
    current = getattr(context, "_cashflow", {"income": 0.0, "spending": 0.0})
    current["spending"] = float(amount)
    context._cashflow = current
    requests.post(
        f"{context.mock_base}/configure",
        json={"cashflow": current},
    )


@given("the household's net worth was {amount:d} dollars last month")
def step_prior_net_worth(context, amount: int):
    requests.post(
        f"{context.mock_base}/configure",
        json={"prior_month_net_worth": float(amount)},
    )
    context.prior_month_net_worth = float(amount)


@given("the household's net worth is {amount:d} dollars this month")
def step_current_net_worth(context, amount: int):
    # Net worth is derived from account balances; set up matching accounts
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                {
                    "name": "Net Worth Account",
                    "type": "checking",
                    "currentBalance": float(amount),
                }
            ]
        },
    )
    context.current_net_worth = float(amount)


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests a financial overview")
def step_request_overview(context):
    context.overview_result = call_tool(context, "financial_overview")


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("the overview reports a net worth of {amount:d} dollars")
def step_assert_net_worth(context, amount: int):
    result = context.overview_result
    actual = result.get("net_worth")
    assert actual == float(amount), (
        f"Expected net worth {amount}, got {actual!r}. Full result: {result}"
    )


@then("the overview reports income of {amount:d} dollars")
def step_assert_income(context, amount: int):
    result = context.overview_result
    actual = result.get("cashflow", {}).get("income")
    assert actual == float(amount), (
        f"Expected income {amount}, got {actual!r}. Full result: {result}"
    )


@then("the overview reports spending of {amount:d} dollars")
def step_assert_spending(context, amount: int):
    result = context.overview_result
    actual = result.get("cashflow", {}).get("spending")
    assert actual == float(amount), (
        f"Expected spending {amount}, got {actual!r}. Full result: {result}"
    )


@then("the overview reports net cash flow of {amount:d} dollars")
def step_assert_net_cashflow(context, amount: int):
    result = context.overview_result
    actual = result.get("cashflow", {}).get("net")
    assert actual == float(amount), (
        f"Expected net cash flow {amount}, got {actual!r}. Full result: {result}"
    )


@then("the overview reports a net-worth change of positive {amount:d} dollars")
def step_assert_net_worth_change(context, amount: int):
    result = context.overview_result
    actual = result.get("net_worth_change")
    assert actual == float(amount), (
        f"Expected net-worth change +{amount}, got {actual!r}. Full result: {result}"
    )
