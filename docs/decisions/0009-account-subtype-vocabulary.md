# ADR 0009 — Account Type/Subtype Vocabulary and Inventory Bucketing

**Status:** Accepted  
**Issue:** [#50 — account_inventory tool](https://github.com/mikelane/monarch-mcp/issues/50)  
**Date:** 2026-06-07

## Context

The `account_inventory` tool groups the household's accounts into retirement-planning buckets.
Bucketing requires knowing Monarch's actual `type.name` and `subtype.name` string values for
each account. These values are NOT documented by Monarch and are NOT guessable — prior issues
(ADR 0002, 0003) show that invented schema strings cause false-green tests that only fail on
live data.

Real values were captured via a gated `MONARCH_LIVE=1` exploration test (`explore_account_subtype_vocabulary`)
that called `GetAccounts` against the production Monarch API and printed each account's
`type.name`, `subtype.name`, `subtype.display`, `currentBalance`, and `isHidden` fields.

## Discovered Vocabulary (captured 2026-06-07 from real Monarch API)

The exploration run covered a multi-account household. The `type.name` / `subtype.name` /
`subtype.display` strings below are the real Monarch schema values; the "Example" column uses
**synthetic placeholder** account names only (per the repo's data-hygiene policy — never commit
real account names, numbers, balances, or institution-identifying detail).

### `type = "depository"` (cash accounts)
| subtype.name | subtype.display | Example account (synthetic) |
|---|---|---|
| `checking` | `Checking` | Primary Checking, Secondary Checking |
| `savings` | `Savings` | Emergency Fund, Savings |
| `paypal` | `Mobile Payment System` | Mobile Payment Account |

Note: One account classified by Monarch as `type="depository"` `subtype="savings"` is treated
as cash (depository), not tax-advantaged, even when the account's user-given name suggests a
retirement vehicle — we follow Monarch's own data-layer classification, not the display name.

### `type = "brokerage"` (investment accounts — refined by subtype)
| subtype.name | subtype.display | Bucket | Example (synthetic) |
|---|---|---|---|
| `brokerage` | `Brokerage (Taxable)` | taxable_brokerage | Taxable Brokerage, Joint Brokerage |
| `stock_plan` | `Stock Plan` | taxable_brokerage | Employer RSU Plan |
| `health_savings_account` | `Health Savings Account (HSA)` | tax_advantaged | Health Savings Account |
| `st_401k` | `401k` | tax_advantaged | Employer 401k |
| `roth` | `Roth IRA` | tax_advantaged | Roth IRA Brokerage |

### `type = "credit"` (liabilities)
| subtype.name | subtype.display | Example (synthetic) |
|---|---|---|
| `credit_card` | `Credit Card` | Rewards Card, Store Card |

### `type = "loan"` (liabilities)
| subtype.name | subtype.display | Example (synthetic) |
|---|---|---|
| `other` | `Other` | Mortgage, Personal Loan |
| `line_of_credit` | `Line of Credit` | Line Of Credit |

### `type = "vehicle"` (other assets)
| subtype.name | subtype.display | Example (synthetic) |
|---|---|---|
| `car` | `Car` | Family Car, Second Car |

## Bucketing Decision

Five buckets, applied in order of precision (type + subtype → type fallback):

| Bucket | `type` | `subtype.name` (if applicable) | Sign |
|---|---|---|---|
| `taxable_brokerage` | `brokerage` | `brokerage`, `stock_plan`, or unrecognized | positive (asset) |
| `tax_advantaged` | `brokerage` | `health_savings_account`, `st_401k`, `roth` | positive (asset) |
| `cash` | `depository` | any | positive (asset) |
| `other_assets` | `vehicle` | any | positive (asset) |
| `liabilities` | `credit`, `loan` | any | negative (liability) |

**Fallback rule:** Any `brokerage` account whose subtype is `None` or unrecognized falls into
`taxable_brokerage` (the more conservative bucket — taxable treatment is the safer assumption).
Any other account with an unrecognized type falls into `other_assets` if its balance is
positive, or `liabilities` if its balance is negative.

**Two-way door:** Every output `AccountEntry` carries the raw `type_name` and
`subtype_name` strings so callers can see the exact classification basis. Accounts whose
subtype is not in the recognized set are flagged with `unknown_subtype: true`. This lets the
bucket map be extended without blocking on a code change.

## Nullable Fields (ADR 0003 compliance)

- `subtype`: nullable — Monarch returns `null` for some manually-tracked accounts. The
  `Account` struct captures this as `Option<AccountSubtype>`. When `None`, the type-level
  fallback bucket applies.
- `currentBalance`: already nullable (ADR 0003). When `null`, the client maps to `0.0` and
  the inventory flags the account with `balance_unknown: true` (stale/unsynced).
- `isHidden`: non-nullable boolean. Hidden accounts are included in output but flagged so
  callers can exclude them from display or calculations.

## Consequences

- The bucket map is built from captured real values — not guesswork. Any subtype Monarch
  introduces in future is handled by the fallback path and surfaced via `unknown_subtype: true`.
- The vocabulary must be re-validated if Monarch changes their subtype strings (a breaking
  change on their side). The large live test (`account_inventory_returns_valid_structure`) will
  catch such regressions.
- Institution-level grouping (Vanguard / Fidelity labels) is Phase 2 — `GetAccounts` does not
  return institution data and it is not documented in our ADRs.
