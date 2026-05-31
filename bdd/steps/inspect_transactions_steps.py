"""Step definitions for inspect_transactions.feature (@ISSUE-23)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given("three Pets transactions this month with known ids")
def step_three_pets_transactions(context):
    txns = [
        {
            "id": "txn-pets-1",
            "merchant": "Petco",
            "amount": -12000.0,
            "category": "Pets",
            "date": "2026-05-10",
        },
        {
            "id": "txn-pets-2",
            "merchant": "Banfield Vet",
            "amount": -500.0,
            "category": "Pets",
            "date": "2026-05-12",
        },
        {
            "id": "txn-pets-3",
            "merchant": "PetSmart",
            "amount": -150.0,
            "category": "Pets",
            "date": "2026-05-14",
        },
    ]
    context._inspect_txn_ids = [t["id"] for t in txns]
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given("five transactions this month across different categories")
def step_five_transactions(context):
    txns = [
        {
            "id": f"txn-multi-{i}",
            "merchant": f"Merchant {i}",
            "amount": -(10.0 * (i + 1)),
            "category": ["Pets", "Medical", "Dining", "Groceries", "Travel"][i],
            "date": "2026-05-15",
        }
        for i in range(5)
    ]
    context._inspect_txn_count = 5
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given("a Pets charge of ${charge:d} and a Petco refund of ${refund:d} this month")
def step_pets_charge_and_refund(context, charge: int, refund: int):
    txns = [
        {
            "id": "txn-charge-1",
            "merchant": "Petco",
            "amount": -float(charge),
            "category": "Pets",
            "date": "2026-05-10",
        },
        {
            "id": "txn-refund-1",
            "merchant": "Petco",
            "amount": float(refund),
            "category": "Pets",
            "date": "2026-05-15",
        },
    ]
    context._expected_outflow = float(charge)
    context._expected_inflow = float(refund)
    context._expected_net = float(refund) - float(charge)
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given('a "Petco" transaction and a "Banfield Vet" transaction both in Pets')
def step_petco_and_banfield(context):
    txns = [
        {
            "id": "txn-petco-1",
            "merchant": "Petco",
            "amount": -100.0,
            "category": "Pets",
            "date": "2026-05-10",
        },
        {
            "id": "txn-banfield-1",
            "merchant": "Banfield Vet",
            "amount": -200.0,
            "category": "Pets",
            "date": "2026-05-11",
        },
    ]
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


@given('a Pets transaction with id "{txn_id}" this month')
def step_pets_transaction_with_id(context, txn_id: str):
    txns = [
        {
            "id": txn_id,
            "merchant": "Petco",
            "amount": -150.0,
            "category": "Pets",
            "date": "2026-05-10",
        }
    ]
    context._inspect_txn_id = txn_id
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when('the advisor inspects transactions in the "{category}" category')
def step_inspect_by_category(context, category: str):
    context.inspect_result = call_tool(
        context, "inspect_transactions", {"category": category}
    )


@when("the advisor inspects transactions with no filters")
def step_inspect_no_filters(context):
    context.inspect_result = call_tool(context, "inspect_transactions", {})


@when('the advisor inspects transactions for merchant "{merchant}"')
def step_inspect_by_merchant(context, merchant: str):
    context.inspect_result = call_tool(
        context, "inspect_transactions", {"merchant": merchant}
    )


@when('the advisor inspects transactions with category filter ""')
def step_inspect_with_empty_category_filter(context):
    # An empty string must be transparent — the server must treat it as no
    # filter and return all transactions, not a misleading scoped view.
    context.inspect_result = call_tool(
        context, "inspect_transactions", {"category": ""}
    )


@when('the advisor applies a changeset recategorizing "{txn_id}" as "{new_category}"')
def step_apply_changeset_from_inspect(context, txn_id: str, new_category: str):
    context.apply_result = call_tool(
        context,
        "apply_changeset",
        {"changes": [{"id": txn_id, "category": new_category}]},
    )


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("the result contains all three Pets transactions")
def step_assert_three_pets(context):
    result = context.inspect_result
    txns = result.get("transactions", [])
    assert len(txns) == 3, (
        f"Expected 3 Pets transactions, got {len(txns)}. Full result: {result}"
    )


@then("every returned transaction has a non-empty id")
def step_assert_ids_non_empty(context):
    result = context.inspect_result
    txns = result.get("transactions", [])
    assert txns, f"Expected at least one transaction. Full result: {result}"
    for txn in txns:
        assert txn.get("id"), (
            f"Transaction missing non-empty id: {txn!r}. Full result: {result}"
        )


@then("the result contains all five transactions")
def step_assert_five_transactions(context):
    result = context.inspect_result
    txns = result.get("transactions", [])
    assert len(txns) == 5, (
        f"Expected 5 transactions, got {len(txns)}. Full result: {result}"
    )


@then("the summary shows total_outflow of ${amount:d}")
def step_assert_total_outflow(context, amount: int):
    result = context.inspect_result
    summary = result.get("summary", {})
    actual = summary.get("total_outflow", None)
    assert actual is not None, f"summary missing total_outflow. Full result: {result}"
    assert abs(actual - float(amount)) < 0.01, (
        f"Expected total_outflow={amount}, got {actual}. Full result: {result}"
    )


@then("the summary shows total_inflow of ${amount:d}")
def step_assert_total_inflow(context, amount: int):
    result = context.inspect_result
    summary = result.get("summary", {})
    actual = summary.get("total_inflow", None)
    assert actual is not None, f"summary missing total_inflow. Full result: {result}"
    assert abs(actual - float(amount)) < 0.01, (
        f"Expected total_inflow={amount}, got {actual}. Full result: {result}"
    )


@then("the summary shows net_amount of ${net:d}")
def step_assert_net_amount_negative(context, net: int):
    result = context.inspect_result
    summary = result.get("summary", {})
    actual = summary.get("net_amount", None)
    assert actual is not None, f"summary missing net_amount. Full result: {result}"
    assert abs(actual - float(net)) < 0.01, (
        f"Expected net_amount={net}, got {actual}. Full result: {result}"
    )


@then("the summary shows net_amount of $-{net:d}")
def step_assert_net_amount_negative_explicit(context, net: int):
    result = context.inspect_result
    summary = result.get("summary", {})
    actual = summary.get("net_amount", None)
    assert actual is not None, f"summary missing net_amount. Full result: {result}"
    expected = -float(net)
    assert abs(actual - expected) < 0.01, (
        f"Expected net_amount={expected}, got {actual}. Full result: {result}"
    )


@then('the result contains only the "Petco" transaction')
def step_assert_only_petco(context):
    result = context.inspect_result
    txns = result.get("transactions", [])
    assert len(txns) == 1, (
        f"Expected 1 transaction (Petco only), got {len(txns)}. Full result: {result}"
    )
    assert txns[0].get("merchant") == "Petco", (
        f"Expected merchant='Petco', got {txns[0].get('merchant')!r}. "
        f"Full result: {result}"
    )


@then("the apply result reports the change was applied without rejection")
def step_assert_apply_no_rejection(context):
    result = context.apply_result
    rejected = result.get("rejected_changes", [])
    assert not rejected, (
        f"Expected no rejected changes, got: {rejected!r}. Full result: {result}"
    )
    applied = result.get("applied_changes", [])
    assert applied, (
        f"Expected at least one applied change, got none. Full result: {result}"
    )


@then("no transaction was modified in the mock")
def step_assert_no_modifications(context):
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    assert not applied, (
        f"inspect_transactions must not apply any changes, but found: {applied!r}"
    )
