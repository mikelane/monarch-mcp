# ADR 0014 — Asset Allocation Taxonomy and Investable-Class Contract

**Status:** Accepted  
**Issue:** [#67 — asset_allocation tool](https://github.com/mikelane/monarch-mcp/issues/67)  
**Date:** 2026-06-10

## Context

`account_inventory` (#50) groups accounts by **tax treatment** (tax-advantaged / taxable /
cash / other / liabilities). `asset_allocation` is the orthogonal lens: **asset class**
(equities / cash / real_estate / crypto / other_assets / liabilities / other). The same
`GetAccounts` data source, a different grouping axis.

`retirement_readiness` (#69) needs a clean "investable vs. liquid" split — specifically,
which accounts count toward the investable portfolio and which do not. This ADR documents
the asset-class taxonomy and codifies the `is_investable()` contract that #69 will consume.

## Asset-Class Taxonomy

Classification key: `account.account_type.name` (primary), then `account.subtype.name`
for `brokerage` accounts (secondary). All string values are captured from real Monarch
API responses (see ADR 0009 for the full vocabulary). Classification logic lives in
`src/asset_allocation.rs` as the single source of truth.

| Asset class    | `type.name`          | `subtype.name`                   | Sign      |
|----------------|----------------------|----------------------------------|-----------|
| `equities`     | `brokerage`          | any (see API limitation below)   | positive  |
| `cash`         | `depository`         | any                              | positive  |
| `real_estate`  | `real_estate`        | any                              | positive  |
| `crypto`       | `crypto`             | any                              | positive  |
| `other_assets` | `vehicle`            | any                              | positive  |
| `liabilities`  | `credit` or `loan`   | any                              | negative  |
| `other`        | anything unrecognized| —                                | varies    |

### Unrecognized subtype handling

An **unrecognized `brokerage` subtype** (not in the known vocabulary from ADR 0009) still
maps to `equities` — the conservative choice for any investment account is "invested, not
unknown." However, the `equities` class is flagged `recognized: false` so callers know the
vocabulary may need updating.

An **unrecognized `type`** (e.g., `collectible`) maps to `other` and is always flagged
`recognized: false`. This mirrors `account_inventory`'s honesty about unknown subtypes:
the tool never silently drops an account, and flags the uncertainty explicitly.

## API Limitation — Equity vs. Bond Within an Account

Monarch's `GetAccounts` API does **not** return per-holding data. A brokerage account
that is partially or fully invested in bonds is indistinguishable from one that is 100%
equity at the account-type level. Consequently:

- All `brokerage` accounts (401k, Roth IRA, taxable brokerage, HSA, stock plan) are
  classified as `equities`.
- The equity/bond breakdown within an account is **not available** from this API.
- The tool's output includes a `note` field documenting this limitation explicitly.

A future enhancement could use a holdings API (if Monarch exposes one) to refine the
classification. For now, "all brokerage = equities" is documented and surfaced to callers.

## Monarch Sign Convention

Asset balances are positive; liability balances are negative.

- `gross_assets` = sum of all positive-balance asset-class totals (excludes liabilities).
- `total_liabilities` = signed sum of the `liabilities` class (typically negative).
- `net_worth` = `gross_assets` + `total_liabilities` = signed sum of all account balances.
- Liabilities are excluded from the percentage base and reported with `percent_of_assets: null`.
- The `net_worth` from `asset_allocation` must equal the `net_worth` from `financial_overview`
  (both aggregate the same `GetAccounts` slice) — this is the **trust cross-check** validated
  in the large integration test.

## Hidden Account Handling

Hidden accounts (`isHidden: true`) are **included** in all totals, matching
`account_inventory`'s treatment. The asset-class view is a complete balance-sheet picture;
hidden accounts still have real balances. Callers who want to exclude hidden accounts must
filter the result themselves.

## Investable-Class Contract (for `retirement_readiness` #69)

`retirement_readiness` (#69) needs to identify the "investable portfolio" — accounts whose
balance counts toward retirement savings. The contract is expressed via two public functions
in `src/asset_allocation.rs`:

### `AssetClass::is_investable() -> bool`

```rust
pub fn is_investable(self) -> bool {
    matches!(self, AssetClass::Equities | AssetClass::RealEstate)
}
```

| Asset class    | `is_investable()` | Rationale                                      |
|----------------|:-----------------:|------------------------------------------------|
| `equities`     | **true**          | Core retirement vehicle (401k, Roth, brokerage)|
| `real_estate`  | **true**          | Long-term investment with compounding value    |
| `cash`         | false             | Liquid buffer; held outside investment accounts|
| `crypto`       | false             | Speculative; treated as separate from portfolio|
| `other_assets` | false             | Illiquid tangible property (vehicles, etc.)    |
| `liabilities`  | false             | Debt; subtracted from net worth, not invested  |
| `other`        | false             | Unknown type; cannot assume investable          |

### `investable_accounts(accounts: &[Account]) -> Vec<&Account>`

Returns only the accounts whose `classify_asset_class()` result satisfies `is_investable()`.
`retirement_readiness` (#69) calls this function to compute the investable portfolio total
without duplicating the classification logic.

### `classify_asset_class(account: &Account) -> (AssetClass, bool)`

The single source of truth for the asset-class taxonomy. Both `compute_asset_allocation`
and `retirement_readiness` call this function — the classification is never duplicated.
Returns `(class, recognized)` where `recognized` is `false` when the account's type or
subtype was not in the known vocabulary.

## Consequences

- The classification map is built from captured real values (ADR 0009) — not guesswork.
- Any future Monarch type/subtype addition falls through to `other` or the `equities`
  unrecognized path, surfacing via `recognized: false`. The large integration test will
  warn when this occurs.
- `retirement_readiness` (#69) must import `investable_accounts` and `is_investable` from
  `asset_allocation.rs` — not re-implement the classification logic.
- The equity/bond limitation is a known gap. If Monarch ever exposes per-holding data,
  the `equities` class can be split without breaking the `is_investable()` contract
  (equities + bonds are both investable).
- `net_worth` agreement between `asset_allocation` and `financial_overview` is validated
  in the large integration test as a regression guard for classification bugs.
