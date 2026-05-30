# ADR 0003 — Tier-2 Monarch Money GraphQL Schema (Epic B)

**Status:** Accepted  
**Date:** 2026-05-29  
**Context:** This ADR documents the real GraphQL operations and response shapes needed for the
three Epic-B tools: `cashflow_forecast`, `net_worth_trend`, and `recurring_scan`. All operations
were validated by HTTP 200 calls to `https://api.monarch.com/graphql` using the proven auth
headers from ADR 0001. Raw captures (with real values) written to `/tmp/` only — never committed.

---

## Background

Epic B requires three compound tools that go beyond the Tier-1 set:

1. **cashflow_forecast** — projects month-end position from recurring bills, income timing, and
   current balances. Inputs: `Web_GetCashFlowPage` (existing, ADR 0002) for period summaries +
   `Web_GetUpcomingRecurringTransactionItems` (new) for scheduled future charges.
2. **net_worth_trend** — net worth over time with delta by account type and biggest movers.
   Inputs: `GetSnapshotsByAccountType` (new) — returns monthly balances per account type.
3. **recurring_scan** — detect new/changed recurring charges, list upcoming renewals, avoid
   false-flagging stable subscriptions. Input: `Web_GetUpcomingRecurringTransactionItems` (new).

The `Web_GetCashFlowPage` and `GetAggregateSnapshots` operations were already documented in ADR
0002. This ADR documents the two new operations not previously captured.

---

## Auth headers (unchanged from ADR 0001/0002)

```
POST https://api.monarch.com/graphql
Authorization: Token <64-char token from ~/.config/monarch-mcp/session.json>
Client-Platform: web
device-uuid: <random UUID per session>
Origin: https://app.monarch.com
User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 …
Accept: application/json
Content-Type: application/json
```

---

## New Operations

### 1. Web_GetUpcomingRecurringTransactionItems

Used by: `cashflow_forecast` (upcoming bill amounts + dates), `recurring_scan` (detect changes).

**Source:** `monarchmoney` library —
`get_recurring_transactions()` in `monarchmoney.py` (confirmed at
`/Users/mikelane/.cache/uv/archive-v0/.../monarchmoney/monarchmoney.py`).

```graphql
query Web_GetUpcomingRecurringTransactionItems(
  $startDate: Date!,
  $endDate: Date!,
  $filters: RecurringTransactionFilter
) {
  recurringTransactionItems(
    startDate: $startDate
    endDate: $endDate
    filters: $filters
  ) {
    stream {
      id
      frequency
      amount
      isApproximate
      merchant {
        id
        name
        logoUrl
        __typename
      }
      __typename
    }
    date
    isPast
    transactionId
    amount
    amountDiff
    category {
      id
      name
      __typename
    }
    account {
      id
      displayName
      logoUrl
      __typename
    }
    __typename
  }
}
```

**Variables:**
```json
{
  "startDate": "YYYY-MM-DD",
  "endDate": "YYYY-MM-DD"
}
```

**Response shape (no real values):**
```json
{
  "data": {
    "recurringTransactionItems": [
      {
        "stream": {
          "id": "<string>",
          "frequency": "<string>",
          "amount": "<float>",
          "isApproximate": "<bool>",
          "merchant": {
            "id": "<string>",
            "name": "<string>",
            "logoUrl": "<string|null>",
            "__typename": "RecurringTransactionStream"
          },
          "__typename": "RecurringTransactionStream"
        },
        "date": "<string YYYY-MM-DD>",
        "isPast": "<bool>",
        "transactionId": "<string|null>",
        "amount": "<float>",
        "amountDiff": "<float|null>",
        "category": {
          "id": "<string>",
          "name": "<string>",
          "__typename": "Category"
        },
        "account": {
          "id": "<string>",
          "displayName": "<string>",
          "logoUrl": "<string|null>",
          "__typename": "Account"
        },
        "__typename": "RecurringTransactionItem"
      }
    ]
  }
}
```

**Key field semantics:**
- `stream.frequency`: string enum from Monarch — e.g. `"monthly"`, `"annually"`, `"weekly"`.
- `stream.amount`: the stream's expected/canonical amount (negative for outflows, positive for
  income, following Monarch sign convention).
- `stream.isApproximate`: `true` when Monarch inferred the amount from history rather than a
  fixed schedule. Stable subscriptions have `false`.
