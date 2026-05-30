# ADR 0002 — Real Monarch Money GraphQL Schema

**Status:** Accepted  
**Date:** 2026-05-29  
**Context:** ADR 0001 proved authentication. This ADR documents the real GraphQL operations and
response shapes validated against the live Monarch API, replacing the invented fiction in the
original `src/client.rs`.

---

## Background

The original client used fabricated queries (`GetCashflow { cashflow { income spending } }`,
`GetNetWorthHistory { netWorthHistory { … } }`, `GetBudgets { budgets { … } }`, etc.) that
do not exist in Monarch's real schema. The first live call returned a GraphQL error:
`field 'cashflow' does not exist on type 'Query'`. This ADR documents the real operations
lifted from the `monarchmoneycommunity` open-source client (robcerda/bradleyseanf forks),
validated by HTTP 200 calls to `https://api.monarch.com/graphql` using the proven auth headers
from ADR 0001.

---

## Working auth headers (from ADR 0001)

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

## Operations — real queries, shapes, and variables

### 1. GetAccounts

**Operation:** `GetAccounts`

```graphql
query GetAccounts {
  accounts {
    id
    displayName
    currentBalance
    isHidden
    type {
      name
      display
      __typename
    }
    subtype {
      name
      display
      __typename
    }
    __typename
  }
}
```

**Response shape (no real values):**
```json
{
  "data": {
    "accounts": [
      {
        "id": "<string>",
        "displayName": "<string>",
        "currentBalance": "<float>",
        "isHidden": "<bool>",
        "type": { "name": "<string>", "display": "<string>", "__typename": "AccountType" },
        "subtype": { "name": "<string>", "display": "<string>", "__typename": "AccountSubtype" },
        "__typename": "Account"
      }
    ]
  }
}
```

**Validated:** YES — 37 accounts returned, HTTP 200.

---

### 2. GetTransactionsList

**Operation:** `GetTransactionsList`

```graphql
query GetTransactionsList(
  $offset: Int,
  $limit: Int,
  $filters: TransactionFilterInput,
  $orderBy: TransactionOrdering
) {
  allTransactions(filters: $filters) {
    totalCount
    results(offset: $offset, limit: $limit, orderBy: $orderBy) {
      id
      amount
      pending
      date
      hideFromReports
      plaidName
      notes
      isRecurring
      reviewStatus
      needsReview
      isSplitTransaction
      createdAt
      updatedAt
      category {
        id
        name
        __typename
      }
      merchant {
        name
        id
        transactionsCount
        __typename
      }
      account {
        id
        displayName
        __typename
      }
      tags {
        id
        name
        color
        order
        __typename
      }
      __typename
    }
    __typename
  }
}
```

**Variables:**
```json
{
  "offset": 0,
  "limit": 100,
  "orderBy": "date",
  "filters": {
    "search": "",
    "categories": [],
    "accounts": [],
    "tags": [],
    "startDate": "YYYY-MM-DD",
    "endDate": "YYYY-MM-DD"
  }
}
```

**Response shape:**
```json
{
  "data": {
    "allTransactions": {
      "totalCount": "<int>",
      "results": [
        {
          "id": "<string>",
          "amount": "<float>",
          "pending": "<bool>",
          "date": "<string YYYY-MM-DD>",
          "hideFromReports": "<bool>",
          "plaidName": "<string|null>",
          "notes": "<string|null>",
          "isRecurring": "<bool>",
          "reviewStatus": "<string|null>",
          "needsReview": "<bool>",
          "isSplitTransaction": "<bool>",
          "createdAt": "<string>",
          "updatedAt": "<string>",
          "category": { "id": "<string>", "name": "<string>", "__typename": "Category" },
          "merchant": { "name": "<string>", "id": "<string>", "transactionsCount": "<int>", "__typename": "Merchant" },
          "account": { "id": "<string>", "displayName": "<string>", "__typename": "Account" },
          "tags": [],
          "__typename": "Transaction"
        }
      ],
      "__typename": "TransactionList"
    }
  }
}
```

**Key difference from fiction:** Root field is `allTransactions`, not `transactions`.
Results are under `allTransactions.results`, not at the top level.
Filter for needsReview uses `"needsReview": true` in the `filters` object.

**Validated:** YES — 317 transactions total, HTTP 200.

---

### 3. Web_GetCashFlowPage (replaces invented `GetCashflow`)

**Operation:** `Web_GetCashFlowPage`

