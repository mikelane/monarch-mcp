"""Step definitions for recurring_scan.feature (@ISSUE-B3)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household has the following recurring charges this period")
def step_recurring_charges(context):
    """Table: merchant | stream_amount | actual_amount? | frequency | is_approximate | is_past?"""
    items = []
    for row in context.table:
        item: dict = {
            "merchant": row["merchant"],
            "stream_amount": float(row["stream_amount"]),
            "frequency": row["frequency"],
            "is_approximate": row["is_approximate"].lower() == "true",
        }
        # actual_amount is optional — defaults to stream_amount (stable charge)
        if "actual_amount" in row.headings:
            item["actual_amount"] = float(row["actual_amount"])
        else:
            item["actual_amount"] = item["stream_amount"]

        # is_past is optional — defaults to False (upcoming)
        if "is_past" in row.headings:
            item["is_past"] = row["is_past"].lower() == "true"
        else:
            item["is_past"] = False

        items.append(item)

    requests.post(
        f"{context.mock_base}/configure",
        json={"recurring_items": items},
    )


@given("the household has no recurring charges this period")
def step_no_recurring_charges(context):
    requests.post(
        f"{context.mock_base}/configure",
        json={"recurring_items": []},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor runs a recurring charge scan")
def step_run_recurring_scan(context):
    context.scan_result = call_tool(context, "recurring_scan")


# ---------------------------------------------------------------------------
# Then — creeping charge assertions
# ---------------------------------------------------------------------------


@then("the scan flags {merchant} as a creeping charge")
def step_assert_flagged_creeping(context, merchant: str):
    result = context.scan_result
    creeping = result.get("creeping_charges", [])
    names = [c.get("merchant") for c in creeping]
    assert merchant in names, (
        f"Expected {merchant!r} in creeping_charges but got: {names!r}"
    )


@then("the scan does not flag {merchant} as creeping")
def step_assert_not_flagged_creeping(context, merchant: str):
    result = context.scan_result
    creeping = result.get("creeping_charges", [])
    names = [c.get("merchant") for c in creeping]
    assert merchant not in names, (
        f"Expected {merchant!r} NOT in creeping_charges but it was: {names!r}"
    )


@then("the scan reports {merchant} increased by {amount:f} dollars")
def step_assert_increased_by(context, merchant: str, amount: float):
    result = context.scan_result
    creeping = result.get("creeping_charges", [])
    entry = next((c for c in creeping if c.get("merchant") == merchant), None)
    assert entry is not None, f"{merchant!r} not found in creeping_charges: {creeping!r}"
    change = entry.get("amount_change", 0.0)
    assert abs(change - amount) < 0.01, (
        f"Expected {merchant!r} amount_change {amount}, got {change}"
    )


@then("the scan reports {merchant} changed by {amount:f} dollars")
def step_assert_changed_by(context, merchant: str, amount: float):
    result = context.scan_result
    creeping = result.get("creeping_charges", [])
    entry = next((c for c in creeping if c.get("merchant") == merchant), None)
    assert entry is not None, f"{merchant!r} not found in creeping_charges: {creeping!r}"
    change = abs(entry.get("amount_change", 0.0))
    assert abs(change - amount) < 0.01, (
        f"Expected {merchant!r} |amount_change| {amount}, got {change}"
    )


@then("the scan returns no creeping charges")
def step_assert_no_creeping(context):
    result = context.scan_result
    creeping = result.get("creeping_charges", [])
    assert not creeping, f"Expected no creeping_charges but got: {creeping!r}"


# ---------------------------------------------------------------------------
# Then — upcoming renewal assertions
# ---------------------------------------------------------------------------


@then("the upcoming renewals list contains {merchant}")
def step_assert_upcoming_contains(context, merchant: str):
    result = context.scan_result
    upcoming = result.get("upcoming_renewals", [])
    names = [u.get("merchant") for u in upcoming]
    assert merchant in names, (
        f"Expected {merchant!r} in upcoming_renewals but got: {names!r}"
    )


@then("the upcoming renewals list does not contain {merchant}")
def step_assert_upcoming_not_contains(context, merchant: str):
    result = context.scan_result
    upcoming = result.get("upcoming_renewals", [])
    names = [u.get("merchant") for u in upcoming]
    assert merchant not in names, (
        f"Expected {merchant!r} NOT in upcoming_renewals but it was: {names!r}"
    )


@then("the scan returns no upcoming renewals")
def step_assert_no_upcoming(context):
    result = context.scan_result
    upcoming = result.get("upcoming_renewals", [])
    assert not upcoming, f"Expected no upcoming_renewals but got: {upcoming!r}"
