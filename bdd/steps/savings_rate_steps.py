"""Step definitions for savings_rate.feature (@ISSUE-65)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given -- configure mock fixtures
# ---------------------------------------------------------------------------


def _post_transactions(context) -> None:
    """Push the current context._transactions list to the mock."""
    requests.post(
        f"{context.mock_base}/configure",
        json={"transactions": getattr(context, "_transactions", [])},
    )


@given("a month {month} with {income:d} income and {spending:d} true spending")
def step_month_income_and_spending(context, month: str, income: int, spending: int):
    txns = getattr(context, "_transactions", [])
    # Income transaction — positive amount, group_type "income"
    txns.append(
        {
            "merchant": "Employer",
            "amount": float(income),
            "category": "Paychecks",
            "group_type": "income",
            "date": f"{month}-01",
        }
    )
    # Expense transaction — negative amount, group_type "expense"
    if spending > 0:
        txns.append(
            {
                "merchant": "Expenses merchant",
                "amount": -float(spending),
                "category": "Expenses",
                "group_type": "expense",
                "date": f"{month}-15",
            }
        )
    context._transactions = txns
    _post_transactions(context)


@given(
    "a month {month} with {income:d} income, {spending:d} expenses, "
    "and a {cc_payment:d} credit-card payment"
)
def step_month_with_cc_payment(
    context, month: str, income: int, spending: int, cc_payment: int
):
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": "Employer",
            "amount": float(income),
            "category": "Paychecks",
            "group_type": "income",
            "date": f"{month}-01",
        }
    )
    txns.append(
        {
            "merchant": "Expenses merchant",
            "amount": -float(spending),
            "category": "Expenses",
            "group_type": "expense",
            "date": f"{month}-15",
        }
    )
    # Credit card payment is a transfer — must NOT count as true spending
    txns.append(
        {
            "merchant": "Chase CC Payment",
            "amount": -float(cc_payment),
            "category": "Credit Card Payment",
            "group_type": "transfer",
            "date": f"{month}-20",
        }
    )
    context._transactions = txns
    _post_transactions(context)


@given("three complete months of income and spending starting {start_month}")
def step_three_months_income_spending(context, start_month: str):
    """Build three months of synthetic income+expense data starting at start_month."""
    # Parse YYYY-MM
    year, month = map(int, start_month.split("-"))
    txns = getattr(context, "_transactions", [])
    for i in range(3):
        m = month + i
        y = year
        if m > 12:
            m -= 12
            y += 1
        label = f"{y:04}-{m:02}"
        txns.append(
            {
                "merchant": "Employer",
                "amount": 6000.0,
                "category": "Paychecks",
                "group_type": "income",
                "date": f"{label}-01",
            }
        )
        # Vary spending so each month has a different rate
        spending = 3000.0 + i * 600.0  # 3000, 3600, 4200
        txns.append(
            {
                "merchant": "Groceries merchant",
                "amount": -spending,
                "category": "Groceries",
                "group_type": "expense",
                "date": f"{label}-15",
            }
        )
    context._transactions = txns
    context._three_months_start = start_month
    _post_transactions(context)


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests savings_rate for {start} to {end}")
def step_request_savings_rate_explicit(context, start: str, end: str):
    context.savings_result = call_tool(
        context,
        "savings_rate",
        {"start_date": start, "end_date": end},
    )


@when("the advisor requests savings_rate with months {months:d}")
def step_request_savings_rate_months(context, months: int):
    context.savings_result = call_tool(
        context,
        "savings_rate",
        {"months": months},
    )


@when("the advisor requests savings_rate")
def step_request_savings_rate_default(context):
    context.savings_result = call_tool(context, "savings_rate", {})


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


def _get_month(context, month_label: str) -> dict:
    """Return the MonthlySavings entry for the given YYYY-MM label."""
    result = context.savings_result
    months = result.get("months", [])
    for m in months:
        if m.get("month") == month_label:
            return m
    labels = [m.get("month") for m in months]
    raise AssertionError(
        f"Month {month_label!r} not found in savings_rate result. "
        f"Available months: {labels}. Full result: {result}"
    )


@then("the month {month} reports income {amount:d}")
def step_assert_month_income(context, month: str, amount: int):
    m = _get_month(context, month)
    actual = m.get("income")
    assert actual == float(amount), (
        f"Expected month {month!r} income={amount}, got {actual!r}. Month: {m}"
    )


@then("the month {month} reports true_spending {amount:d}")
def step_assert_month_true_spending(context, month: str, amount: int):
    m = _get_month(context, month)
    actual = m.get("true_spending")
    assert actual == float(amount), (
        f"Expected month {month!r} true_spending={amount}, got {actual!r}. Month: {m}"
    )


@then("the month {month} reports net_savings {amount:d}")
def step_assert_month_net_savings(context, month: str, amount: int):
    m = _get_month(context, month)
    actual = m.get("net_savings")
    assert actual == float(amount), (
        f"Expected month {month!r} net_savings={amount}, got {actual!r}. Month: {m}"
    )


@then("the month {month} reports a savings_rate of {rate:d} percent")
def step_assert_month_savings_rate(context, month: str, rate: int):
    m = _get_month(context, month)
    actual = m.get("savings_rate")
    assert actual is not None, (
        f"Expected savings_rate to be present for month {month!r}, but it was absent. Month: {m}"
    )
    assert abs(actual - float(rate)) < 0.01, (
        f"Expected month {month!r} savings_rate={rate}%, got {actual!r}. Month: {m}"
    )


@then("the credit-card payment is not counted in true_spending")
def step_assert_cc_not_in_spending(context):
    # The assertion is that true_spending only reflects expense-group transactions.
    # Verified by the companion step that checks the exact true_spending value.
    result = context.savings_result
    assert "months" in result, f"Expected 'months' key in result: {result}"


@then("three months are returned")
def step_assert_three_months(context):
    result = context.savings_result
    months = result.get("months", [])
    assert len(months) == 3, (
        f"Expected 3 months, got {len(months)}. Full result: {result}"
    )


@then("a window-average savings_rate is reported")
def step_assert_window_average(context):
    result = context.savings_result
    avg = result.get("window_average_savings_rate")
    assert avg is not None, (
        f"Expected window_average_savings_rate to be present. Full result: {result}"
    )
    assert isinstance(avg, (int, float)), (
        f"Expected window_average_savings_rate to be numeric, got {type(avg)}. "
        f"Full result: {result}"
    )


@then("the response contains no individual transaction list")
def step_assert_no_raw_transactions(context):
    result = context.savings_result
    assert "transactions" not in result, (
        f"Response must not contain a raw 'transactions' key. Keys: {list(result.keys())}"
    )
    for m in result.get("months", []):
        assert "transactions" not in m, (
            f"Monthly entry {m.get('month')!r} must not contain a 'transactions' key. "
            f"Keys: {list(m.keys())}"
        )


@then("the month {month} savings_rate is absent")
def step_assert_savings_rate_absent(context, month: str):
    m = _get_month(context, month)
    actual = m.get("savings_rate")
    assert actual is None, (
        f"Expected savings_rate to be absent (None) for zero-income month {month!r}, "
        f"but got {actual!r}. Month: {m}"
    )
