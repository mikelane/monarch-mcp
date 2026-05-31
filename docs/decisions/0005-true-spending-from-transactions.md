# ADR 0005 — True spending is computed from transactions, not the Monarch aggregate

**Status:** Accepted  
**Date:** 2026-05-31  
**Issue:** #25

## Context

`financial_overview` previously returned `cashflow.spending`, which is
Monarch's server-side `sumExpense` aggregate from `Web_GetCashFlowPage`. This
aggregate:

1. **May include transfers and Credit Card Payments.** Categories whose
   `group_type` is `"transfer"` represent money moving between the family's
   own accounts (e.g. a CC payment transfers funds from checking to credit). They
   inflate the spending figure without representing consumption.

2. **Is opaque.** We cannot inspect Monarch's server-side logic to verify which
   categories it includes or excludes.

3. **Disagrees with `spending_report`.** `spending_report` sums transaction
   magnitudes using our own classification logic (`group_type` filtering, sign
   convention). Without a shared definition both tools could report different
   "spending" figures for the same month — confusing and untrustworthy.

## Decision

Both `financial_overview` and `spending_report` compute spending via the shared
pure function `compute_true_spending(transactions: &[Transaction]) -> f64`
(in `src/spending_report.rs`, re-used by `src/financial_overview.rs`).

**True spending** = sum of expense magnitudes across all transactions for the
period, where:
- `group_type == "expense"` → magnitude `(-amount).max(0.0)`
- `group_type == "income"` or `"transfer"` → 0 (excluded)
- `group_type == None` → sign-based fallback (negative → magnitude, positive → 0)

The `financial_overview` handler fetches current-month transactions (same call
`spending_report` already makes) and passes them to `compute_true_spending`.
The Monarch `cashflow.spending` aggregate is **not used** for the spending field.

`cashflow.income` is still sourced from the Monarch aggregate (`sumIncome`)
because income classification is less ambiguous and we have no intent to
override it.

## Consequences

- `financial_overview.cashflow.spending` and `spending_report.total_spent` agree
  by construction — they call the same function with the same transactions.
- Transfer and Credit Card Payment categories are excluded from both figures.
- The `financial_overview` handler makes one additional API call
  (`GetTransactionsList`) per invocation. This is acceptable: the call already
  runs for `spending_report` and is bounded (500 transactions max).
- The Monarch `cashflow` field is still fetched (it carries `income` and
  `prior_month_spending`); only the `spending` field from that response is
  ignored.

## Alternatives Considered

**Use `cashflow.spending` in both tools.** Rejected: the aggregate is opaque,
may include transfers, and cannot be audited via our unit tests.

**Compute income from transactions too.** Deferred: income classification is
straightforward (`group_type == "income"`), Monarch's aggregate matches our
logic, and the change would add scope without fixing the reported bug.
