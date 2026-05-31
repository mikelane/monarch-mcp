# ADR 0004 — Category `group.type` Classification for `spending_report`

**Status:** Accepted  
**Date:** 2026-05-31  
**Context:** Fix the sign-convention bug (#24) in `spending_report`: income and transfer
categories were mixed into `total_spent` and falsely flagged as over-budget because the
classifier field was not threaded through the transaction query.

---

## Background

Monarch Money represents every transaction with a signed `amount`:
- Expense outflows: **negative** (e.g., `-439.94` for insurance)
- Income (paychecks, interest): **positive**
- Transfers (credit-card payments): **positive** on the receiving side

`category.group.type` is the authoritative classifier for how a category behaves. Without
it, `spending_report` has no way to separate grocery spending from a paycheck deposit.

The bug: `aggregate_spending_by_category` summed raw `txn.amount` for all categories,
netting income against expenses. `find_over_budget_categories` checked `spent >
budget.abs()` — a negative expense never exceeds a positive magnitude, so real overruns
were silently hidden while income categories (positive amounts) were falsely flagged.

---

## Decision

Thread `category.group.type` from `GetTransactionsList` → `CategoryRaw` → `Category` →
`Transaction`, and use it in `spending_report` to classify transactions:

- `group_type == "expense"`: include spend magnitude `(-amount).max(0.0)` in `total_spent`
- `group_type == "income"` or `"transfer"`: exclude from spending entirely
- `group_type == None` (unknown): defensive fallback — treat negative amounts as expense
  magnitude so real spending is never silently hidden (see below)

---

## Schema Sources

The field `category.group.type` is a **non-null lowercase string enum** with values:

| Value | Meaning |
|-------|---------|
| `"expense"` | Outflow category (food, housing, insurance, etc.) |
| `"income"` | Inflow category (paychecks, interest, etc.) |
| `"transfer"` | Between-account move (credit-card payments, savings transfers) |

**Sources confirming the schema:**

1. **`emmachase/splitsync` `schema.graphql`** — introspected Monarch GraphQL schema;
   `CategoryGroup.type` is a non-null enum with values `INCOME`, `EXPENSE`, `TRANSFER`.
   The wire value is lowercase (Monarch serialises the enum to lowercase strings).

2. **`hammem/monarchmoney` `GetCategories` query** — community Python library uses
   `group { id name type }` in category queries; the field is always present in responses.

3. **`eshaffer321/monarchmoney-go`** — Go community library; tests assert
   `Type == "income"` and `Type == "expense"` (lowercase), confirming wire format.

4. **ADR 0002** (`0002-real-monarch-schema.md`) — the `GetCategories` operation already
   selects `group { id name type }`. `CategoryWithId` was not carrying `type`; this ADR
   extends the threading.

**Credit-card payments** fall under the `transfer` group, NOT a fourth type. They should
never appear in `total_spent`.

**`systemCategory`** is nullable and its token values are unpinned — do NOT hardcode
`systemCategory` values to determine group type. Use `group.type` exclusively.

---

## Defensive Fallback

When `group_type` is `None` (a category whose group type the API did not return — should
not occur with the current query, but possible if Monarch adds new group types or returns
partial data), we fall back to sign alone:

- negative `amount` → treat as expense magnitude `(-amount).max(0.0)`
- non-negative `amount` → treat as 0 (income or refund, not counted as spend)

This ensures real spending is never silently hidden due to a missing classifier. The
fallback is documented here so future maintainers understand the intent.

---

## Consequences

- `total_spent` now reflects only expense outflows (positive magnitudes)
- `percent_of_budget` is always positive for expense categories
- `find_over_budget_categories` correctly detects real overruns
- Income and transfer categories never appear in `over_budget_categories`
- A changed client operation (`GetTransactionsList`) requires a gated large test per the
  project test-pyramid rules (`tests/live_integration.rs`, `MONARCH_LIVE=1`)
- All unit-test fixtures in `spending_report.rs` are re-baselined to the real Monarch sign
  convention (negative expense amounts)
- `bdd/mock_monarch/server.py` `_make_transaction` now emits `category.group` with `type`
  matching the fixture's `group_type` key (default `"expense"`)
