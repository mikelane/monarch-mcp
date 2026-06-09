# ADR 0011 — spending_history tool: multi-month baseline, fixed/discretionary taxonomy, compact output

**Status:** Accepted
**Date:** 2026-06-08
**Issue:** #54

## Context

`spending_report` covers current + prior month only. `inspect_transactions` returns raw
transaction lists with no income/transfer exclusion, no aggregation, and no month-over-month
structure. Neither tool supports the multi-month spending baseline needed for retirement-spending
analysis (e.g. "what is our average monthly spend over the last 6 months, and how much of it
is non-negotiable?").

The absence of a clean multi-month tool meant that any advisor workflow requiring a baseline
had to call `spending_report` or `inspect_transactions` N times and aggregate manually —
toil that the MCP server should eliminate.

## Decisions

### (a) New `spending_history` compound tool

A new tool (`src/spending_history.rs` + handler in `tools.rs`) computes per-month true spending
for a configurable range. The default is the last 6 complete calendar months (excluding the
current partial month). The range can also be set explicitly via `start_date`/`end_date`.

The tool reuses the existing `compute_true_spending` exclusion logic from
`src/spending_report.rs` by making `transaction_spend_magnitude` `pub(crate)`. This ensures
both tools apply identical income/transfer exclusion rules — the same bug fixed in issue #24
cannot appear in `spending_history`.

### (b) Fixed vs. discretionary category taxonomy

Each monthly entry splits true spending into **fixed** and **discretionary** buckets:

**FIXED** — non-negotiable recurring obligations whose amounts cannot easily be reduced
month-to-month:
- `"mortgage"` / `"rent"` — housing payments
- `"insurance"` — health, auto, home, life premiums
- `"utilities"` / `"utility"` — electricity, gas, water, internet
- `"loan"` — debt service on a fixed repayment schedule
- `"medical"` / `"dental"` — recurring healthcare premiums and copay plans

**DISCRETIONARY** — all other expense categories.

The taxonomy is implemented as `FIXED_CATEGORY_PATTERNS: &[&str]` — matched against the
category name using **whole-word (token-boundary) matching**, not substring matching.
The category name is split on non-alphanumeric characters (spaces, `/`, `-`, `&`, etc.)
into lowercase tokens, and a pattern matches only when its own tokens appear as a contiguous
sequence in the category's token list. This prevents false positives: "Concert Rentals"
does NOT match `"rent"` (only the token "rentals" is present), and "Accidental Purchases"
does NOT match `"dental"`.

Patterns like `"loan"` still match both "Auto Loan" and "Loan Repayment" because "loan"
appears as a whole token in each. Multi-word patterns (if ever needed) match as a token
sequence. The constant is documented and testable; future maintainers can extend it without
changing logic.

**Rationale for whole-word matching over exact enumeration:** Monarch category names are
user-customisable. An exact list would be brittle and expensive to maintain. Whole-word
token matching covers the common variants (e.g. "Home Mortgage", "Car Loan") while
avoiding false positives from coincidental substring containment.

### (c) Compact aggregate-only output contract

The tool **never** returns raw transaction lists. Each monthly entry contains:
- `total_true_spending` — scalar f64
- `by_category` — `HashMap<String, f64>` (name → magnitude sum)
- `split` — `FixedDiscretionarySplit { fixed, discretionary }`
- `outliers` — `Vec<SpendingOutlier>` (omitted when empty)

**Rationale:** A 6-month transaction pull for an active household easily exceeds 500
transactions, often 2,000+. Returning raw transactions in MCP tool output overflows the
LLM's context window and makes the tool useless. All aggregation is done server-side so
the advisor receives only the numbers it needs.

The `outliers` field surfaces single large transactions that dominate their category (≥ 3×
the per-transaction average of the other transactions in the same category in that month).
This catches annual insurance premiums, large medical bills, etc. — events that would
otherwise distort trend analysis. The comparison is made against the *rest-of-category*
average (excluding the candidate) to prevent the outlier from inflating its own threshold.

### (d) "Complete months" default — exclude the current partial month

When `months` is used (not explicit dates), the range covers the last `N` complete calendar
months, ending on the last day of the month before the current one. This matches the user's
mental model of "last 6 months" and avoids partial-month distortion.

## Consequences

- `transaction_spend_magnitude` in `spending_report.rs` is now `pub(crate)` (minimal
  surface change; the function's documented contract is unchanged).
- A new `spending_history` tool appears in the MCP tool list.
- Both `spending_report` and `spending_history` are guaranteed to agree on exclusion rules
  because they call the same underlying function.
- The fixed/discretionary taxonomy uses whole-word token matching. A category like
  "Camera Rental" does NOT match `"rent"` because "rental" is not the same token as
  "rent". Categories whose name IS the bare word "Rent" or contains "Rent" as a whole
  token (e.g. "Rent Payment") correctly classify as fixed. The `months` parameter is
  clamped to a minimum of 1 inside `range_for_months_count` to prevent u32 underflow
  when callers pass 0.
- Explicit `start_date`/`end_date` inputs are validated via `resolve_history_range`
  before any API call. Malformed dates (e.g. `"garbage"`, `"2026-13-01"`), reversed
  ranges (start > end), and partial inputs (only one of `start_date`/`end_date`
  provided) return a soft `{"error": "..."}` payload rather than silently falling back
  to the months default. This prevents the silent-degradation failure mode: a caller
  who provides only `start_date` gets an error, not a quietly-ignored bound.
- Fixed-category token matching tolerates simple English plural forms: a category token
  matches a pattern token when they are equal OR the category token equals the pattern
  token + "s". This makes `"Student Loans"` (a default Monarch category) and
  `"Loans"`, `"Insurances"`, `"Mortgages"` classify as FIXED, while the run #1
  false-positive guard is preserved: `"rentals"` stripped to `"rental"` does not equal
  `"rent"`, so `"Concert Rentals"` remains DISCRETIONARY.

## Alternatives Considered

**Exact category enumeration.** Rejected: Monarch category names are user-editable; any
fixed list would break on rename.

**Return raw transactions with a large limit.** Rejected: overflows LLM context window at
scale; the purpose of a compound tool is to compute server-side.

**Use spending_report N times.** Rejected: requires N API calls for N months, no fixed/
discretionary split, no outlier surfacing, imposes per-call budget fetching overhead.
