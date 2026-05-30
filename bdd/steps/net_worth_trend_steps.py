"""Step definitions for net_worth_trend.feature (@ISSUE-B2)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("the household's net worth by account type over the past {n:d} months is")
def step_snapshots_by_type(context, n: int):
    """Table: month | account_type | balance"""
    rows = []
    for row in context.table:
        rows.append(
            {
                "month": row["month"],
                "account_type": row["account_type"],
                "balance": float(row["balance"]),
            }
        )
    context._snapshots_by_type = rows
    requests.post(
        f"{context.mock_base}/configure",
        json={"snapshots_by_type": rows},
    )


@given("the household has no net worth snapshot history")
def step_no_snapshot_history(context):
    context._snapshots_by_type = []
    requests.post(
        f"{context.mock_base}/configure",
        json={"snapshots_by_type": []},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests a net worth trend for the past {n:d} months")
def step_run_net_worth_trend(context, n: int):
    context.trend_result = call_tool(context, "net_worth_trend", {"months": n})


@when("the advisor requests a net worth trend for the past {n:d} month")
def step_run_net_worth_trend_singular(context, n: int):
    context.trend_result = call_tool(context, "net_worth_trend", {"months": n})


# ---------------------------------------------------------------------------
# Then — assertions
# ---------------------------------------------------------------------------


@then("the trend contains {n:d} monthly data points")
def step_assert_trend_points(context, n: int):
    result = context.trend_result
    points = result.get("monthly_snapshots", [])
    assert len(points) == n, (
        f"Expected {n} monthly data points, got {len(points)}: {points!r}"
    )


@then("the trend contains {n:d} monthly data point")
def step_assert_trend_points_singular(context, n: int):
    step_assert_trend_points(context, n)


@then("the trend reports a net worth of {amount:d} dollars in the most recent month")
def step_assert_latest_net_worth(context, amount: int):
    result = context.trend_result
    latest = result.get("latest_net_worth", 0.0)
    assert abs(latest - float(amount)) < 0.01, (
        f"Expected latest net worth {amount}, got {latest}"
    )


@then("the trend reports a net worth change of positive {amount:d} dollars over the period")
def step_assert_net_worth_change_positive(context, amount: int):
    result = context.trend_result
    change = result.get("net_worth_change", 0.0)
    assert abs(change - float(amount)) < 0.01, (
        f"Expected net worth change +{amount}, got {change}"
    )


@then("the trend reports a net worth change of {amount:d} dollars over the period")
def step_assert_net_worth_change(context, amount: int):
    result = context.trend_result
    change = result.get("net_worth_change", 0.0)
    assert abs(change - float(amount)) < 0.01, (
        f"Expected net worth change {amount}, got {change}"
    )


@then("the trend identifies {account_type} as the biggest mover")
def step_assert_biggest_mover(context, account_type: str):
    result = context.trend_result
    mover = result.get("biggest_mover", {}).get("account_type")
    assert mover == account_type, (
        f"Expected biggest mover {account_type!r}, got {mover!r}"
    )


@then("the trend reports {account_type} moved by positive {amount:d} dollars")
def step_assert_mover_positive(context, account_type: str, amount: int):
    result = context.trend_result
    by_type = result.get("by_account_type", {})
    change = by_type.get(account_type, {}).get("change", 0.0)
    assert abs(change - float(amount)) < 0.01, (
        f"Expected {account_type} change +{amount}, got {change}"
    )


@then("the trend reports {account_type} moved by positive {amount:f} dollars")
def step_assert_mover_positive_float(context, account_type: str, amount: float):
    result = context.trend_result
    by_type = result.get("by_account_type", {})
    change = by_type.get(account_type, {}).get("change", 0.0)
    assert abs(change - amount) < 0.01, (
        f"Expected {account_type} change +{amount}, got {change}"
    )


@then("the trend reports {account_type} moved by positive {amount:d} dollars")
def step_assert_type_moved_positive(context, account_type: str, amount: int):
    result = context.trend_result
    by_type = result.get("by_account_type", {})
    change = by_type.get(account_type, {}).get("change", 0.0)
    assert abs(change - float(amount)) < 0.01, (
        f"Expected {account_type} change +{amount}, got {change}"
    )


@then("the trend reports total assets of {amount:d} dollars")
def step_assert_total_assets(context, amount: int):
    result = context.trend_result
    assets = result.get("total_assets", 0.0)
    assert abs(assets - float(amount)) < 0.01, (
        f"Expected total assets {amount}, got {assets}"
    )


@then("the trend reports total liabilities of {amount:d} dollars")
def step_assert_total_liabilities(context, amount: int):
    result = context.trend_result
    liabilities = result.get("total_liabilities", 0.0)
    assert abs(liabilities - float(amount)) < 0.01, (
        f"Expected total liabilities {amount}, got {liabilities}"
    )