- `amountDiff`: how much the actual transaction amount differed from `stream.amount` on the most
  recent occurrence. A large non-zero `amountDiff` is the "creeping charge" signal.
- `isPast`: `true` when the item's `date` is in the past (the charge already occurred this period).
- `transactionId`: ID of the matched transaction when `isPast=true`; `null` for future items.

**Validated:** YES — 4 recurring items returned, HTTP 200.

---

### 2. GetSnapshotsByAccountType

Used by: `net_worth_trend` (monthly net worth per account type, delta, biggest movers).

**Source:** `monarchmoney` library — `get_account_snapshots_by_type()` in `monarchmoney.py`.

```graphql
query GetSnapshotsByAccountType($startDate: Date!, $timeframe: Timeframe!) {
  snapshotsByAccountType(startDate: $startDate, timeframe: $timeframe) {
    accountType
    month
    balance
    __typename
  }
  accountTypes {
    name
    group
    __typename
  }
}
```

**Variables:**
```json
{
  "startDate": "YYYY-MM-DD",
  "timeframe": "month"
}
```

(`timeframe` is a GraphQL enum: `"month"` or `"year"`.)

**Response shape (no real values):**
```json
{
  "data": {
    "snapshotsByAccountType": [
      {
        "accountType": "<string>",
        "month": "<string YYYY-MM>",
        "balance": "<float>",
        "__typename": "AccountTypeSnapshot"
      }
    ],
    "accountTypes": [
      {
        "name": "<string>",
        "group": "<string>",
        "__typename": "AccountType"
      }
    ]
  }
}
```

**Key field semantics:**
- `accountType`: lowercase string from a fixed enum — validated values include: `"depository"`,
  `"brokerage"`, `"credit"`, `"loan"`, `"vehicle"`, and others.
- `month`: `"YYYY-MM"` format (NOT a full ISO date — no day part). Represents the end-of-month
  snapshot for that calendar month.
- `balance`: total balance across all accounts of that type at end of month. Negative for
  liabilities (credit, loan). Positive for assets (depository, brokerage).
- `accountTypes`: the full type catalog returned alongside snapshots — useful for labelling
  `group` (e.g. `"asset"` vs `"liability"`).
- The flat list must be grouped client-side by `accountType` to produce per-type series.

**Validated:** YES — 25 rows (5 account types × 5 months), 11 `accountTypes` entries, HTTP 200.

---

## Re-use of existing operations (ADR 0002)

These operations from ADR 0002 are also used by Epic-B tools without modification:

| Tool | Existing operations re-used | Purpose |
|---|---|---|
| `cashflow_forecast` | `Web_GetCashFlowPage` | period income/expense summary |
| `cashflow_forecast` | `GetAccounts` | current balances for projected position |
| `net_worth_trend` | `GetAggregateSnapshots` | daily total net worth series (falls back when per-type not needed) |
| `recurring_scan` | (none beyond the new op) | — |

---

## Summary — new operation → field mapping for Epic-B tools

| Tool | Operation | Key output fields |
|---|---|---|
| `cashflow_forecast` | `Web_GetUpcomingRecurringTransactionItems` | `date`, `amount`, `stream.frequency`, `isPast` |
| `cashflow_forecast` | `Web_GetCashFlowPage` (ADR 0002) | `summary[0].summary.{sumIncome,sumExpense}` |
| `cashflow_forecast` | `GetAccounts` (ADR 0002) | `currentBalance` per account |
| `net_worth_trend` | `GetSnapshotsByAccountType` | `accountType`, `month`, `balance` grouped by type |
| `recurring_scan` | `Web_GetUpcomingRecurringTransactionItems` | `amountDiff`, `stream.isApproximate`, `stream.amount`, `date` |

## Sources

- `monarchmoney` Python library installed at
  `/Users/mikelane/.cache/uv/archive-v0/vLK_UHjrOZ-WX2qar3Lg5/lib/python3.10/site-packages/monarchmoney/monarchmoney.py`
- `.tools/monarch-mcp-server/src/monarch_mcp_server/tools/transactions.py` (recurring tool usage)
- `.tools/monarch-mcp-server/src/monarch_mcp_server/tools/financial.py` (net worth usage)
- Live HTTP 200 validation against `https://api.monarch.com/graphql`
- Raw captures (with real values) in `/tmp/recurring_raw.json`, `/tmp/snapshots_by_type_raw.json`,
  `/tmp/cashflow_3month_raw.json` — not committed
