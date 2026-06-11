"""Step definitions for budget_review.feature (@ISSUE-66)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _post_fixtures(context) -> None:
    """Push the current budgets + transactions to the mock configure endpoint."""
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "budgets": getattr(context, "_budgets", []),
            "transactions": getattr(context, "_transactions", []),
        },
    )


# ---------------------------------------------------------------------------
# Given -- configure mock fixtures
# ---------------------------------------------------------------------------


@given('a "{category}" budget of {amount:d} dollars')
def step_add_budget(context, category: str, amount: int):
    """Add an expense budget entry (negative amount per Monarch sign convention)."""
    budgets = getattr(context, "_budgets", [])
    budgets.append(
        {
            "category": category,
            # Monarch stores expense budgets as negative plannedCashFlowAmount.
            "amount": -float(amount),
        }
    )
    context._budgets = budgets
    _post_fixtures(context)


@given('{amount:d} dollars spent on "{category}" so far this month')
def step_add_expense_spend(context, amount: int, category: str):
    """Add a negative-amount expense transaction for the pinned current month."""
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": f"{category} merchant",
            # Monarch sign convention: expense outflows are negative.
            "amount": -float(amount),
            "category": category,
            "group_type": "expense",
            "date": "2026-05-10",
        }
    )
    context._transactions = txns
    _post_fixtures(context)


@given('an income transaction of {amount:d} dollars in category "{category}"')
def step_add_income_transaction(context, amount: int, category: str):
    """Add an income transaction — must NOT contribute to spending or pacing."""
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": category,
            "amount": float(amount),
            "category": category,
            "group_type": "income",
            "date": "2026-05-01",
        }
    )
    context._transactions = txns
    _post_fixtures(context)


@given('a transfer transaction of {amount:d} dollars in category "{category}"')
def step_add_transfer_transaction(context, amount: int, category: str):
    """Add a transfer transaction — must NOT contribute to spending or pacing."""
    txns = getattr(context, "_transactions", [])
    txns.append(
        {
            "merchant": category,
            "amount": -float(amount),
            "category": category,
            "group_type": "transfer",
            "date": "2026-05-05",
        }
    )
    context._transactions = txns
    _post_fixtures(context)


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests budget_review")
def step_request_budget_review(context):
    context.budget_review_result = call_tool(context, "budget_review", {})


# ---------------------------------------------------------------------------
# Then -- helpers
# ---------------------------------------------------------------------------


def _get_category(context, category: str) -> dict:
    """Return the CategoryPacing entry for the given category name."""
    result = context.budget_review_result
    by_category = result.get("by_category", {})
    if category not in by_category:
        raise AssertionError(
            f"Category {category!r} not found in budget_review result. "
            f"Available categories: {list(by_category.keys())}. "
            f"Full result: {result}"
        )
    return by_category[category]


def _get_rollup(context) -> dict:
    """Return the rollup block from the budget_review result."""
    result = context.budget_review_result
    rollup = result.get("rollup")
    assert rollup is not None, (
        f"Expected 'rollup' key in budget_review result. Full result: {result}"
    )
    return rollup


# ---------------------------------------------------------------------------
# Then -- category assertions
# ---------------------------------------------------------------------------


@then('"{category}" shows budget {budget:f} and spent {spent:f}')
def step_assert_budget_and_spent(
    context, category: str, budget: float, spent: float
):
    cat = _get_category(context, category)
    actual_budget = cat.get("budget")
    actual_spent = cat.get("spent")
    assert abs(actual_budget - budget) < 0.001, (
        f"Expected {category!r} budget={budget}, got {actual_budget!r}. Cat: {cat}"
    )
    assert abs(actual_spent - spent) < 0.001, (
        f"Expected {category!r} spent={spent}, got {actual_spent!r}. Cat: {cat}"
    )


@then('"{category}" remaining is {remaining:f}')
def step_assert_remaining(context, category: str, remaining: float):
    cat = _get_category(context, category)
    actual = cat.get("remaining")
    assert abs(actual - remaining) < 0.001, (
        f"Expected {category!r} remaining={remaining}, got {actual!r}. Cat: {cat}"
    )


@then('"{category}" pace_status is "{status}"')
def step_assert_pace_status(context, category: str, status: str):
    cat = _get_category(context, category)
    actual = cat.get("pace_status")
    assert actual == status, (
        f"Expected {category!r} pace_status={status!r}, got {actual!r}. Cat: {cat}"
    )


@then('"{category}" does not appear in the pacing list')
def step_assert_category_absent(context, category: str):
    result = context.budget_review_result
    by_category = result.get("by_category", {})
    assert category not in by_category, (
        f"Category {category!r} must not appear in budget_review pacing list, "
        f"but it was found. by_category keys: {list(by_category.keys())}"
    )


@then('"{category}" appears in the pacing list')
def step_assert_category_present(context, category: str):
    result = context.budget_review_result
    by_category = result.get("by_category", {})
    assert category in by_category, (
        f"Category {category!r} must appear in budget_review pacing list, "
        f"but it was not found. by_category keys: {list(by_category.keys())}"
    )


# ---------------------------------------------------------------------------
# Then -- rollup assertions
# ---------------------------------------------------------------------------


@then("the rollup shows at least {count:d} over category")
def step_assert_rollup_over(context, count: int):
    rollup = _get_rollup(context)
    actual = rollup.get("over_count", 0)
    assert actual >= count, (
        f"Expected rollup.over_count >= {count}, got {actual}. Rollup: {rollup}"
    )


@then("the rollup shows at least {count:d} on_track category")
def step_assert_rollup_on_track(context, count: int):
    rollup = _get_rollup(context)
    actual = rollup.get("on_track_count", 0)
    assert actual >= count, (
        f"Expected rollup.on_track_count >= {count}, got {actual}. Rollup: {rollup}"
    )


# ---------------------------------------------------------------------------
# Then -- no raw transactions
# ---------------------------------------------------------------------------


@then("the budget_review response does not contain raw transactions")
def step_assert_no_raw_transactions(context):
    result = context.budget_review_result
    assert "transactions" not in result, (
        f"Response must not contain a raw 'transactions' key. "
        f"Keys present: {list(result.keys())}"
    )
    by_category = result.get("by_category", {})
    for cat_name, cat_data in by_category.items():
        assert "transactions" not in cat_data, (
            f"Category {cat_name!r} must not contain a 'transactions' key. "
            f"Keys present: {list(cat_data.keys())}"
        )
