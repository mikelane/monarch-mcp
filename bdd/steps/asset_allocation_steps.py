"""Step definitions for asset_allocation.feature (@ISSUE-67).

The asset_allocation tool splits net worth by asset class — equities, cash,
real_estate, crypto, other_assets, liabilities, other — each with a dollar
total and a percent_of_assets (None for liabilities).

Fixture seeding reuses the same /configure + "accounts" mechanism as
account_inventory_steps.py: post a list of account dicts, the mock server's
_handle_get_accounts() shapes them into real GetAccounts response format.
No new mock server routes are required.
"""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock accounts for the scenario
# ---------------------------------------------------------------------------


@given("the following accounts for asset allocation:")
def step_configure_accounts_for_allocation(context):
    """Table: name | type | subtype | balance | is_hidden (optional)

    Mirrors account_inventory_steps.step_configure_accounts; kept separate so
    the two feature files remain independent and their steps don't collide.

    is_hidden column is optional — defaults to False.
    balance value "null" sends JSON null currentBalance (unsynced account).
    subtype value "null" sends JSON null subtype (no subtype on the account).
    """
    accounts = []
    has_subtype = "subtype" in context.table.headings
    has_is_hidden = "is_hidden" in context.table.headings

    for row in context.table:
        raw_balance = row["balance"].strip()
        balance = None if raw_balance.lower() == "null" else float(raw_balance)

        acct: dict = {
            "name": row["name"],
            "type": row["type"],
            "currentBalance": balance,
        }

        if has_subtype:
            raw_subtype = row["subtype"].strip()
            acct["subtype"] = None if raw_subtype.lower() == "null" else (raw_subtype or None)

        if has_is_hidden:
            acct["is_hidden"] = row["is_hidden"].strip().lower() == "true"

        accounts.append(acct)

    requests.post(
        f"{context.mock_base}/configure",
        json={"accounts": accounts},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests asset_allocation")
def step_request_asset_allocation(context):
    context.allocation_result = call_tool(context, "asset_allocation")


# ---------------------------------------------------------------------------
# Then — per-class assertions
# ---------------------------------------------------------------------------


@then('the "{asset_class}" class total is {amount:f}')
def step_assert_class_total(context, asset_class: str, amount: float):
    result = context.allocation_result
    classes = result.get("classes", {})
    assert asset_class in classes, (
        f"Expected asset class {asset_class!r} in allocation, "
        f"got classes: {list(classes.keys())}"
    )
    actual = classes[asset_class].get("total", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected {asset_class!r} class total {amount}, got {actual}"
    )


@then('the "{asset_class}" class percent_of_assets is {percent:f}')
def step_assert_class_percent(context, asset_class: str, percent: float):
    result = context.allocation_result
    classes = result.get("classes", {})
    assert asset_class in classes, (
        f"Expected asset class {asset_class!r} in allocation, "
        f"got classes: {list(classes.keys())}"
    )
    actual = classes[asset_class].get("percent_of_assets")
    assert actual is not None, (
        f"Expected {asset_class!r} class to have percent_of_assets, got None"
    )
    assert abs(actual - percent) < 0.01, (
        f"Expected {asset_class!r} class percent_of_assets {percent}, got {actual}"
    )


@then('the "{asset_class}" class has no percent_of_assets')
def step_assert_class_no_percent(context, asset_class: str):
    result = context.allocation_result
    classes = result.get("classes", {})
    assert asset_class in classes, (
        f"Expected asset class {asset_class!r} in allocation, "
        f"got classes: {list(classes.keys())}"
    )
    actual = classes[asset_class].get("percent_of_assets")
    assert actual is None, (
        f"Expected {asset_class!r} class to have no percent_of_assets, got {actual}"
    )


@then('the "{asset_class}" class is flagged as not recognized')
def step_assert_class_not_recognized(context, asset_class: str):
    result = context.allocation_result
    classes = result.get("classes", {})
    assert asset_class in classes, (
        f"Expected asset class {asset_class!r} in allocation, "
        f"got classes: {list(classes.keys())}"
    )
    recognized = classes[asset_class].get("recognized", True)
    assert recognized is False, (
        f"Expected {asset_class!r} class recognized=false, got recognized={recognized!r}"
    )


# ---------------------------------------------------------------------------
# Then — rollup assertions
# ---------------------------------------------------------------------------


@then("the allocation total_liabilities is {amount:f}")
def step_assert_total_liabilities(context, amount: float):
    result = context.allocation_result
    actual = result.get("total_liabilities", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected total_liabilities {amount}, got {actual}"
    )


@then("the allocation net_worth is {amount:f}")
def step_assert_net_worth(context, amount: float):
    result = context.allocation_result
    actual = result.get("net_worth", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected net_worth {amount}, got {actual}"
    )


@then("the allocation gross_assets is {amount:f}")
def step_assert_gross_assets(context, amount: float):
    result = context.allocation_result
    actual = result.get("gross_assets", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected gross_assets {amount}, got {actual}"
    )
