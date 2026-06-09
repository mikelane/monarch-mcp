"""Step definitions for spending_history.feature (@ISSUE-54)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given -- configure mock fixtures
# ---------------------------------------------------------------------------


def _add_expense_transaction(context, amount: float, category: str, month: str) -> None:
    """Append a negative-amount expense transaction for the given YYYY-MM month."""
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": f"{category} merchant",
            # Monarch sign convention: expense outflows are negative amounts.
            "amount": -float(amount),
            "category": category,
            "group_type": "expense",
            "date": f"{month}-15",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given("the household spent {amount:d} dollars on {category} in {month}")
def step_expense_in_month(context, amount: int, category: str, month: str) -> None:
    _add_expense_transaction(context, float(amount), category, month)


@given("the household received {amount:d} dollars of {category} income in {month}")
def step_income_in_month(context, amount: int, category: str, month: str) -> None:
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": category,
            "amount": float(amount),
            "category": category,
            "group_type": "income",
            "date": f"{month}-01",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given(
    "the household made a {amount:d} dollar {category} transfer in {month}"
)
def step_transfer_in_month(context, amount: int, category: str, month: str) -> None:
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": category,
            "amount": -float(amount),
            "category": category,
            "group_type": "transfer",
            "date": f"{month}-05",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given("the household has no transactions in the history range")
def step_no_transactions_in_range(context) -> None:
    context._transactions = []
    requests.post(f"{context.mock_base}/configure", json={"transactions": []})


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor generates spending history for {start} to {end}")
def step_generate_spending_history(context, start: str, end: str) -> None:
    context.history_result = call_tool(
        context,
        "spending_history",
        {"start_date": start, "end_date": end},
    )


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


def _get_month(context, month_label: str) -> dict:
    """Return the MonthlySpend entry for the given YYYY-MM label."""
    result = context.history_result
    months = result.get("months", [])
    for m in months:
        if m.get("month") == month_label:
            return m
    labels = [m.get("month") for m in months]
    raise AssertionError(
        f"Month {month_label!r} not found in history result. "
        f"Available months: {labels}. Full result: {result}"
    )


@then("the history contains {count:d} monthly entries")
def step_assert_month_count(context, count: int) -> None:
    result = context.history_result
    months = result.get("months", [])
    assert len(months) == count, (
        f"Expected {count} monthly entries, got {len(months)}. "
        f"Full result: {result}"
    )


@then("the month {month} shows total spending of {amount:d} dollars")
def step_assert_month_total(context, month: str, amount: int) -> None:
    m = _get_month(context, month)
    actual = m.get("total_true_spending")
    assert actual == float(amount), (
        f"Expected month {month!r} total_true_spending={amount}, got {actual!r}. "
        f"Month data: {m}"
    )


@then("the month {month} fixed spending is {amount:d} dollars")
def step_assert_month_fixed(context, month: str, amount: int) -> None:
    m = _get_month(context, month)
    split = m.get("split", {})
    actual = split.get("fixed")
    assert actual == float(amount), (
        f"Expected month {month!r} fixed={amount}, got {actual!r}. Split: {split}"
    )


@then("the month {month} discretionary spending is {amount:d} dollars")
def step_assert_month_discretionary(context, month: str, amount: int) -> None:
    m = _get_month(context, month)
    split = m.get("split", {})
    actual = split.get("discretionary")
    assert actual == float(amount), (
        f"Expected month {month!r} discretionary={amount}, got {actual!r}. Split: {split}"
    )


@then("the month {month} total spending equals fixed plus discretionary")
def step_assert_split_sum_equals_total(context, month: str) -> None:
    m = _get_month(context, month)
    total = m.get("total_true_spending", 0.0)
    split = m.get("split", {})
    fixed = split.get("fixed", 0.0)
    disc = split.get("discretionary", 0.0)
    assert abs((fixed + disc) - total) < 0.01, (
        f"Month {month!r}: fixed ({fixed}) + discretionary ({disc}) = {fixed + disc} "
        f"!= total_true_spending ({total}). Month data: {m}"
    )


@then("the response does not contain a raw transaction list")
def step_assert_no_raw_transactions(context) -> None:
    result = context.history_result
    assert "transactions" not in result, (
        f"Response must not contain a raw 'transactions' key. "
        f"Keys present: {list(result.keys())}"
    )
    for m in result.get("months", []):
        assert "transactions" not in m, (
            f"Monthly entry {m.get('month')!r} must not contain a 'transactions' key. "
            f"Keys present: {list(m.keys())}"
        )


@then("the month {month} by-category shows {category} at {amount:d} dollars")
def step_assert_by_category(context, month: str, category: str, amount: int) -> None:
    m = _get_month(context, month)
    by_category = m.get("by_category", {})
    actual = by_category.get(category)
    assert actual == float(amount), (
        f"Expected month {month!r} by_category[{category!r}]={amount}, "
        f"got {actual!r}. by_category: {by_category}"
    )
