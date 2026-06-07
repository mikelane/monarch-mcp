# ADR 0008 — GraphQL Int32 limit for unbounded transaction fetches

**Status:** Accepted  
**Issue:** #47  
**Date:** 2026-06-07

## Context

`financial_overview` and `spending_report` use `fetch_current_month_transactions`, which
calls `client.get_transactions(start, end, limit)` with `limit = u32::MAX` (4,294,967,295).
The intent was "fetch every transaction — no silent row cap".

Both tools were failing with:

```
GraphQL "[{\"message\":\"Something went wrong while processing: None on request_id: None.\"}]"
```

Live probing confirmed:

- `get_cashflow` alone → succeeds
- `get_transactions(limit: 500)` alone → succeeds
- `get_transactions(limit: u32::MAX)` alone → **fails with the above GraphQL error**

## Root cause

Monarch's GraphQL schema types the `limit` argument as `Int` (signed 32-bit integer,
range −2,147,483,648 to 2,147,483,647). Passing `u32::MAX` (4,294,967,295) overflows
that range. Monarch's resolver receives an out-of-range value and returns the opaque
server-side error rather than a validation message.

This was misdiagnosed as a concurrency issue because the symptom only appeared when
multiple requests fired together (masking that the transaction request itself was
always broken, just not tested in isolation at that limit value).

## Decision

Use `i32::MAX as u32` (2,147,483,647) as the effective "fetch all" limit:

```rust
client.get_transactions(start, end, i32::MAX as u32).await
```

`i32::MAX` rows (≈2.1 billion) is unbounded for any real household.
It fits in GraphQL's `Int` type and Monarch accepts it without error.

Additionally, `fetch_and_compute` and `fetch_and_compute_spending` now sequence the
transaction fetch **after** the lightweight requests (accounts, cashflow, budgets,
net-worth history) complete. This is belt-and-suspenders: the primary fix is the
corrected limit value; the sequencing guards against any residual server-side
contention from a large response payload on the same HTTP/2 connection.

## Consequences

- `financial_overview` and `spending_report` now succeed reliably against the real
  Monarch API.
- Latency is slightly higher than the old concurrent burst (lightweight requests run
  in parallel; the heavy fetch follows sequentially). The increase is bounded by one
  extra round-trip and is negligible compared to the prior failure cost.
- Future callers that need an unbounded fetch must use `i32::MAX as u32`, not
  `u32::MAX`. A comment in `fetch_current_month_transactions` documents why.
- The large (live) test tier now includes two regression tests
  (`financial_overview_concurrent_burst_exercises_production_fetch_path` and
  `spending_report_concurrent_burst_exercises_production_fetch_path`) that call
  the private `fetch_and_compute` / `fetch_and_compute_spending` functions directly,
  ensuring a revert of the `i32::MAX` fix would cause them to fail.

## Testing-seam decision

The regression tests live inside `src/tools.rs` under `#[cfg(test)]`, not in
`tests/live_integration.rs`. An external integration test (separate crate) cannot
reach private helpers, so it must re-implement the fetch pattern inline — a copy
that does not guard the production code path. In-crate `#[cfg(test)]` tests call
the real orchestration functions directly and are gated by `MONARCH_LIVE=1` so
`cargo test` and CI remain hermetic. `pub(crate)` exposure was not needed and was
avoided to keep the public API surface minimal.
