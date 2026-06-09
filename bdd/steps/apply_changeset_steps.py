"""Step definitions for apply_changeset.feature (@ISSUE-53).

Tests that apply_changeset resolves category names to UUIDs before sending
mutations, and rejects unknown category names without calling the API.
"""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given('a transaction "{txn_id}" exists with category "{category}"')
def step_transaction_exists(context, txn_id: str, category: str):
    """Seed a single transaction with the given id and category name."""
    txn = {
        "id": txn_id,
        "merchant": f"Merchant for {txn_id}",
        "amount": -10.0,
        "category": category,
        "date": "2026-05-15",
    }
    requests.post(f"{context.mock_base}/configure", json={"transactions": [txn]})


@given('the category "{name}" has UUID "{uuid}"')
def step_category_has_uuid(context, name: str, uuid: str):
    """Register a category with an explicit UUID in the mock's category catalog."""
    existing = getattr(context, "_categories_override", [])
    # Avoid duplicates if the step is called multiple times
    existing = [c for c in existing if c["name"] != name]
    existing.append({"id": uuid, "name": name})
    context._categories_override = existing
    requests.post(
        f"{context.mock_base}/configure",
        json={"categories_override": existing},
    )


@given('no category named "{name}" exists')
def step_no_category_named(context, name: str):
    """Ensure the named category is absent from the mock's catalog.

    If categories_override is already set, remove any entry with that name.
    If it is not set, initialise it to an empty list so the override path
    is taken (empty override → no categories → every name is unknown).
    """
    existing = getattr(context, "_categories_override", [])
    existing = [c for c in existing if c["name"] != name]
    context._categories_override = existing
    requests.post(
        f"{context.mock_base}/configure",
        json={"categories_override": existing},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when(
    'the advisor applies a changeset setting transaction "{txn_id}" category to "{category}"'
)
def step_apply_category_change(context, txn_id: str, category: str):
    context.apply_result = call_tool(
        context,
        "apply_changeset",
        {"changes": [{"id": txn_id, "category": category}]},
    )


@when('the advisor applies a changeset adding tag "{tag}" to transaction "{txn_id}"')
def step_apply_tag_change(context, tag: str, txn_id: str):
    context.apply_result = call_tool(
        context,
        "apply_changeset",
        {"changes": [{"id": txn_id, "tags": [tag]}]},
    )


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then('the mutation for transaction "{txn_id}" recorded categoryId "{expected_id}"')
def step_mutation_recorded_category_id(context, txn_id: str, expected_id: str):
    """Verify the mock recorded the resolved UUID, not the category name."""
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    match = next((c for c in applied if str(c.get("id")) == txn_id), None)
    assert match is not None, (
        f"Expected a mutation for transaction {txn_id!r} but none was recorded. "
        f"applied_changes={applied!r}"
    )
    recorded_id = match.get("categoryId")
    assert recorded_id == expected_id, (
        f"Expected categoryId {expected_id!r} for txn {txn_id!r}, "
        f"got {recorded_id!r}. full record={match!r}"
    )


@then('the mutation for transaction "{txn_id}" recorded no categoryId')
def step_mutation_recorded_no_category_id(context, txn_id: str):
    """Verify the mutation did not include a categoryId (tags-only change)."""
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    match = next((c for c in applied if str(c.get("id")) == txn_id), None)
    assert match is not None, (
        f"Expected a mutation for transaction {txn_id!r} but none was recorded. "
        f"applied_changes={applied!r}"
    )
    assert "categoryId" not in match, (
        f"Expected no categoryId in mutation for {txn_id!r}, "
        f"but found {match.get('categoryId')!r}. full record={match!r}"
    )


@then("no rejected changes are reported")
def step_no_rejected_changes(context):
    result = context.apply_result
    rejected = result.get("rejected_changes", [])
    assert not rejected, (
        f"Expected no rejected changes, but got: {rejected!r}. "
        f"full result={result!r}"
    )


@then('no mutation is recorded for transaction "{txn_id}"')
def step_no_mutation_recorded(context, txn_id: str):
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    match = next((c for c in applied if str(c.get("id")) == txn_id), None)
    assert match is None, (
        f"Expected NO mutation for transaction {txn_id!r}, "
        f"but found: {match!r}. applied_changes={applied!r}"
    )


@then(
    'a rejected change is reported for transaction "{txn_id}" mentioning "{text}"'
)
def step_rejected_change_reported(context, txn_id: str, text: str):
    result = context.apply_result
    rejected = result.get("rejected_changes", [])
    match = next((r for r in rejected if r.get("id") == txn_id), None)
    assert match is not None, (
        f"Expected a rejected change for transaction {txn_id!r} "
        f"but none found. rejected_changes={rejected!r}. full result={result!r}"
    )
    reason = match.get("reason", "")
    assert text in reason, (
        f"Expected rejection reason to mention {text!r}, "
        f"got {reason!r} for txn {txn_id!r}"
    )
