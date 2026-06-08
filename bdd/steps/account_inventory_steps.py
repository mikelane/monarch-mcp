"""Step definitions for account_inventory.feature (@ISSUE-50)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household has the following accounts:")
def step_configure_accounts(context):
    """Table: name | type | subtype | balance | is_hidden (optional)

    Subtype may be omitted from the header — defaults to "checking".
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


@when("the advisor runs an account inventory")
def step_run_inventory(context):
    context.inventory_result = call_tool(context, "account_inventory")


# ---------------------------------------------------------------------------
# Then — structural assertions
# ---------------------------------------------------------------------------


@then('the inventory has a "{bucket}" bucket')
def step_assert_bucket_exists(context, bucket: str):
    result = context.inventory_result
    buckets = result.get("buckets", {})
    assert bucket in buckets, (
        f"Expected bucket {bucket!r} in inventory, got keys: {list(buckets.keys())}"
    )


@then('the inventory does not have a "{bucket}" bucket')
def step_assert_bucket_absent(context, bucket: str):
    result = context.inventory_result
    buckets = result.get("buckets", {})
    assert bucket not in buckets, (
        f"Expected no {bucket!r} bucket, but it was present with "
        f"{len(buckets[bucket]['accounts'])} accounts"
    )


@then('the "{bucket}" bucket contains {count:d} account')
def step_assert_bucket_count_singular(context, bucket: str, count: int):
    _assert_bucket_account_count(context, bucket, count)


@then('the "{bucket}" bucket contains {count:d} accounts')
def step_assert_bucket_count_plural(context, bucket: str, count: int):
    _assert_bucket_account_count(context, bucket, count)


def _assert_bucket_account_count(context, bucket: str, expected: int) -> None:
    result = context.inventory_result
    buckets = result.get("buckets", {})
    assert bucket in buckets, f"Bucket {bucket!r} not found"
    actual = len(buckets[bucket].get("accounts", []))
    assert actual == expected, (
        f"Expected {expected} account(s) in {bucket!r}, got {actual}"
    )


@then('the "{bucket}" bucket total is {amount:f} dollars')
def step_assert_bucket_total(context, bucket: str, amount: float):
    result = context.inventory_result
    buckets = result.get("buckets", {})
    assert bucket in buckets, f"Bucket {bucket!r} not found"
    actual = buckets[bucket].get("total", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected {bucket!r} bucket total {amount}, got {actual}"
    )


# ---------------------------------------------------------------------------
# Then — rollup assertions
# ---------------------------------------------------------------------------


@then("the inventory rollup shows total assets of {amount:f} dollars")
def step_assert_rollup_assets(context, amount: float):
    rollup = context.inventory_result.get("rollup", {})
    actual = rollup.get("total_assets", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected total_assets {amount}, got {actual}"
    )


@then("the inventory rollup shows total liabilities of {amount:f} dollars")
def step_assert_rollup_liabilities(context, amount: float):
    rollup = context.inventory_result.get("rollup", {})
    actual = rollup.get("total_liabilities", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected total_liabilities {amount}, got {actual}"
    )


@then("the inventory rollup shows net worth of {amount:f} dollars")
def step_assert_rollup_net_worth(context, amount: float):
    rollup = context.inventory_result.get("rollup", {})
    actual = rollup.get("net_worth", 0.0)
    assert abs(actual - amount) < 0.01, (
        f"Expected net_worth {amount}, got {actual}"
    )


# ---------------------------------------------------------------------------
# Then — per-account flag assertions
# ---------------------------------------------------------------------------


@then('the account "{name}" in the "{bucket}" bucket is flagged as hidden')
def step_assert_account_hidden(context, name: str, bucket: str):
    account = _find_account_in_bucket(context, name, bucket)
    assert account.get("is_hidden"), (
        f"Expected account {name!r} in {bucket!r} to be flagged as hidden, "
        f"but is_hidden={account.get('is_hidden')!r}"
    )


@then('the account "{name}" in the "{bucket}" bucket is flagged as unknown subtype')
def step_assert_account_unknown_subtype(context, name: str, bucket: str):
    account = _find_account_in_bucket(context, name, bucket)
    assert account.get("unknown_subtype"), (
        f"Expected account {name!r} in {bucket!r} to have unknown_subtype=true, "
        f"but unknown_subtype={account.get('unknown_subtype')!r}"
    )


@then('the account "{name}" in the "{bucket}" bucket is flagged as balance unknown')
def step_assert_account_balance_unknown(context, name: str, bucket: str):
    account = _find_account_in_bucket(context, name, bucket)
    assert account.get("balance_unknown"), (
        f"Expected account {name!r} in {bucket!r} to have balance_unknown=true, "
        f"but balance_unknown={account.get('balance_unknown')!r}"
    )


def _find_account_in_bucket(context, name: str, bucket: str) -> dict:
    result = context.inventory_result
    buckets = result.get("buckets", {})
    assert bucket in buckets, f"Bucket {bucket!r} not found in inventory"
    accounts = buckets[bucket].get("accounts", [])
    matching = [a for a in accounts if a.get("display_name") == name]
    assert matching, (
        f"Account {name!r} not found in {bucket!r} bucket. "
        f"Available: {[a.get('display_name') for a in accounts]}"
    )
    return matching[0]
