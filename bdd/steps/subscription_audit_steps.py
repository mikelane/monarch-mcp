"""Step definitions for subscription_audit.feature (@ISSUE-68)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household has the following income streams:")
def step_income_streams(context):
    """Table: merchant | stream_amount | frequency

    Configures positive-amount (inflow) recurring streams.  The mock's normal
    recurring_items path always negates stream_amount to produce an outflow.
    Income items must bypass that negation, so they are posted via the
    ``income_items`` fixture key which the mock emits with a positive
    stream.amount (see mock_monarch/server.py _handle_web_get_upcoming_recurring).
    """
    items = []
    for row in context.table:
        items.append(
            {
                "merchant": row["merchant"],
                "stream_amount": float(row["stream_amount"]),
                "frequency": row.get("frequency", "monthly"),
                "is_approximate": False,
                "is_past": False,
            }
        )

    requests.post(
        f"{context.mock_base}/configure",
        json={"income_items": items},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests subscription_audit")
def step_run_subscription_audit(context):
    context.audit_result = call_tool(context, "subscription_audit")


# ---------------------------------------------------------------------------
# Then — subscription list assertions
# ---------------------------------------------------------------------------


@then("the subscription list contains {merchant}")
def step_assert_subscription_contains(context, merchant: str):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    names = [s.get("merchant") for s in subs]
    assert merchant in names, (
        f"Expected {merchant!r} in subscriptions but got: {names!r}"
    )


@then("the subscription list does not contain {merchant}")
def step_assert_subscription_not_contains(context, merchant: str):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    names = [s.get("merchant") for s in subs]
    assert merchant not in names, (
        f"Expected {merchant!r} NOT in subscriptions but it was: {names!r}"
    )


@then("the subscription list is empty")
def step_assert_subscription_empty(context):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    assert not subs, f"Expected empty subscriptions but got: {subs!r}"


@then("{merchant_a} is ranked above {merchant_b}")
def step_assert_ranked_above(context, merchant_a: str, merchant_b: str):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    names = [s.get("merchant") for s in subs]
    assert merchant_a in names, f"{merchant_a!r} not found in subscriptions: {names!r}"
    assert merchant_b in names, f"{merchant_b!r} not found in subscriptions: {names!r}"
    idx_a = names.index(merchant_a)
    idx_b = names.index(merchant_b)
    assert idx_a < idx_b, (
        f"Expected {merchant_a!r} (index {idx_a}) ranked above "
        f"{merchant_b!r} (index {idx_b})"
    )


@then("{merchant} annualized_amount is {expected:f}")
def step_assert_annualized_amount(context, merchant: str, expected: float):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    entry = next((s for s in subs if s.get("merchant") == merchant), None)
    assert entry is not None, f"{merchant!r} not found in subscriptions: {subs!r}"
    actual = entry.get("annualized_amount", 0.0)
    assert abs(actual - expected) < 0.01, (
        f"Expected {merchant!r} annualized_amount {expected:.2f}, got {actual}"
    )


@then("the {merchant} monthly_amount is {expected:f}")
def step_assert_monthly_amount(context, merchant: str, expected: float):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    entry = next((s for s in subs if s.get("merchant") == merchant), None)
    assert entry is not None, f"{merchant!r} not found in subscriptions: {subs!r}"
    actual = entry.get("monthly_amount", 0.0)
    assert abs(actual - expected) < 0.01, (
        f"Expected {merchant!r} monthly_amount {expected:.2f}, got {actual}"
    )


@then("total_monthly is {expected:f}")
def step_assert_total_monthly(context, expected: float):
    result = context.audit_result
    actual = result.get("total_monthly", 0.0)
    assert abs(actual - expected) < 0.01, (
        f"Expected total_monthly {expected:.2f}, got {actual}"
    )


@then("total_annual is {expected:f}")
def step_assert_total_annual(context, expected: float):
    result = context.audit_result
    actual = result.get("total_annual", 0.0)
    assert abs(actual - expected) < 0.01, (
        f"Expected total_annual {expected:.2f}, got {actual}"
    )


@then("{merchant} is flagged as approximate")
def step_assert_approximate_flagged(context, merchant: str):
    result = context.audit_result
    subs = result.get("subscriptions", [])
    entry = next((s for s in subs if s.get("merchant") == merchant), None)
    assert entry is not None, f"{merchant!r} not found in subscriptions: {subs!r}"
    assert entry.get("approximate") is True, (
        f"Expected {merchant!r} approximate=true, got: {entry!r}"
    )
