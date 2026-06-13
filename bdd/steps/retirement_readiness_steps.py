"""Step definitions for retirement_readiness.feature (@ISSUE-69).

The retirement_readiness tool combines:
  - Invested portfolio: Equities-class account balances (brokerage, 401k, Roth, HSA)
  - Annualised baseline spend: compute_true_spending over a trailing N-month window

Fixture seeding reuses the existing /configure mechanism:
  - "accounts" list → shaped by _handle_get_accounts (same as asset_allocation_steps)
  - "transactions" list → shaped by _handle_get_transactions_list

MONARCH_NOW is pinned to 2026-05-15 by environment.py, so "6 complete months"
covers Nov 2025 – Apr 2026. Transactions are spread across those months.
"""

from __future__ import annotations

import os

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock accounts and transactions
# ---------------------------------------------------------------------------


@given("the following accounts for retirement readiness:")
def step_configure_accounts_for_retirement(context):
    """Table: name | type | subtype | balance

    Mirrors asset_allocation_steps; kept separate so the two feature files
    remain independent.
    """
    accounts = []
    has_subtype = "subtype" in context.table.headings

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

        accounts.append(acct)

    requests.post(
        f"{context.mock_base}/configure",
        json={"accounts": accounts},
    )


@given("{n_months:d} months of expense transactions totalling {total:f}")
def step_configure_expense_transactions(context, n_months: int, total: float):
    """Spread `total` expense amount evenly across n_months complete months.

    MONARCH_NOW=2026-05-15, so complete months are Apr, Mar, Feb, Jan, Dec, Nov
    working backwards. Each month gets one expense transaction dated on the 15th.
    """
    # Build month labels working backwards from Apr 2026
    # (the most recent complete month before May 2026)
    months = _trailing_months(n_months)
    per_month = total / n_months
    txns = []
    for month in months:
        txns.append(
            {
                "merchant": "Monthly Expenses",
                # Monarch sign convention: expense outflows are NEGATIVE
                "amount": -per_month,
                "category": "Expenses",
                "group_type": "expense",
                "date": f"{month}-15",
            }
        )

    requests.post(
        f"{context.mock_base}/configure",
        json={"transactions": txns},
    )


def _trailing_months(n: int) -> list[str]:
    """Return n complete month labels (YYYY-MM) ending before the MONARCH_NOW month.

    Derives the anchor from MONARCH_NOW (e.g. "2026-05-15") so the helper
    survives a test-clock change without needing a manual update.
    The most recent *complete* month is always one month before the current month.
    """
    monarch_now = os.environ.get("MONARCH_NOW", "")
    if not monarch_now:
        # Should not occur in the BDD suite (environment.py pins this).
        raise RuntimeError(
            "MONARCH_NOW is not set — environment.py must pin it before scenarios run"
        )

    # MONARCH_NOW is always ISO "YYYY-MM-DD"; unpacking documents that shape
    # and turns any malformed value into a clear ValueError at this boundary.
    year_str, month_str, _day_str = monarch_now.split("-")
    anchor_year, anchor_month = int(year_str), int(month_str)

    # Step back one month from the current month to get the most recent complete month.
    anchor_month -= 1
    if anchor_month == 0:
        anchor_month = 12
        anchor_year -= 1

    result = []
    year, month = anchor_year, anchor_month
    for _ in range(n):
        result.append(f"{year:04}-{month:02}")
        month -= 1
        if month == 0:
            month = 12
            year -= 1
    return result


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor requests retirement_readiness")
def step_request_retirement_readiness_default(context):
    context.rr_result = call_tool(context, "retirement_readiness", {})


@when("the advisor requests retirement_readiness with withdrawal_rate {rate:f}")
def step_request_retirement_readiness_with_rate(context, rate: float):
    context.rr_result = call_tool(
        context,
        "retirement_readiness",
        {"withdrawal_rate": rate},
    )


# ---------------------------------------------------------------------------
# Then — field assertions
# ---------------------------------------------------------------------------


