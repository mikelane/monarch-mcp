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

The household has 37 accounts. Complete raw type/subtype vocabulary:

### `type = "depository"` (cash accounts)
| subtype.name | subtype.display | Example account |
|---|---|---|
| `checking` | `Checking` | Mike's Checking Account, Rebecca's Checking |
| `savings` | `Savings` | Emergency Fund, Mike's Savings, Robin Savings |
| `paypal` | `Mobile Payment System` | PayPal |

Note: One account ("IRA ...6382") appears as `type="depository"` `subtype="savings"` — this
is Monarch's own classification and is treated as cash (depository), not tax-advantaged.
The name says "IRA" but Monarch's data layer classifies it as a depository savings account.

### `type = "brokerage"` (investment accounts — refined by subtype)
| subtype.name | subtype.display | Bucket | Example |
|---|---|---|---|
| `brokerage` | `Brokerage (Taxable)` | taxable_brokerage | Individual - TOD, joint brokerage |
| `stock_plan` | `Stock Plan` | taxable_brokerage | AMAZON RSU |
| `health_savings_account` | `Health Savings Account (HSA)` | tax_advantaged | Health Savings Account |
| `st_401k` | `401k` | tax_advantaged | DATA.AI 401k, GD 401k, Amazon 401k |
| `roth` | `Roth IRA` | tax_advantaged | Roth IRA Brokerage Account |

### `type = "credit"` (liabilities)
| subtype.name | subtype.display | Example |
|---|---|---|
| `credit_card` | `Credit Card` | Delta Reserve, Apple Card, Costco Visa |

### `type = "loan"` (liabilities)
| subtype.name | subtype.display | Example |
|---|---|---|
| `other` | `Other` | Mortgage (WA 98660), Account (...6046) |
| `line_of_credit` | `Line of Credit` | Line Of Credit |

### `type = "vehicle"` (other assets)
| subtype.name | subtype.display | Example |
|---|---|---|
| `car` | `Car` | 2018 Mini Countryman, Sassy |

## Bucketing Decision

Five buckets, applied in order of precision (type + subtype → type fallback):

| Bucket | `type` | `subtype.name` (if applicable) | Sign |
|---|---|---|---|
| `taxable_brokerage` | `brokerage` | `brokerage`, `stock_plan` | positive (asset) |
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
