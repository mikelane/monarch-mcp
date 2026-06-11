# ADR 0013 — budget_review pace taxonomy and tolerance band

**Status:** Accepted  
**Date:** 2026-06-10  
**Issue:** #66

---

## Context

The `budget_review` tool answers the question: _which expense categories are burning through
their budget faster than the month is progressing?_  The core computation is:

```
pace_fraction  = today_day_of_month / days_in_month   (0 … 1)
percent_spent  = spent / budget * 100                 (0 … ∞)
```

We need a four-way taxonomy so the advisor can distinguish categories that are mildly fast
from those that have already blown their budget.

---

## Decision

### Pace status taxonomy

| Status | Condition | Meaning |
|--------|-----------|---------|
| `under` | `percent_spent < pace_fraction * 100 − TOLERANCE` | Spending trails the calendar; good headroom |
| `on_track` | within ±TOLERANCE of pace_fraction * 100 | Spending is proportional to elapsed time |
| `over` | `percent_spent > pace_fraction * 100 + TOLERANCE` AND `spent < budget` | Spending faster than the month, but not yet exhausted |
| `over_budget` | `spent >= budget` | Budget fully consumed (or exceeded); remaining ≤ 0 |

`over_budget` takes priority over all pace comparisons — a category at 110% is always
`over_budget` regardless of how much of the month has elapsed.

### Tolerance: ±10 percentage points

We chose **10 pp** as the `PACE_TOLERANCE_PP` constant.  A narrower band (e.g. 5 pp) would
flag normal daily-variance noise as over-pace.  A wider band (e.g. 20 pp) would mask real
acceleration until spending was already severe.  10 pp matches the informal mental model
most households use ("within 10% is fine").

Example at day 15 of a 31-day month (pace ≈ 48.4 %):

| percent_spent | status |
|---|---|
| < 38.4 % | `under` |
| 38.4 – 58.4 % | `on_track` |
| > 58.4 % and spent < budget | `over` |
| spent ≥ budget | `over_budget` |

### Sign convention

All output values (`budget`, `spent`, `remaining`, `percent_spent`) are expressed as
**positive magnitudes**, consistent with the advisor-facing surface of every other tool in
this server.  Monarch stores expense budgets as negative `plannedCashFlowAmount` values; the
compute function converts to magnitudes before building `CategoryPacing`.

### Income and transfer exclusion

`transaction_spend_magnitude` (shared with `spending_report`) returns 0.0 for any
transaction whose `group_type` is `"income"` or `"transfer"`.  Because the spending map is
built by summing that helper, income and transfer transactions contribute nothing to any
category's `spent` total.

In production the budget `group_type` field is always `None` (the GraphQL join path omits
it), so the income/transfer guard on budgets themselves (`Some("income") | Some("transfer")`)
is a defensive belt-and-suspenders check that matters mainly in unit tests where synthetic
budgets can carry explicit group types.

### `percent_spent` is `Option<i64>`

When `budget == 0` division is undefined; the field is `None` rather than a sentinel value
such as `f64::INFINITY`.  Callers that display or compare this field must handle `None`.
The integer type (`i64`) is intentional: percent-spent is displayed as a whole number
(e.g. "84%"), so storing a fractional value would imply false precision.

---

## Alternatives considered

### Three-way taxonomy (under / on_track / over)

Rejected.  A category already past 100% spending is qualitatively different from one that
is merely fast — the advisor needs to treat them differently (e.g., stop recommending
spending in an `over_budget` category immediately, not just flag it for attention).

### Dynamic tolerance tied to category volatility

Rejected for v1.  Computing per-category historical variance would require multi-month
transaction history and significant additional complexity.  A fixed 10 pp is a reasonable
first approximation and can be made configurable in a future issue if households report it
is too noisy or too lenient.

### Pace fraction based on business days

Rejected.  Household expenses (groceries, restaurants, utilities) do not cluster on business
days.  A calendar-day fraction is a better model of how a typical household spends.

---

## Consequences

- The `PaceStatus` enum is serialized as snake_case strings (`under`, `on_track`, `over`,
  `over_budget`) — changing any value is a breaking API change.
- `PACE_TOLERANCE_PP = 10.0` is a named constant in `src/budget_review.rs`; adjusting it
  requires only a single-line change and re-running the test suite.
- The live integration test (`budget_review_returns_valid_structure_from_real_monarch`)
  verifies that rollup counts sum to the number of `by_category` entries — a regression
  guard for any future change to the taxonomy or exclusion logic.
