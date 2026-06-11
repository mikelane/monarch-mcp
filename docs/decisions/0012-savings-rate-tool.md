# ADR 0012 — savings_rate tool: income source, zero-income guard, compact output

**Status:** Accepted
**Date:** 2026-06-10
**Issue:** #65

## Context

No existing tool answers "what fraction of our income did we save this month?" The
`financial_overview` tool reports income and spending figures but does not compute a rate.
`spending_history` reports true spending without income, making the rate uncomputable from
a single tool call.

Two sub-decisions drive this ADR:

1. **Where to source income data** — the Monarch API exposes two possible sources:
   the `GetCashflow` query (which returns a pre-aggregated cashflow summary) and the
   `GetTransactionsList` query (which returns individual transactions).

2. **How to handle months with zero income** — dividing by zero must never produce
   `NaN` or `Infinity` in the JSON output.

## Decisions

### (a) Income sourced from GetTransactionsList, not GetCashflow

Income is computed by summing the positive amounts of transactions whose
`category.group_type` is `"income"`. The same `GetTransactionsList` call that
`spending_history` uses for expenses also returns income transactions.

**Why not GetCashflow?**

The cashflow API aggregates by Monarch's own category-group logic. Its income figure
includes transfers (e.g. credit-card payment refunds) that Monarch classifies as income
in certain configurations. The transaction-based income figure uses the same
`group_type` field filter applied to individual transactions, matching only those
explicitly tagged as income by the user — the same standard the user sees in the
Monarch "Income" view.

More importantly: `savings_rate.true_spending` is computed by the same
`transaction_spend_magnitude` helper used in `spending_history`. Using the same
transaction source for both income and spending means `savings_rate.true_spending`
always agrees with `spending_history.total_true_spending` for the same date range.
Using `GetCashflow` for income while using `GetTransactionsList` for spending would
introduce a dual-source discrepancy that could confuse advisor queries combining
both tools.

**Income definition:**

```
income(month) = Σ max(txn.amount, 0)
                  for all txn where txn.category.group_type == "income"
                  and txn.date ∈ [month_start, month_end]
```

Positive amounts only (`max(amount, 0)`) to guard against data anomalies. Expense
refunds with `group_type = "expense"` (i.e. positive amounts in the expense group)
are excluded — they reduce net spending but are not income.

### (b) Zero-income guard: savings_rate is absent, not NaN or Infinity

When a month has no income transactions (income = 0), the savings rate is
mathematically undefined. The field is represented as `Option<f64>` in Rust and
serialized with `#[serde(skip_serializing_if = "Option::is_none")]`. In the JSON
output the field is **absent** rather than `null`, `NaN`, or `Infinity`.

This means advisor code can always branch on field presence:

```
if "savings_rate" in month:
    display(month["savings_rate"])
else:
    display("N/A — no income recorded")
```

The `window_average_savings_rate` field follows the same rule: it is computed as
the mean of per-month rates, skipping zero-income months. If all months have zero
income, the field is absent.

### (c) Compact aggregated output — no raw transaction list

The response contains only per-month aggregates and a window average. Raw
transaction lists are never included. This matches the same compact-output contract
established by `spending_history` (ADR 0011) and prevents the tool from becoming
a proxy for `inspect_transactions`.

Per-month fields:

| Field | Type | Description |
|-------|------|-------------|
| `month` | `String` | `YYYY-MM` label |
| `income` | `f64` | Sum of income-group transaction amounts |
| `true_spending` | `f64` | Sum of expense-group magnitudes (transfers excluded) |
| `net_savings` | `f64` | `income − true_spending` |
| `savings_rate` | `f64?` | `(net_savings / income) × 100`; absent when income = 0 |

Top-level fields also include `range_start`, `range_end`, and
`window_average_savings_rate` (absent when all months have zero income).

### (d) Range resolution reuses spending_history helpers

`savings_rate` uses the same `resolve_history_range` and `range_for_months_count`
helpers as `spending_history`. The default window is the last 3 complete calendar
months (the current partial month is excluded). The range can be overridden via
`start_date`/`end_date` or shortened via `months`.

## Consequences

- `savings_rate.true_spending` is guaranteed to agree with
  `spending_history.total_true_spending` for the same date range — verified by the
  live integration test in `tests/live_integration.rs`.
- Zero-income months are surfaced structurally (absent field) rather than with a
  sentinel value, so advisor code never needs to check for NaN/Infinity.
- Adding income to the transaction fetch does not increase API call count — income
  and expense transactions are returned in the same `GetTransactionsList` response.
