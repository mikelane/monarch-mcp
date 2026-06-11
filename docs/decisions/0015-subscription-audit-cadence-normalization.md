# ADR 0015 — Subscription Audit: Cadence Normalization, Approximate Streams, and Inventory vs. Anomaly Lens

**Status:** Accepted  
**Issue:** #68  
**Date:** 2026-06-10

## Context

`recurring_scan` (ADR 0003) uses `Web_GetUpcomingRecurringTransactionItems` to surface
creeping amounts and upcoming renewals — an *anomaly* lens. Issue #68 requests a second,
distinct tool: a full ranked inventory of the household's recurring-charge burn, annualized,
with totals — an *inventory* lens.

Both tools share the same Monarch data source (`recurringTransactionItems`) but answer
different questions and produce different output contracts.

## Decision 1 — Separate tool, not an overloaded `recurring_scan`

`subscription_audit` is a **new tool** with its own handler, compute module
(`src/subscription_audit.rs`), and output contract:

```json
{
  "subscriptions": [
    {"merchant": "StreamBundle", "monthly_amount": 50.0, "annualized_amount": 600.0,
     "cadence": "monthly", "approximate": false},
    {"merchant": "NewsCo", "monthly_amount": 10.0, "annualized_amount": 120.0,
     "cadence": "yearly", "approximate": false}
  ],
  "total_monthly": 60.0,
  "total_annual": 720.0
}
```

`recurring_scan` is **not changed**. Its output (`creeping_charges`, `upcoming_renewals`)
remains unchanged. No shared compute helper was extracted because the two tools have no
overlapping logic — `recurring_scan` works on diffs and past/future flags while
`subscription_audit` works on stream amounts and cadence.

## Decision 2 — Cadence normalization factor table

Each recurring stream carries a `frequency` string from Monarch. To produce a
monthly-equivalent amount the following factor table is applied:

| Monarch `frequency`                         | Monthly factor   | Rationale                         |
|---------------------------------------------|------------------|-----------------------------------|
| `"monthly"`                                 | 1.0              | Already per-month                 |
| `"yearly"` / `"annually"`                   | 1 / 12 ≈ 0.0833  | One charge per 12 months          |
| `"weekly"`                                  | 52 / 12 ≈ 4.333  | 52 weeks per year                 |
| `"biweekly"` / `"every_two_weeks"`          | 26 / 12 ≈ 2.167  | 26 pay periods per year           |
| `"quarterly"` / `"every_three_months"`      | 1 / 3  ≈ 0.333   | 4 charges per year                |
| `"semiannual"` / `"twice_a_year"`           | 1 / 6  ≈ 0.167   | 2 charges per year                |
| (unknown / any other value)                 | 1.0              | Conservative: treat as monthly    |

`annualized_amount = monthly_equivalent * 12`

The fallback for unknown cadences is monthly (factor 1.0) rather than zero, to avoid
silently zeroing a stream whose frequency string changes in a future Monarch schema version.
This is the conservative choice: a stream appearing too large is more visible than a stream
disappearing from the totals.

## Decision 3 — Approximate streams: include and flag

Streams with `is_approximate = true` (utilities, variable charges) are **included** in the
audit with their `stream.amount` as the expected amount. They are flagged with
`"approximate": true` in the output so callers can distinguish fixed from variable costs.

Rationale: approximate streams represent real budget load. Excluding them would understate
total recurring burn. Flagging them lets the advisor communicate uncertainty explicitly
("your electric bill is approximately $120/month") without hiding the cost.

This differs from `recurring_scan`'s treatment: `recurring_scan` excludes approximate
streams from creeping detection (amount drift is expected on variable charges). Both
treatments are correct for their respective questions.

## Decision 4 — Income exclusion

Streams with `stream.amount > 0` (positive = inflow in Monarch sign convention) are
**excluded** from the subscription audit. The tool answers "what do I spend on
subscriptions?" — income streams are irrelevant to that question.

The filter is applied at the compute layer (`stream_amount < 0.0`), not at the client
layer, so the client can remain a thin data-fetching layer.

## Decision 5 — Stream deduplication in `get_recurring_for_audit`

`Web_GetUpcomingRecurringTransactionItems` returns one *item* per scheduled occurrence
within the requested date window. A weekly subscription that fires four times in a month
produces four items from the same underlying stream.

`get_recurring_for_audit` deduplicates by `(merchant_name, stream_amount_bits)` key to
produce one `SubscriptionAuditItem` per *stream*. The normalization factor then handles
the per-period → monthly conversion correctly (a weekly stream's item has the weekly
amount; multiplied by 52/12 it produces the correct monthly equivalent).

Alternative considered: deduplicate by stream id. The `id` field in `RecurringStreamRaw`
is fetched by the GraphQL query but not mapped into any Rust struct — mapping it would
require a new field in the input type and a test for the new field. The
merchant+amount key is sufficient because two different streams with the same merchant
and the same expected amount are, for the purposes of an audit, indistinguishable to the
user. If Monarch ever splits one stream into two with identical names/amounts (unlikely),
the totals would still be correct since only one entry is deduped.

## Decision 6 — `RecurringStreamRaw.frequency` field

The `frequency` field was already requested in both `get_recurring` and
`get_recurring_for_scan` GraphQL queries but was **not deserialized** in
`RecurringStreamRaw`. This was not a bug for those two callers (they don't use
frequency), but `subscription_audit` requires it.

The field is added to `RecurringStreamRaw` as `pub frequency: String`. Since
`RecurringStreamRaw` is private (not part of any public API), the change is
backward-compatible. The two existing callers (`get_recurring`, `get_recurring_for_scan`)
are unaffected — they ignore the field after deserialization.

A gated large test for the new field is not added separately because the existing
`get_recurring_for_scan_parses_enriched_fields` test already verifies that `frequency`
is present in the mock response shape (it was in the fixture), and the new
`subscription_audit_returns_valid_structure_and_reconciles_with_recurring_scan`
live test verifies the field is non-empty on real Monarch data.

## Alternatives considered

**Reuse `get_recurring_for_scan` in the audit handler.** Rejected: `RecurringScanItem`
does not carry `frequency`, and adding it to `RecurringScanItem` would leak the audit
concern into the scan type. Separate input types with separate client methods keep each
tool's data contract independent.

**Compute monthly-equivalent from transaction history.** Rejected: the stream's declared
`frequency` is the correct source of truth for cadence. Transaction history is unreliable
for streams with irregular occurrence counts in a short window.

**Use a 30-day month for all normalizations (amount * 30 / cadence_days).** Rejected:
the factor table above is exact for the cadence strings Monarch actually uses; a 30-day
approximation introduces unnecessary error for yearly charges (365/12 ≠ 30).

## Consequences

- `subscription_audit` and `recurring_scan` remain independently testable and have no
  shared compute logic. Adding a new cadence or changing one tool's behavior cannot
  break the other.
- `RecurringStreamRaw.frequency` is now deserialized, making the field available to any
  future tool that needs cadence information without a new GraphQL query.
- The monthly-factor table is the single source of truth for cadence normalization. Any
  new Monarch frequency string will fall back to monthly (factor 1.0) until explicitly
  added to the match arm.
