"""Step definitions for triage_uncategorized.feature (@ISSUE-A6)."""

from __future__ import annotations

import requests
from behave import given, then, when

from steps.common import call_tool


# ---------------------------------------------------------------------------
# Given — configure mock fixtures
# ---------------------------------------------------------------------------


@given('past transactions from "{merchant}" were categorized as {category}')
def step_past_categorized(context, merchant: str, category: str):
    """Seed transaction history so the triage engine can infer a category."""
    history = getattr(context, "_history_transactions", [])
    history.append(
        {
            "merchant": merchant,
            "amount": 5.50,
            "category": category,
            "date": "2026-04-10",
        }
    )
    context._history_transactions = history
    all_txns = history + getattr(context, "_uncategorized_transactions", [])
    requests.post(f"{context.mock_base}/configure", json={"transactions": all_txns})


@given('no past transactions from "{merchant}"')
def step_no_past_transactions(context, merchant: str):
    """Ensure the merchant has no history so no category suggestion is made."""
    # Remove any history for this merchant from the fixture store
    history = [
        t for t in getattr(context, "_history_transactions", [])
        if t.get("merchant") != merchant
    ]
    context._history_transactions = history
    all_txns = history + getattr(context, "_uncategorized_transactions", [])
    requests.post(f"{context.mock_base}/configure", json={"transactions": all_txns})


@given('a new uncategorized transaction from "{merchant}"')
def step_new_uncategorized(context, merchant: str):
    uncategorized = getattr(context, "_uncategorized_transactions", [])
    uncategorized.append(
        {
            "merchant": merchant,
            "amount": 5.50,
            "category": "Uncategorized",
            "date": "2026-05-20",
        }
    )
    context._uncategorized_transactions = uncategorized
    context._last_uncategorized_merchant = merchant
    requests.post(
        f"{context.mock_base}/configure",
        json={"transactions_needing_review": uncategorized},
    )
    all_txns = getattr(context, "_history_transactions", []) + uncategorized
    requests.post(f"{context.mock_base}/configure", json={"transactions": all_txns})


@given('an uncategorized transaction from "{merchant}"')
def step_uncategorized_only(context, merchant: str):
    """Same as step_new_uncategorized — no historical context."""
    step_new_uncategorized(context, merchant)


@given('a proposed change categorizing the "{merchant}" transaction as {category}')
def step_proposed_change(context, merchant: str, category: str):
    txn_id = "txn-1"
    uncategorized = [
        {
            "merchant": merchant,
            "amount": 5.50,
            "category": "Uncategorized",
            "date": "2026-05-20",
            "id": txn_id,
        }
    ]
    requests.post(
        f"{context.mock_base}/configure",
        json={
            "transactions_needing_review": uncategorized,
            "transactions": uncategorized,
        },
    )
    # Register the target category in the mock catalog so the Rust client's
    # name→UUID resolution succeeds. In real Monarch the target category would
    # already exist in the household's category list — the pre-fix code skipped
    # this resolution entirely (the bug), so the old setup never needed it.
    # A budget entry is the lightest seed: it registers the category without
    # adding spurious transactions that would corrupt count assertions.
    requests.post(
        f"{context.mock_base}/configure",
        json={"budgets": [{"category": category, "amount": -50.0}]},
    )
    # Use the transaction id in the changeset — the apply_changeset tool
    # requires an explicit id; merchant-based lookup was removed when
    # ChangeEntry adopted deny_unknown_fields (see triage.rs).
    context._proposed_changeset = [{"id": txn_id, "category": category}]


@given("a proposed change categorizing one transaction as {category}")
def step_proposed_change_one_transaction(context, category: str):
    """A changeset targeting the first transaction in the existing list."""
    all_txns = getattr(context, "_all_transactions", [])
    if all_txns:
        txn_id = str(all_txns[0].get("id", "0"))
        merchant = all_txns[0].get("merchant", "")
    else:
        txn_id = "0"
        merchant = "Merchant 0"
    # Register the target category in the mock catalog so the Rust client's
    # name→UUID resolution succeeds. A budget entry is the lightest seed:
    # it registers the category without adding spurious transactions that
    # would corrupt the 40-transaction count assertion in the sibling Then step.
    requests.post(
        f"{context.mock_base}/configure",
        json={"budgets": [{"category": category, "amount": -50.0}]},
    )
    context._proposed_changeset = [
        {"merchant": merchant, "category": category, "id": txn_id}
    ]


@given("the month has {count:d} transactions")
def step_month_transaction_count(context, count: int):
    txns = [
        {
            "merchant": f"Merchant {i}",
            "amount": 10.0,
            "category": "General",
            "date": "2026-05-15",
            "id": str(i),
        }
        for i in range(count)
    ]
    context._all_transactions = txns
    requests.post(f"{context.mock_base}/configure", json={"transactions": txns})
    context.expected_transaction_count = count


