# ADR 0016 — Retirement Readiness: Safe-Withdrawal-Rate Model

**Status:** Accepted  
**Issue:** [#69 — retirement_readiness tool](https://github.com/mikelane/monarch-mcp/issues/69)  
**Date:** 2026-06-10

## Context

The README and issue #69 name "retirement-spending analysis" as the north-star use case for
this server. The prior tools built the necessary primitives:

- `asset_allocation` (#67) classifies accounts by asset class and exposes `classify_asset_class`.
- `spending_history` / `savings_rate` (#65) computes true spending via `compute_true_spending`.
- `retirement_readiness` (#69) is the synthesis: it combines the investable portfolio with the
  annualised spending baseline into a safe-withdrawal-rate coverage check.

## Safe-Withdrawal-Rate Model

The 4% rule (Bengen 1994) states that a retiree can withdraw 4% of their initial portfolio
per year with high historical probability of not outliving their savings over 30 years.
The arithmetic is:

```
sustainable_annual_withdrawal = invested_assets × withdrawal_rate
coverage_ratio                = sustainable_annual_withdrawal / annual_baseline_spend
target_portfolio              = annual_baseline_spend / withdrawal_rate   (25× at 4%)
surplus_or_gap                = invested_assets − target_portfolio
```

### Withdrawal Rate Parameter

| Parameter              | Value  | Rationale                                   |
|------------------------|--------|---------------------------------------------|
| Default rate           | 0.04   | 4% rule — the most widely cited benchmark   |
| Minimum allowed rate   | 0.02   | Below 2% is effectively a savings account   |
| Maximum allowed rate   | 0.10   | Above 10% is historically reckless          |

Rates outside `[0.02, 0.10]` are rejected with `MonarchError::InvalidInput` before any
network I/O. The rate is echoed in every response as `withdrawal_rate_used` so the output
is self-interpreting (ADR post-mortem lesson: "numbers the user can trust").

## Invested-Assets Definition

### Why Narrower Than ADR 0014's `is_investable()`

ADR 0014's `is_investable()` marks **equities + real_estate** as investable because both
are long-term investment vehicles. However, for a **safe-withdrawal-rate portfolio**,
standard financial planning convention excludes primary-residence real estate because:

1. **Illiquidity**: A home cannot be drawn down annually like a brokerage account.
2. **Consumption vs. investment**: The primary residence provides shelter, not cash flow.
3. **SWR convention**: The classic Bengen / Trinity Study portfolios are stock+bond
   portfolios. Real estate is modelled separately in retirement planning.

`retirement_readiness` therefore defines the investable portfolio as:

| Asset class    | Included in SWR base? | Rationale                              |
|----------------|:---------------------:|----------------------------------------|
| `equities`     | **yes**               | Liquid invested financial assets       |
| `real_estate`  | **no**                | Illiquid; excluded per SWR convention  |
| `cash`         | no                    | Liquid buffer; held outside portfolio  |
| `crypto`       | no                    | Speculative; managed separately        |
| `other_assets` | no                    | Vehicles and tangible property         |
| `liabilities`  | no                    | Debt; not a portfolio asset            |
| `other`        | no                    | Unknown type; cannot assume investable |

### Implementation: No Reimplementation of Classification

The helper `invested_financial_accounts(accounts)` in `src/retirement_readiness.rs`
delegates entirely to `classify_asset_class` from `asset_allocation.rs` — no classification
logic is duplicated. The filter is a one-liner:

```rust
class == AssetClass::Equities
```

This is intentionally **narrower** than `investable_accounts` (ADR 0014), which includes
`RealEstate`. Both helpers exist as the single source of truth for their respective
definitions. Callers that need the broader "investable + real_estate" view use
`investable_accounts`; callers that need the SWR-specific "liquid financial assets only"
view use `invested_financial_accounts`.

### API Limitation (Inherited from ADR 0014)

Monarch's `GetAccounts` API does not return per-holding data. All brokerage accounts
(401k, Roth, taxable, HSA, stock plan) are classified as `equities`. The equity/bond
breakdown within an account is not available. This limitation is documented in the tool's
response via the `invested_assets_note` field.

## Spend-Annualization Window

The annualised baseline spend is derived from `compute_true_spending` over a trailing
N-month window of complete calendar months:

```
annual_baseline_spend = (compute_true_spending(transactions) / N) × 12
```

| Parameter          | Default | Range    | Rationale                                    |
|--------------------|---------|----------|----------------------------------------------|
| `months` (window)  | 6       | 1–24     | 6 months smooths seasonal variation; 24 is   |
|                    |         |          | the max meaningful trailing window for most  |
|                    |         |          | households                                   |

The same `range_for_months_count` / `resolve_history_range` logic used by
`spending_history` and `savings_rate` is reused here — the spend window is always
trailing complete months, never a partial current month. The window length is echoed
in the response as `spend_window_months`.

`compute_true_spending` (from `spending_report.rs`) excludes income, transfers, and
credit-card payments — the same exclusion rules used by every other spend number in
the server. This ensures the spending baseline is consistent with `spending_history`
and `savings_rate` for the same transaction slice.

## Zero Guards

| Condition                         | Result                         | Rationale              |
|-----------------------------------|--------------------------------|------------------------|
| `annual_baseline_spend == 0.0`    | `coverage_ratio = None`        | Avoids ÷0 / NaN        |
|                                   | `target_portfolio = None`      |                        |
|                                   | `surplus_or_gap = None`        |                        |
| `withdrawal_rate` out of range    | `MonarchError::InvalidInput`   | Validated before I/O   |
| `invested_assets == 0.0`          | `coverage_ratio = Some(0.0)`   | Mathematically correct |

## Surfacing Assumptions

Every response includes three transparency fields (the post-mortem's "numbers the user
can trust" lesson):

- `withdrawal_rate_used` — the rate applied (default or caller-supplied)
- `spend_window_months` — number of complete months used for the baseline
- `invested_assets_note` — human-readable description of what counts as "invested"

## Consequences

- The tool reuses `classify_asset_class` (#67) and `compute_true_spending` (#65) —
  no classification or spending logic is duplicated.
- `invested_assets` agrees with `asset_allocation`'s `equities` class total for the
  same account slice. This cross-check is validated in the large integration test.
- Real estate accounts (primary home, rental properties) are visible in
  `asset_allocation` but NOT in `retirement_readiness`'s invested base. Users who
  want to include real estate equity in their SWR calculation should be directed to
  `asset_allocation` for the broader picture and `retirement_readiness` for the
  liquid-portfolio-only SWR number.
- Future enhancement: if Monarch ever exposes per-holding data, `equities` can be
  split into equity + bond sub-classes without changing the `invested_financial_accounts`
  filter — both sub-classes would remain `AssetClass::Equities` until the taxonomy
  is expanded.
