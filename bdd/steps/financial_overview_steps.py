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
    context._asset_accounts = [
        {"name": "Assets", "type": "checking", "currentBalance": float(amount)}
    ]
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                *context._asset_accounts,
                *getattr(context, "_liability_accounts", []),
            ]
        },
    )


@given("the household owes {amount:d} dollars across liability accounts")
def step_liability_accounts(context, amount: int):
    context.mock_liabilities = amount
    context._liability_accounts = [
        {"name": "Liabilities", "type": "credit", "currentBalance": -float(amount)}
    ]
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                *getattr(context, "_asset_accounts", []),
                *context._liability_accounts,
            ]
        },
    )


@given("this month the household received {amount:d} dollars of income")
def step_income(context, amount: int):
    current = getattr(context, "_cashflow", {"income": 0.0, "spending": 0.0})
    current["income"] = float(amount)
    context._cashflow = current
    requests.post(f"{context.mock_base}/configure", json={"cashflow": current})


@given("this month the household spent {amount:d} dollars")
def step_spending(context, amount: int):
    current = getattr(context, "_cashflow", {"income": 0.0, "spending": 0.0})
    current["spending"] = float(amount)
    context._cashflow = current
    requests.post(f"{context.mock_base}/configure", json={"cashflow": current})
    # financial_overview now computes spending from transactions (compute_true_spending),
    # not from the cashflow aggregate. Configure a matching expense transaction so the
    # mock returns the right spending figure when GetTransactionsList is called.
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": "Household spending",
            "amount": -float(amount),
            "category": "General",
            "group_type": "expense",
            "date": "2026-05-15",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given("the household's net worth was {amount:d} dollars last month")
def step_prior_net_worth(context, amount: int):
    requests.post(
        f"{context.mock_base}/configure",
        json={"prior_month_net_worth": float(amount)},
    )
    context.prior_month_net_worth = float(amount)


@given("the household's net worth is {amount:d} dollars this month")
def step_current_net_worth(context, amount: int):
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


@given("the household has no accounts")
def step_no_accounts(context):
    """Configure an empty account list — the tool must report zeros, not error."""
    requests.post(f"{context.mock_base}/configure", json={"accounts": []})


@given("the household has accounts including one with a null balance")
def step_accounts_with_null_balance(context):
    """Configure accounts where one has currentBalance=null (unsynced/manual account).

    Verifies Bug B2 fix: a null balance must not fail the entire accounts parse.
    The synced account (5000) still contributes to net worth; the null-balance
    account is treated as 0 and not dropped from the list.
    """
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "accounts": [
                {"name": "Checking", "type": "checking", "currentBalance": 5000.0},
                {"name": "Unsynced Account", "type": "savings", "currentBalance": None},
            ]
        },
    )


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


@then("the overview reports a net worth of negative {amount:d} dollars")
def step_assert_negative_net_worth(context, amount: int):
    result = context.overview_result
    actual = result.get("net_worth")
    expected = -float(amount)
    assert actual == expected, (
        f"Expected net worth {expected}, got {actual!r}. Full result: {result}"
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


@then("the accounts still load and the overview reports a net worth of {amount:d} dollars")
def step_assert_accounts_load_with_null_balance(context, amount: int):
    """Assert overview succeeded (null balance did not kill the parse) and net worth is correct."""
    result = context.overview_result
    assert result is not None, "overview must succeed even with a null-balance account"
    assert "error" not in result, f"overview must not return an error; got: {result}"
    actual = result.get("net_worth")
    assert actual == float(amount), (
        f"Expected net worth {amount} (null balance = 0, synced = 5000), got {actual!r}. "
        f"Full result: {result}"
    )