def _get_rr(context) -> dict:
    """Return the retirement_readiness result, setting context.rr_result if needed."""
    result = getattr(context, "rr_result", None)
    assert result is not None, (
        "No retirement_readiness result found — make sure the When step ran first."
    )
    return result


@then("the retirement_readiness invested_assets is {amount:f}")
def step_assert_invested_assets(context, amount: float):
    result = _get_rr(context)
    actual = result.get("invested_assets")
    assert actual is not None, f"invested_assets missing from result: {result}"
    assert abs(actual - amount) < 0.01, (
        f"Expected invested_assets={amount}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness annual_baseline_spend is {amount:f}")
def step_assert_annual_baseline_spend(context, amount: float):
    result = _get_rr(context)
    actual = result.get("annual_baseline_spend")
    assert actual is not None, f"annual_baseline_spend missing from result: {result}"
    # Tolerance is 1.0 (not 0.01 like other assertions) because annualisation
    # multiplies the average monthly spend by 12, which can accumulate up to
    # ~12× the per-month floating-point rounding slack. A 1.0 band is
    # appropriate for annualized figures while still catching real divergences.
    assert abs(actual - amount) < 1.0, (
        f"Expected annual_baseline_spend={amount}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness sustainable_annual_withdrawal is {amount:f}")
def step_assert_sustainable_withdrawal(context, amount: float):
    result = _get_rr(context)
    actual = result.get("sustainable_annual_withdrawal")
    assert actual is not None, (
        f"sustainable_annual_withdrawal missing from result: {result}"
    )
    assert abs(actual - amount) < 0.01, (
        f"Expected sustainable_annual_withdrawal={amount}, got {actual}."
    )


@then("the retirement_readiness coverage_ratio is {ratio:f}")
def step_assert_coverage_ratio(context, ratio: float):
    result = _get_rr(context)
    actual = result.get("coverage_ratio")
    assert actual is not None, (
        f"coverage_ratio is None (or missing) — expected {ratio}. Result: {result}"
    )
    assert abs(actual - ratio) < 0.001, (
        f"Expected coverage_ratio={ratio}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness coverage_ratio is less than {threshold:f}")
def step_assert_coverage_ratio_less_than(context, threshold: float):
    result = _get_rr(context)
    actual = result.get("coverage_ratio")
    assert actual is not None, (
        f"coverage_ratio is None — expected a value < {threshold}. Result: {result}"
    )
    assert actual < threshold, (
        f"Expected coverage_ratio < {threshold}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness target_portfolio is {amount:f}")
def step_assert_target_portfolio(context, amount: float):
    result = _get_rr(context)
    actual = result.get("target_portfolio")
    assert actual is not None, (
        f"target_portfolio is None — expected {amount}. Result: {result}"
    )
    assert abs(actual - amount) < 1.0, (
        f"Expected target_portfolio={amount}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness surplus_or_gap is negative")
def step_assert_surplus_or_gap_negative(context):
    result = _get_rr(context)
    actual = result.get("surplus_or_gap")
    assert actual is not None, (
        f"surplus_or_gap is None — expected a negative value. Result: {result}"
    )
    assert actual < 0.0, (
        f"Expected surplus_or_gap < 0 (a gap), got {actual}. Full result: {result}"
    )


@then("the retirement_readiness withdrawal_rate_used is {rate:f}")
def step_assert_withdrawal_rate_used(context, rate: float):
    result = _get_rr(context)
    actual = result.get("withdrawal_rate_used")
    assert actual is not None, (
        f"withdrawal_rate_used missing from result: {result}"
    )
    assert abs(actual - rate) < 1e-6, (
        f"Expected withdrawal_rate_used={rate}, got {actual}. Full result: {result}"
    )


@then("the retirement_readiness response contains an error about withdrawal rate range")
def step_assert_withdrawal_rate_error(context):
    result = _get_rr(context)
    error = result.get("error", "")
    assert error, (
        f"Expected an error for out-of-range withdrawal_rate, got: {result}"
    )
    assert "range" in error.lower() or "supported" in error.lower(), (
        f"Error message must mention the supported range. Got: {error!r}"
    )
