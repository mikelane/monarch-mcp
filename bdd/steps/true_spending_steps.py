"""Step definitions for true_spending.feature (@ISSUE-25).

Covers:
- Transfer / Credit Card Payment transactions excluded from spending
- financial_overview and spending_report spending totals agree
- Charge-and-refund reversal pairs surfaced (not netted)
"""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _push_transaction(context, txn: dict) -> None:
    """Append a transaction to the mock and push the full list."""
    txns = getattr(context, "_transactions", [])
    txns.append(txn)
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("a mock Monarch month containing a Transfer transaction of {amount:g}")
def step_transfer_transaction(context, amount: float):
    _push_transaction(
        context,
        {
            "merchant": "Chase CC Payment",
            "amount": amount,
            "category": "Credit Card Payment",
            "group_type": "transfer",
            "date": "2026-05-05",
        },
    )


@given("a mock Monarch month containing a Credit Card Payment transaction of {amount:g}")
def step_credit_card_payment_transaction(context, amount: float):
    _push_transaction(
        context,
        {
            "merchant": "Chase CC Payment",
            "amount": amount,
            "category": "Credit Card Payment",
            "group_type": "transfer",
            "date": "2026-05-05",
        },
    )


@given("an expense transaction in Groceries of {amount:g}")
def step_expense_groceries(context, amount: float):
    _push_transaction(
        context,
        {
            "merchant": "Whole Foods",
            "amount": amount,
            "category": "Groceries",
            "group_type": "expense",
            "date": "2026-05-10",
        },
    )


@given("an expense transaction in Restaurants of {amount:g}")
def step_expense_restaurants(context, amount: float):
    _push_transaction(
        context,
        {
            "merchant": "Chipotle",
            "amount": amount,
            "category": "Restaurants",
            "group_type": "expense",
            "date": "2026-05-12",
        },
    )


@given(
    'a mock Monarch month where merchant "{merchant}" has a charge of {charge:g} and a refund of {refund:g}'
)
def step_charge_and_refund(context, merchant: str, charge: float, refund: float):
    _push_transaction(
        context,
        {
            "merchant": merchant,
            "amount": charge,
            "category": "Pets",
            "group_type": "expense",
            "date": "2026-05-01",
        },
    )
    _push_transaction(
        context,
        {
            "merchant": merchant,
            "amount": refund,
            "category": "Pets",
            "group_type": "expense",
            "date": "2026-05-10",
        },
    )


@given(
    'a mock Monarch month where merchant "{merchant}" has two charges of {amount:g} each'
)
def step_two_charges_no_refund(context, merchant: str, amount: float):
    for day in ("01", "02"):
        _push_transaction(
            context,
            {
                "merchant": merchant,
                "amount": amount,
                "category": "Pets",
                "group_type": "expense",
                "date": f"2026-05-{day}",
            },
        )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("an agent calls spending_report for that month")
def step_call_spending_report(context):
    context.spending_result = call_tool(context, "spending_report")


@when("an agent calls financial_overview for that month")
def step_call_financial_overview(context):
    context.overview_result = call_tool(context, "financial_overview")


@when("an agent also calls financial_overview for that month")
def step_also_call_financial_overview(context):
    context.overview_result = call_tool(context, "financial_overview")


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("total_spent reflects only the Groceries amount")
def step_total_spent_groceries_only(context):
    result = context.spending_result
    txns = getattr(context, "_transactions", [])
    groceries_magnitude = sum(
        abs(t["amount"])
        for t in txns
        if t.get("category") == "Groceries" and t.get("group_type") == "expense"
    )
    actual = result.get("total_spent")
    assert actual == groceries_magnitude, (
        f"Expected total_spent={groceries_magnitude} (Groceries only), "
        f"got {actual!r}. Full result: {result}"
    )


@then("total_spent reflects only the Restaurants amount")
def step_total_spent_restaurants_only(context):
    result = context.spending_result
    txns = getattr(context, "_transactions", [])
    restaurants_magnitude = sum(
        abs(t["amount"])
        for t in txns
        if t.get("category") == "Restaurants" and t.get("group_type") == "expense"
    )
    actual = result.get("total_spent")
    assert actual == restaurants_magnitude, (
        f"Expected total_spent={restaurants_magnitude} (Restaurants only), "
        f"got {actual!r}. Full result: {result}"
    )


@then("the financial overview spending reflects only the Groceries amount")
def step_overview_spending_groceries_only(context):
    result = context.overview_result
    txns = getattr(context, "_transactions", [])
    groceries_magnitude = sum(
        abs(t["amount"])
        for t in txns
        if t.get("category") == "Groceries" and t.get("group_type") == "expense"
    )
    actual = result.get("cashflow", {}).get("spending")
    assert actual == groceries_magnitude, (
        f"Expected financial_overview cashflow.spending={groceries_magnitude} (Groceries only), "
        f"got {actual!r}. Full result: {result}"
    )


@then("the financial overview spending equals the spending report total")
def step_overview_spending_equals_report_total(context):
    overview = context.overview_result
    report = context.spending_result
    overview_spending = overview.get("cashflow", {}).get("spending")
    report_total = report.get("total_spent")
    assert overview_spending == report_total, (
        f"financial_overview.cashflow.spending ({overview_spending!r}) "
        f"!= spending_report.total_spent ({report_total!r}). "
        f"Overview: {overview}. Report: {report}"
    )


@then('the response includes a possible_reversals entry for merchant "{merchant}"')
def step_has_reversal_entry(context, merchant: str):
    result = context.spending_result
    reversals = result.get("possible_reversals", [])
    merchants = [r.get("merchant") for r in reversals]
    assert merchant in merchants, (
        f"Expected '{merchant}' in possible_reversals merchants {merchants}. "
        f"Full result: {result}"
    )
    context.reversal_for_merchant = next(r for r in reversals if r.get("merchant") == merchant)


@then("the possible_reversals entry identifies both the charge and the refund")
def step_reversal_has_both_dates(context):
    pair = context.reversal_for_merchant
    assert pair.get("charge_date"), f"Expected charge_date in reversal pair: {pair}"
    assert pair.get("refund_date"), f"Expected refund_date in reversal pair: {pair}"
    assert pair.get("amount"), f"Expected amount in reversal pair: {pair}"


@then('the response has no possible_reversals entry for merchant "{merchant}"')
def step_no_reversal_entry(context, merchant: str):
    result = context.spending_result
    reversals = result.get("possible_reversals", [])
    merchants = [r.get("merchant") for r in reversals]
    assert merchant not in merchants, (
        f"Expected no reversal for '{merchant}' but found one. "
        f"possible_reversals: {reversals}"
    )
