# ADR 0010 — apply_changeset accepts category names; resolves to UUIDs server-side

**Status:** Accepted  
**Issue:** #53  
**Date:** 2026-06-08

---

## Context

`apply_changeset` lets callers recategorize transactions by supplying a
`category` field per change entry. The MCP boundary is human-readable — callers
write `"category": "Pets"`, not an opaque UUID.

Monarch's `updateTransaction` mutation (ADR 0002 §8) requires `categoryId` to be
a **category UUID** (e.g. `"abc123-…"`). Sending a human-readable name as
`categoryId` causes Monarch to return an opaque server error:

```json
[{"locations":[{"column":43,"line":1}],
  "message":"Something went wrong while processing: None on request_id: None."}]
```

This is the same class of bad-mutation-value error as issues #47 and #48
(GraphQL `Int` overflow from sending `u32::MAX` as a transaction-list limit).

---

## Decision

**The tool handler (`apply_approved_changeset` in `src/tools.rs`) resolves
category names → UUIDs before calling the Monarch API.**

1. When any applied change carries a `category` field, `get_categories()` is
   called **once per `apply_changeset` invocation** (not once per change) to
   obtain the full category catalog.

2. The pure function `resolve_category_names()` in `src/triage.rs` maps each
   name to its UUID using a `HashMap<&str, &str>` built from the catalog:
   - **Known name** → replaced with its UUID in the `AppliedChange`.
   - **Unknown name** → moved to `RejectedChange` with reason
     `unknown category "Foo"`. It is **never sent to the Monarch API**.
   - **No category** (tags/notes-only change) → passes through unchanged.

3. `update_transaction` receives the resolved UUID in the `category_id`
   parameter, which it already inserts as `input.categoryId`. No change to
   `update_transaction`'s signature or semantics is required.

4. The `ApplyResult` returned to the caller reflects the resolution: unknown
   names appear in `rejected_changes`, not `applied_changes`.

---

## Consequences

### Positive

- Eliminates the opaque Monarch mutation error for valid category names.
- MCP clients write human-readable names; UUID complexity is hidden server-side.
- Unknown category names are caught **before** any API call, with a clear reason
  that names the offending string.
- `resolve_category_names` is pure and I/O-free — fully unit-testable without a
  mock server (small tier).

### Negative / trade-offs

- One extra `GetCategories` GraphQL call per `apply_changeset` invocation that
  includes a category change. The call is skipped entirely for tags/notes-only
  changesets.
- If Monarch's category catalog is stale relative to the caller's expectations
  (e.g., a category was renamed between triage and apply), the change is
  rejected as unknown. The caller must re-triage with the current catalog.

---

## Alternatives considered

**Send names and handle the error**: Retry with a lookup on 4xx/5xx. Rejected —
the error Monarch returns is opaque (no field reference, no UUID hint); there is
no reliable signal to distinguish "bad categoryId" from other server errors.

**Require callers to supply UUIDs**: Would make the MCP tool harder to use
(UUIDs are not surfaced in `triage_uncategorized` or `inspect_transactions`
output). Contradicts the tool's goal of being human-readable at the MCP boundary.

**Cache categories across calls**: Adds stale-data risk with no significant
performance benefit at typical changeset sizes. A single `GetCategories` call
per `apply_changeset` invocation is cheap.

---

## Test coverage

| Tier | File | What it tests |
|------|------|---------------|
| Small | `src/triage.rs` | `resolve_category_names`: known→UUID, unknown→rejection, no-category passthrough, mixed batch, empty catalog |
| Medium | `bdd/features/apply_changeset.feature` | End-to-end via MCP: UUID recorded by mock, unknown rejected with no mutation, tags-only unaffected |
| Large | `tests/live_integration.rs` | Real Monarch: name→UUID resolution, mutation accepted, change persisted, revert persisted |