@given("a proposed change that sets a transaction amount to {amount:d} dollars")
def step_proposed_amount_change(context, amount: int):
    """A changeset entry that includes an amount field — must be rejected."""
    context._proposed_changeset = [
        {"id": "txn-forbidden", "amount": float(amount)}
    ]
    # Seed a transaction with that id so the mock can look it up
    forbidden_txn = {
        "merchant": "Forbidden Merchant",
        "amount": 99.0,
        "category": "General",
        "date": "2026-05-15",
        "id": "txn-forbidden",
    }
    requests.post(
        f"{context.mock_base}/configure",
        json={"transactions": [forbidden_txn]},
    )


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("the advisor triages uncategorized transactions")
def step_triage(context):
    context.triage_result = call_tool(context, "triage_uncategorized")


@when("the advisor applies the approved changeset")
def step_apply_changeset(context):
    changeset = getattr(context, "_proposed_changeset", [])
    context.apply_result = call_tool(
        context, "apply_changeset", {"changes": changeset}
    )


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then('the proposed change categorizes the "{merchant}" transaction as {category}')
def step_assert_proposed_category(context, merchant: str, category: str):
    result = context.triage_result
    proposed = result.get("proposed_changes", [])
    match = next((p for p in proposed if p.get("merchant") == merchant), None)
    assert match is not None, (
        f"Expected a proposed change for {merchant!r} in {proposed!r}. "
        f"Full result: {result}"
    )
    assert match.get("category") == category, (
        f"Expected category {category!r} for {merchant!r}, got {match.get('category')!r}. "
        f"Full result: {result}"
    )


@then('no category is proposed for the "{merchant}" transaction')
def step_assert_no_proposed_category(context, merchant: str):
    result = context.triage_result
    proposed = result.get("proposed_changes", [])
    merchant_proposals = [p for p in proposed if p.get("merchant") == merchant]
    assert not merchant_proposals, (
        f"Expected NO proposed change for {merchant!r}, but found: {merchant_proposals!r}. "
        f"Full result: {result}"
    )


@then('the "{merchant}" transaction remains uncategorized')
def step_assert_still_uncategorized(context, merchant: str):
    """Triage proposes but does not apply — the mock must still show Uncategorized."""
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    merchant_applied = [c for c in applied if c.get("merchant") == merchant]
    assert not merchant_applied, (
        f"Triage should NOT have applied changes for {merchant!r}, "
        f"but found: {merchant_applied!r}"
    )


@then('the "{merchant}" transaction is categorized as {category}')
def step_assert_categorized(context, merchant: str, category: str):
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    match = next(
        (
            c for c in applied
            if c.get("merchant") == merchant or c.get("category") == category
        ),
        None,
    )
    assert match is not None, (
        f"Expected {merchant!r} to be categorized as {category!r} in applied changes. "
        f"Got: {applied!r}"
    )


@then("the month still has the same {count:d} transactions")
def step_assert_transaction_count_unchanged(context, count: int):
    # The id-set in the mock is the source of truth for "no transaction was
    # created or deleted". apply_changeset only edits category/tags/notes, so
    # the month's transaction count must be identical before and after.
    #
    # We deliberately do NOT read apply_result["transaction_count"] here: as of
    # #26 that field reports the number of changeset *entries processed*
    # (applied + rejected), not the month's transaction total — the vestigial
    # all-transactions fetch that used to populate the total was removed.
    resp = requests.post(
        f"{context.mock_base}/graphql",
        json={"operationName": "GetTransactionsList", "variables": {"filters": {}}},
    )
    data = resp.json()
    actual = data.get("data", {}).get("allTransactions", {}).get("totalCount")
    assert actual == count, (
        f"Expected the month to still have {count} transactions, got {actual!r}. "
        f"apply_result was: {context.apply_result}"
    )


# Alias for the original step wording ("the month still has {count:d} transactions")
@then("the month still has {count:d} transactions")
def step_assert_transaction_count_unchanged_plain(context, count: int):
    step_assert_transaction_count_unchanged(context, count)


@then("only category, tag, and note fields were changed")
def step_assert_only_allowed_fields_changed(context):
    """Verify the applied changeset contains no forbidden fields."""
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    forbidden_fields = {"amount", "date", "merchant", "id"}
    for change in applied:
        changed_fields = set(change.keys()) - {"id"}  # id is always present as a key
        disallowed = changed_fields & forbidden_fields
        assert not disallowed, (
            f"Applied change contains forbidden field(s) {disallowed!r}: {change!r}"
        )


@then("no transaction amount is changed")
def step_assert_no_amount_changed(context):
    """No applied change record should contain an amount key."""
    resp = requests.get(f"{context.mock_base}/applied_changes")
    applied = resp.json()
    amount_changes = [c for c in applied if "amount" in c and not c.get("rejected")]
    assert not amount_changes, (
        f"Expected no amount changes, but found: {amount_changes!r}"
    )


@then("the advisor reports the disallowed change was rejected")
def step_assert_rejection_reported(context):
    """The tool result must indicate the forbidden change was rejected."""
    result = context.apply_result
    # The tool should surface a rejection either in the result body or via
    # the mock's applied_changes rejection record.
    rejected_in_result = result.get("rejected_changes", [])
    if not rejected_in_result:
        # Fall back: check the mock recorded a rejection
        resp = requests.get(f"{context.mock_base}/applied_changes")
        applied = resp.json()
        rejected_in_result = [c for c in applied if c.get("rejected")]
    assert rejected_in_result, (
        f"Expected a rejected-change record, found none. "
        f"apply_result={result!r}"
    )
