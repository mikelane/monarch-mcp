"""Step definitions for spending_report.feature (@ISSUE-A5)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the {category} budget is {amount:d} dollars this month")
def step_budget(context, category: str, amount: int):
    budgets = getattr(context, "_budgets", [])
    # Replace existing entry for the category if present
    budgets = [b for b in budgets if b["category"] != category]
    budgets.append({"category": category, "amount": float(amount)})
    context._budgets = budgets
    requests.post(f"{context.mock_base}/configure", json={"budgets": budgets})


@given("the household has spent {amount:d} dollars on {category} this month")
def step_category_spending(context, amount: int, category: str):
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": f"{category} merchant",
            "amount": float(amount),
            "category": category,
            "date": "2026-05-15",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given('a charge of {amount} dollars from "{merchant}" on the {day}')
def step_first_charge(context, amount: str, merchant: str, day: str):
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": merchant,
            "amount": float(amount),
            "category": "Subscriptions",
            "date": f"2026-05-{int(day.rstrip('th').rstrip('st').rstrip('nd').rstrip('rd')):02d}",
        }
    )
    context._transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given('another charge of {amount} dollars from "{merchant}" on the {day}')
def step_second_charge(context, amount: str, merchant: str, day: str):
    # Identical to the first — triggers duplicate detection
    step_first_charge(context, amount, merchant, day)


@given("the household spent {amount:d} dollars last month")
def step_prior_month_spending(context, amount: int):
    context.prior_month_spending = float(amount)
    requests.post(
        f"{context.mock_base}/configure",
        json={"prior_month_spending": float(amount)},
    )


@given("the household has spent {amount:d} dollars this month")
def step_this_month_spending(context, amount: int):
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": "Various",
            "amount": float(amount),
            "category": "General",
            "date": "2026-05-15",
        }
    )
    context._transactions = txns
    context.this_month_spending = float(amount)
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor generates a spending report for this month")
def step_generate_spending_report(context):
    context.spending_result = call_tool(
        context, "spending_report", {"period": "this_month"}
    )


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("the report flags {category} as over budget")
def step_assert_over_budget_flag(context, category: str):
    result = context.spending_result
    over_budget = result.get("over_budget_categories", [])
    assert category in over_budget, (
        f"Expected {category!r} in over_budget_categories, got {over_budget!r}. "
        f"Full result: {result}"
    )


@then("the report shows {category} at {pct:d} percent of budget")
def step_assert_budget_pct(context, category: str, pct: int):
    result = context.spending_result
    by_category = result.get("by_category", {})
    actual_pct = by_category.get(category, {}).get("percent_of_budget")
    assert actual_pct == pct, (
        f"Expected {category} at {pct}% of budget, got {actual_pct!r}. "
        f"Full result: {result}"
    )


@then("the report does not flag {category} as over budget")
def step_assert_not_over_budget(context, category: str):
    result = context.spending_result
    over_budget = result.get("over_budget_categories", [])
    assert category not in over_budget, (
        f"Expected {category!r} NOT in over_budget_categories, got {over_budget!r}. "
        f"Full result: {result}"
    )


@then('the report flags a possible duplicate charge from "{merchant}"')
def step_assert_duplicate_flag(context, merchant: str):
    result = context.spending_result
    duplicates = result.get("possible_duplicates", [])
    merchants = [d.get("merchant") for d in duplicates]
    assert merchant in merchants, (
        f"Expected duplicate from {merchant!r} in {merchants!r}. "
        f"Full result: {result}"
    )


@then("the report shows spending up {amount:d} dollars versus the prior month")
def step_assert_spending_up(context, amount: int):
    result = context.spending_result
    delta = result.get("vs_prior_month", {}).get("delta")
    assert delta == float(amount), (
        f"Expected spending delta +{amount}, got {delta!r}. Full result: {result}"
    )