```graphql
query Web_GetCashFlowPage($filters: TransactionFilterInput) {
  byCategory: aggregates(filters: $filters, groupBy: ["category"]) {
    groupBy {
      category {
        id
        name
        group {
          id
          type
          __typename
        }
        __typename
      }
      __typename
    }
    summary {
      sum
      __typename
    }
    __typename
  }
  byCategoryGroup: aggregates(filters: $filters, groupBy: ["categoryGroup"]) {
    groupBy {
      categoryGroup {
        id
        name
        type
        __typename
      }
      __typename
    }
    summary {
      sum
      __typename
    }
    __typename
  }
  summary: aggregates(filters: $filters, fillEmptyValues: true) {
    summary {
      sumIncome
      sumExpense
      savings
      savingsRate
      __typename
    }
    __typename
  }
}
```

**Variables:**
```json
{
  "filters": {
    "startDate": "YYYY-MM-DD",
    "endDate": "YYYY-MM-DD",
    "search": "",
    "categories": [],
    "accounts": [],
    "tags": []
  }
}
```

**Response shape:**
```json
{
  "data": {
    "byCategory": [
      {
        "groupBy": {
          "category": {
            "id": "<string>",
            "name": "<string>",
            "group": { "id": "<string>", "type": "<string>", "__typename": "CategoryGroup" },
            "__typename": "Category"
          },
          "__typename": "AggregateGroupBy"
        },
        "summary": { "sum": "<float>", "__typename": "AggregateSummary" },
        "__typename": "Aggregate"
      }
    ],
    "byCategoryGroup": [ "... same shape with categoryGroup ..." ],
    "summary": [
      {
        "summary": {
          "sumIncome": "<float>",
          "sumExpense": "<float>",
          "savings": "<float>",
          "savingsRate": "<float>",
          "__typename": "AggregateSummary"
        },
        "__typename": "Aggregate"
      }
    ]
  }
}
```

