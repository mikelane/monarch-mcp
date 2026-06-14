# ADR 0017 — Monarch transaction fetch: single-page at i32::MAX is sufficient; no pagination loop required

**Date:** 2026-06-14
**Status:** Accepted
**Issue:** #33

---

## Context

`get_transactions` (and now `get_transactions_with_count`) issues a single
`GetTransactionsList` query with `offset=0` and `limit=GRAPHQL_INT_MAX`
(`i32::MAX = 2,147,483,647`; see ADR 0008 for why `u32::MAX` is rejected).

The question deferred from PR #32 was: does Monarch silently cap
`results[]` below `allTransactions.totalCount` at real household volumes?
If so, a pagination loop (offset += page, repeat until exhausted) would be
needed to assemble the full set.

---

## Empirical verification (2026-06-14)

The live test `transaction_fetch_returns_full_result_set_no_silent_cap`
(in `tests/live_integration.rs`) called `get_transactions_with_count` at
`limit=GRAPHQL_INT_MAX` against a real Monarch account and observed:

| Range | `totalCount` | `results.len()` | Match? |
|-------|-------------|-----------------|--------|
| Current month (2026-06) | 149 | 149 | ✓ |
| Trailing 12 months (2025-06 – 2026-06) | 4,770 | 4,770 | ✓ |

`results.len() == totalCount` in both probes. Monarch returned the full
result set at `limit=i32::MAX` for this household's volume (hundreds of
transactions per month, low thousands across 12 months).

---

## Decision

**No pagination loop is needed.** The single-page `offset=0` fetch at
`limit=GRAPHQL_INT_MAX` captures the complete result set for a typical
household.

The `get_transactions_with_count` method (added in this issue) returns the
server's `totalCount` alongside the `Vec<Transaction>`. The live test
`transaction_fetch_returns_full_result_set_no_silent_cap` is the permanent
regression canary: it asserts `results.len() == totalCount` for both the
current month and trailing-12-month ranges. If Monarch ever introduces a
server-side cap below a household's true monthly transaction count, that
assertion will fail and a pagination loop must be added at that point.

---

## Consequences

- `get_transactions` and all callers (tool handlers, live tests) continue
  to issue a single query per date range. No offset-loop complexity added.
- `get_transactions_with_count` is the correct entry point for any future
  caller that needs to validate completeness — pass `totalCount` through
  and assert equality at the call site.
- If a future household has dramatically higher monthly volume and the
  assertion begins failing, implement a pagination loop in
  `get_transactions_with_count`: iterate `offset += page_size` until
  `accumulated.len() == totalCount` or the server returns an empty page,
  then update this ADR.

---

## Related

- ADR 0002 — real `GetTransactionsList` GraphQL schema
  (`allTransactions { totalCount results(offset, limit) { … } }`)
- ADR 0008 — why `limit` is capped at `i32::MAX` (Monarch's signed Int32)
- Issue #33 — the two deferred findings from PR #32 that motivated this work
- `src/client.rs` — `GRAPHQL_INT_MAX` (now `pub const`), `get_transactions_with_count`
- `tests/live_integration.rs` — `transaction_fetch_returns_full_result_set_no_silent_cap`