**Key difference from fiction:** There is NO `cashflow` root field. Income/expense are under
`summary[0].summary.sumIncome` and `summary[0].summary.sumExpense`. By-category breakdown
is under `byCategory[].groupBy.category` with `summary.sum`. The `sum` field is negative for
expenses (matches Monarch's sign convention).

**Validated:** YES — 35 categories, HTTP 200.

---

### 4. GetAggregateSnapshots (replaces invented `GetNetWorthHistory`)

**Operation:** `GetAggregateSnapshots`

```graphql
query GetAggregateSnapshots($filters: AggregateSnapshotFilters) {
  aggregateSnapshots(filters: $filters) {
    date
    balance
    __typename
  }
}
```

**Variables:**
```json
{
  "filters": {
    "startDate": "YYYY-MM-DD",
    "endDate": "YYYY-MM-DD"
  }
}
```

**Response shape:**
```json
{
  "data": {
    "aggregateSnapshots": [
      {
        "date": "<string YYYY-MM-DD>",
        "balance": "<float>",
        "__typename": "AggregateSnapshot"
      }
    ]
  }
}
```

**Key difference from fiction:** Field is `aggregateSnapshots` (not `netWorthHistory`).
Each item has `date` and `balance` (not `priorMonthNetWorth`). Returns daily snapshots —
to get prior-month net worth, take the last entry from the prior month's date range.

**Validated:** YES — 59 snapshots, HTTP 200.

---

### 5. GetCategories

**Operation:** `GetCategories`

```graphql
query GetCategories {
  categories {
    id
    order
    name
    systemCategory
    isSystemCategory
    isDisabled
    updatedAt
    createdAt
    group {
      id
      name
      type
      __typename
    }
    __typename
  }
}
```

**Response shape:**
```json
{
  "data": {
    "categories": [
      {
        "id": "<string>",
        "order": "<int>",
        "name": "<string>",
        "systemCategory": "<string|null>",
        "isSystemCategory": "<bool>",
        "isDisabled": "<bool>",
        "updatedAt": "<string>",
        "createdAt": "<string>",
        "group": { "id": "<string>", "name": "<string>", "type": "<string>", "__typename": "CategoryGroup" },
        "__typename": "Category"
      }
    ]
  }
}
```

**Validated:** YES — 62 categories, HTTP 200.

---

### 6. GetHouseholdTransactionTags (replaces invented `GetTags`)

**Operation:** `GetHouseholdTransactionTags`

```graphql
query GetHouseholdTransactionTags(
  $search: String,
  $limit: Int,
  $bulkParams: BulkTransactionDataParams
) {
  householdTransactionTags(
    search: $search
    limit: $limit
    bulkParams: $bulkParams
  ) {
    id
    name
    color
    order
    transactionCount
    __typename
  }
}
```

**Variables:** `{}` (all optional)

**Response shape:**
```json
{
  "data": {
    "householdTransactionTags": [
      {
        "id": "<string>",
        "name": "<string>",
        "color": "<string hex>",
        "order": "<int>",
        "transactionCount": "<int>",
        "__typename": "TransactionTag"
      }
    ]
  }
}
```

**Key difference from fiction:** Field is `householdTransactionTags` not `tags`.

**Validated:** YES — 5 tags, HTTP 200.

---

### 7. GetJointPlanningData (replaces invented `GetBudgets`)

**Operation:** `GetJointPlanningData`

```graphql
query GetJointPlanningData($startDate: Date!, $endDate: Date!) {
  budgetData(startMonth: $startDate, endMonth: $endDate) {
    monthlyAmountsByCategory {
      category {
        id
        __typename
      }
      monthlyAmounts {
        month
        plannedCashFlowAmount
        actualAmount
        remainingAmount
        __typename
      }
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

**Response shape:**
```json
{
  "data": {
    "budgetData": {
      "monthlyAmountsByCategory": [
        {
          "category": { "id": "<string>", "__typename": "Category" },
          "monthlyAmounts": [
            {
              "month": "<string YYYY-MM-DD>",
              "plannedCashFlowAmount": "<float>",
              "actualAmount": "<float>",
              "remainingAmount": "<float>",
              "__typename": "BudgetMonthlyAmounts"
            }
          ],
          "__typename": "BudgetDataByCategory"
        }
      ],
      "__typename": "BudgetData"
    }
  }
}
```

**Key difference from fiction:** Field is `budgetData.monthlyAmountsByCategory`. Each entry
has `category.id` (not `category.name`) and `monthlyAmounts[].plannedCashFlowAmount` (not `amount`).
The `useV2Goals: Boolean!` parameter in the library causes a 400 — removed it.

**Validated:** YES — 61 category budgets, HTTP 200.

---

### 8. Common_UpdateTransactionMutation

**Operation:** `Common_UpdateTransactionMutation`

```graphql
mutation Common_UpdateTransactionMutation($input: UpdateTransactionMutationInput!) {
  updateTransaction(input: $input) {
    transaction {
      id
      notes
      category {
        id
        name
        __typename
      }
      __typename
    }
    errors {
      message
      __typename
    }
    __typename
  }
}
```

**Variables:**
```json
{
  "input": {
    "id": "<transaction UUID>",
    "notes": "<string>",
    "categoryId": "<category UUID>"
  }
}
```

**Response shape:**
```json
{
  "data": {
    "updateTransaction": {
      "transaction": {
        "id": "<string>",
        "notes": "<string>",
        "category": { "id": "<string>", "name": "<string>", "__typename": "Category" },
        "__typename": "Transaction"
      },
      "errors": null,
      "__typename": "UpdateTransactionPayload"
    }
  }
}
```

**Validated:** YES — mutation against a real transaction (notes only, no change), HTTP 200.

---

## Summary of fictitious → real field mapping

| Fictitious query | Real operation | Key shape change |
|---|---|---|
| `GetCashflow { cashflow { income spending } }` | `Web_GetCashFlowPage` | `summary[0].summary.{sumIncome,sumExpense}` |
| `GetNetWorthHistory { netWorthHistory { priorMonthNetWorth } }` | `GetAggregateSnapshots` | `aggregateSnapshots[].{date,balance}` |
| `GetBudgets { budgets { category { name } amount } }` | `GetJointPlanningData` | `budgetData.monthlyAmountsByCategory[].{category.id, monthlyAmounts[].plannedCashFlowAmount}` |
| `GetTransactions { transactions { … } }` | `GetTransactionsList` | `allTransactions.{totalCount, results[]}` |
| `GetTransactionsNeedingReview { transactionsNeedingReview { … } }` | `GetTransactionsList` with `filters.needsReview=true` | Same shape, filter-based not separate root field |
| `GetTags { tags { id name } }` | `GetHouseholdTransactionTags` | `householdTransactionTags[]` with extra color/order fields |
| `UpdateTransaction($id, $category, $tags, $notes)` | `Common_UpdateTransactionMutation($input: {id, …})` | Input wrapped in `{input: {id, categoryId, notes}}` |

## Sources

- `monarchmoneycommunity` Python library (installed at `.tools/monarch-mcp-server/.venv/lib/…/monarchmoney/monarchmoney.py`)
- `.tools/monarch-mcp-server/src/monarch_mcp_server/tools/` (robcerda's MCP server)
- Live validation: all operations confirmed HTTP 200 against `https://api.monarch.com/graphql`
- Real captures (with values) in `/tmp/` only — not committed
