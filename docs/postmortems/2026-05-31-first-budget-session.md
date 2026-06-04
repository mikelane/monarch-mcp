# Post-mortem: first live budgeting session (2026-05-31)

**Author:** Claude (root agent), with Mike Lane
**Context:** First attempt to *use* monarch-mcp for real family budgeting (not build it). The
session pulled `financial_overview`, `spending_report`, `triage_uncategorized`, and
`progress_vs_goals` against the live Monarch account. We abandoned budgeting partway through
because the tool's output could not be trusted, and pivoted to writing this up for a follow-up
fix agent.

**Bottom line:** This is not "a few bugs." Every aggregate the server produced this session was
either wrong, misleading, or unusable, and the *user* caught it, not the tool. A budgeting
advisor whose first session produces numbers the user has to overrule spends trust instead of
building it. The single highest-value outcome of the fixes is: **numbers the user can believe
without re-deriving them by hand.**

All findings below are grounded in this session's actual tool output (Appendix A) and the
current source. File:line references are to `main` as of this writing.

---

## Severity summary

| # | Severity | Area | One-line |
|---|----------|------|----------|
| 1 | **Critical** | `spending_report` | Raw-amount summation with no sign handling → meaningless `total_spent`, negative `percent_of_budget`, and income flagged "over budget" while real overspending is hidden |
| 2 | **High** | `progress_vs_goals` | Hard-crashes when the goals file is absent — contradicts the module's own contract, the project CLAUDE.md, and is locked in by a test asserting the wrong behavior |
| 3 | **High** | missing capability | No way to list/drill into transactions in a category — blocked diagnosis of *every* anomaly this session |
| 4 | **High** | categorization | Re-categorizing an *already-categorized* transaction is impossible because no tool surfaces its id (the actual fix the user needed) — **same fix as #3** |
| 5 | **Medium** | `financial_overview` vs `spending_report` | The two tools compute "spending" from two different sources that disagree, and neither controls for transfer / credit-card-payment categories |
| 6 | **Medium** | refunds/reversals | A charge-then-refund (the vet) does not net or pair — it scatters across categories and inflates totals |
| 7 | **Medium (root cause)** | tests/mocks | Unit tests and mocks encode the **wrong sign convention** (positive expenses), which is *why* #1 shipped green. This is exactly the doubles-only hazard the project already documents |
| 8 | Low | `tools.rs` date helpers | `days_to_ymd` and `epoch_days_to_ymd` are duplicate implementations; `apply_changeset` fetches all transactions only to count them and never validates ids |

---

## Finding 1 — `spending_report` ignores Monarch's sign convention (CRITICAL)

### Symptoms (observed this session)
From the live `spending_report` (Appendix A):
- `total_spent: 2373.36` — a meaningless number. It nets **income** categories
  (`Paychecks: +29303.38`, `Business Income: +3406.26`, `Savings: +497.0`) against expense
  outflows. A "total spent" that includes paychecks is simply wrong.
- `over_budget_categories: ["Medical","Paychecks"]` — **Paychecks** (income!) is flagged as over
  budget, and **Medical** is flagged when it was actually a *refund inflow* (`spent: +3356.26`).
  Meanwhile the genuinely over-budget categories are **silently missing**: Mortgage (101%),
  Insurance (107%), Phone (112%), Fitness (133%), Taxes (118%).
- `percent_of_budget` is negative for every real expense: `Mortgage: -101`, `Insurance: -107`,
  `Phone: -112`, `Internet & Cable: -94`. Negative percentages are unreadable.

### Root cause
`aggregate_spending_by_category` sums raw `txn.amount` with no sign normalization and no
category-type filtering (`src/spending_report.rs:80-86`):

```rust
*totals.entry(txn.category.name.clone()).or_insert(0.0) += txn.amount;
```

In Monarch's real convention (CLAUDE.md "Domain conventions"; confirmed by this session's data)
**expenses are negative, income and refunds are positive.** So:

- `total_spent = by_category.values().sum()` (`:64`) sums income + expenses + transfers into one
  net figure mislabeled "total spent."
- `percent_of_budget(spent, budget)` computes `(spent / budget.abs()) * 100` (`:129-135`). With a
  negative `spent` (e.g. Mortgage `-3636.05`) and positive budget magnitude (`3600`), this yields
  `-101`. The function's doc comment and *all* its tests assume `spent` is **positive** — that
  assumption is false against real data.
- `find_over_budget_categories` checks `report.spent > budget.abs()` (`:148`). A negative `spent`
  can never exceed a positive magnitude, so real expense overruns are never flagged; only
  *positive*-amount categories (income, refunds) trip it — exactly Paychecks and Medical.

This reproduces the observed output precisely: `-3636.05 > 3600` is false (Mortgage hidden);
`+29303.38 > 18060` is true (Paychecks wrongly flagged); `+3356.26 > 50` is true (Medical refund
wrongly flagged).

### Proposed fix
1. Decide the canonical internal convention and normalize once at the boundary. Recommended:
   work in **spend magnitude** for expense categories (`(-txn.amount).max(0.0)` for outflows) and
   **separate income** out of the spending report entirely (or report it under a distinct
   `income_by_category` map).
2. Classify categories by type (expense vs income vs transfer). Monarch categories carry a group
   (`category.group` / `category.type` in the GraphQL — see ADR 0002/0003); fetch it so the
   report can exclude income and transfer groups from "spending."
3. `total_spent` = sum of expense magnitudes only.
4. `percent_of_budget` and `find_over_budget_categories` operate on magnitudes consistently so
   percentages are positive and overruns are detected for real expenses.
5. Exclude transfer / credit-card-payment categories from spend (see Finding 5).

### Test guidance (must be RED first)
The current tests pass *positive* expense amounts (`make_txn("Dining", 850.0, …)`,
`src/spending_report.rs:199-212, 237`). That positive-expense fixture is the bug's camouflage.
Add tests using **real sign convention**: expense txns negative, an income category positive, a
refund-in-expense-category positive, and a Transfer/Credit Card Payment category. Assert:
- a negative-amount Mortgage over its budget *is* flagged;
- a positive-amount Paychecks/income category is *never* in `over_budget_categories`;
- `percent_of_budget` is positive;
- `total_spent` excludes income and transfers.
Build the medium-tier mock fixture from a *captured real* shape (per CLAUDE.md test-pyramid
rules), and add/extend the gated large test so it would have caught this against live Monarch.

---

## Finding 2 — `progress_vs_goals` crashes when no goals file exists (HIGH)

### Symptom (observed this session)
```
MCP error -32603: Internal error: Goals file error:
cannot read /Users/mikelane/dev/family-finances/goals.toml: No such file or directory (os error 2)
```
A first-run user with no goals configured gets a hard error instead of "no goals set yet."

### Root cause
`Goals::load_from_path` treats **any** read failure — including file-not-found — as an error
(`src/goals.rs:69-79`):

```rust
let contents = std::fs::read_to_string(path)
    .map_err(|e| MonarchError::GoalsFile(format!("cannot read {}: {e}", path.display())))?;
```

This directly contradicts the module's own doc comment (`src/goals.rs:4-7`):
> "Missing goals are simply absent (not errors). A missing file or an empty file yields an empty
> `Goals` struct with all fields `None`."

…and the project CLAUDE.md ("`goals.rs` — Missing goals are absent, not errors"). The wrong
behavior is **locked in by a test** that asserts the crash
(`missing_file_returns_goals_file_error`, `src/goals.rs:204-208`). `load_from_env` only returns
default when the env var is *unset* (`:83-88`); since the MCP runs with `MONARCH_GOALS_FILE`
pointed at a not-yet-created path, it hits the error path.

### Proposed fix
- In `load_from_path`, distinguish `io::ErrorKind::NotFound` → `Ok(Goals::default())` from other
  I/O errors (permissions, etc.) → `Err(GoalsFile(...))`.
- Update `missing_file_returns_goals_file_error` to assert the new contract (missing → default),
  and add a test that a *permission-denied* path still errors.
- Confirm `progress_vs_goals` returns a graceful "no goals configured; here's how to add some"
  payload when `Goals::default()` is empty, rather than an empty/zeroed report.

---

## Finding 3 — No transaction-level drill-down (HIGH)

### Symptom
When the $12k Pets and $3.3k Medical anomalies appeared, neither the user nor the agent could
inspect the underlying transactions to confirm the suspected vet double-charge-then-refund. The
server exposes only aggregates (`spending_report`), the uncategorized worklist
(`triage_uncategorized`), recurring charges, and trends. There is **no tool to list the
transactions inside a category / merchant / date range.**

### Root cause / good news
The capability is *almost entirely present already*. `client.get_transactions(start, end, limit)`
issues `GetTransactionsList` with a full filter object — `search`, `categories`, `accounts`,
`tags`, `startDate`, `endDate` (`src/client.rs:600-648`) — and returns ids, amounts, dates,
merchant, category, tags, notes. **Only the MCP tool wrapper is missing.** No handler in
`tools.rs` exposes it.

### Proposed fix
Add a compound, task-oriented read tool (keep it advisor-shaped, not a raw passthrough — see the
project's "compound tools over wrappers" rule), e.g. `list_transactions` / `inspect_category`
that accepts category name and/or merchant and/or date range, calls `get_transactions` with the
matching filter, and returns the line items **including ids**, plus a small summary (count, net,
inflow vs outflow split so refunds are visible). Surfacing ids is what unlocks Finding 4.

---

## Finding 4 — Re-categorizing an already-categorized transaction is impossible (HIGH)

### Symptom
The actual thing the user needed — fix the mis-categorized vet/medical transactions — cannot be
done through the server. `triage_uncategorized` only returns transactions where
`needsReview: true` (`src/client.rs:654-696`), so already-categorized transactions never surface
an id. `apply_changeset` requires ids in its input.

### Root cause / good news
The *mutation* path is fully capable and correctly scoped. `apply_changeset` →
`apply_approved_changeset` → `client.update_transaction(id, category, tags, notes)`
(`src/tools.rs:521-548`) can re-categorize **any** transaction by id, and the allowlist already
restricts changes to category/tags/notes (prime directive intact). The *only* missing link is
**discovery of ids for already-categorized transactions** — which Finding 3's new tool provides.

### Proposed fix
No new mutation code needed. Once the list/inspect tool (Finding 3) returns ids, the existing
`apply_changeset` flow handles re-categorization. Consider a worked example in the tool
description so the agent knows the two-step pattern: inspect → apply_changeset. Optionally have
`apply_changeset` validate that each id exists in a recent fetch and report unknown ids (it
currently fetches all transactions only to count them and does not validate — `:531-533`).

---

## Finding 5 — `financial_overview` and `spending_report` disagree on "spending"; transfers not excluded (MEDIUM)

### Symptom
`financial_overview.spending = 23169.99` (Appendix A). The `spending_report` separately shows
`Credit Card Payment: -4364.48` and `Transfer: -3299.02` as their own spend lines — money moving
between the user's own accounts, not consumption. Depending on how Monarch's aggregate treats
those groups, the $23k headline either double-counts inter-account movement or silently
disagrees with the transaction-level report.

### Root cause
The two tools source "spending" differently:
- `financial_overview` passes through Monarch's `Web_GetCashFlowPage` aggregate
  `sumExpense.abs()` (`src/client.rs:775-830`, `src/financial_overview.rs:46-64`).
- `spending_report` sums raw transaction amounts (`src/spending_report.rs:80-86`).

Neither explicitly excludes transfer/credit-card-payment categories, and they can therefore
disagree. A savings-rate or "did we overspend" judgment built on either is unreliable until
transfers are handled consistently.

### Proposed fix
- Define one canonical "true spending" = expense magnitudes **excluding** transfer and
  credit-card-payment category groups, and use it in both tools (or have one tool delegate to a
  shared compute helper).
- Verify against Monarch's category groups (ADR 0002/0003) which groups are transfer-like.
- Add a test that a Transfer and a Credit Card Payment transaction do **not** count toward
  `total_spent` or `cashflow.spending`.

---

## Finding 6 — Refunds / reversals don't net or pair (MEDIUM)

### Symptom
The vet charged 2–3 times and refunded all but one. The net economic event is ~one charge, but
the session saw `Pets: -12053.08` (outflows retained in full) and a `Medical: +3356.26` positive
(a refund landed in the wrong category). Nothing reconciled them, so Pets is overstated and a
phantom Medical "inflow" appears.

### Proposed fix
- At minimum, make refunds visible: the new inspect tool (Finding 3) should show inflow vs
  outflow within a category so a refund is obvious.
- Consider a reversal-pairing heuristic (same merchant + opposite-sign amount within N days) in
  `spending_report` anomalies, analogous to the existing `find_possible_duplicates`
  (`src/spending_report.rs:163-188`), reported as `possible_reversals`.
- Do **not** silently net them away — surface the pair and let the advisor explain it.

---

## Finding 7 — Tests and mocks encode the wrong sign convention (MEDIUM, root cause of #1)

The reason Finding 1 shipped green is that the unit tests use **positive** amounts for expenses
(`src/spending_report.rs:199-548`, e.g. Dining `850.0`, Loan `1000.0`). Real Monarch returns
**negative** expenses / **positive** income, as this session's live data proves. The suite is
internally consistent and fully green while being wrong about the world — the precise
"doubles-only false-green" failure mode the project already calls out (memory:
`test-size-pyramid-no-doubles-only`; CLAUDE.md "never doubles-only").

### Proposed fix
- Re-baseline fixtures to real sign convention (negative expenses, positive income/refunds), and
  rebuild the medium-tier `bdd/mock_monarch` fixtures from *captured real* shapes including the
  documented-nullable cases (ADR 0003).
- Add a **gated large test** (`tests/live_integration.rs`, `MONARCH_LIVE=1`) that asserts
  `spending_report` and `financial_overview` agree on true spending and never flag an income
  category as over budget — the tier that would have caught this.

---

## Finding 8 — Minor cleanups (LOW)
- `src/tools.rs`: `days_to_ymd` (`:355-368`) and `epoch_days_to_ymd` (`:507-519`) are byte-for-byte
  duplicate calendar conversions. Collapse to one.
- `src/tools.rs:531-533`: `apply_changeset` fetches all transactions solely to obtain
  `total_count` and never validates that change ids exist. Either validate ids (and report
  unknown ones) or drop the fetch.

---

## What worked — do not regress
- `financial_overview` net worth ($518,846) and net-worth change look correct; `compute_overview`
  math is sound (`src/financial_overview.rs`). The problem there is only the upstream `spending`
  input (Finding 5).
- `triage_uncategorized` proposed sane categories for the 3 genuinely uncategorized items
  (Salmon Creek → Rent, two Apple → Credit Card Payment).
- The capability-denial design held: only category/tags/notes are mutable; no money-movement
  surface exists. Keep it that way — Findings 3 and 4 add **reads** and reuse the existing
  allowlisted mutation; they must not widen the mutation surface.

## Suggested fix sequencing for the next agent
1. **Finding 2** (goals crash) — smallest, unblocks `progress_vs_goals` and goal-setting.
2. **Finding 3 + 4** (one new list/inspect read tool) — unblocks drill-down *and*
   re-categorization; highest user-visible value.
3. **Finding 1 + 7** (sign convention + real-shape tests) — the trust fix; do these together so
   the new tests drive the rewrite RED→GREEN.
4. **Finding 5 + 6** (transfer exclusion, refund visibility) — depends on the canonical
   spending definition from #1.
5. **Finding 8** (cleanups) — opportunistic while in the files.

Each finding above is sized to become one GitHub issue (issue-before-implementation per project
rules). Recommend filing #1–#7 as separate issues, #8 as a single chore.

---

## Appendix A — raw tool output from this session (2026-05-31)

`financial_overview`:
```json
{"cashflow":{"income":32709.85,"net":9539.86,"spending":23169.99},
 "net_worth":518846.35,"net_worth_change":32624.04}
```

`spending_report` (abridged to the relevant lines):
```json
{"total_spent":2373.36,
 "over_budget_categories":["Medical","Paychecks"],
 "by_category":{
   "Paychecks":{"budget":18060.0,"percent_of_budget":162,"spent":29303.38},
   "Business Income":{"spent":3406.26},
   "Medical":{"budget":50.0,"percent_of_budget":6713,"spent":3356.26},
   "Mortgage":{"budget":3600.0,"percent_of_budget":-101,"spent":-3636.05},
   "Insurance":{"budget":410.0,"percent_of_budget":-107,"spent":-439.94},
   "Phone":{"budget":200.0,"percent_of_budget":-112,"spent":-223.12},
   "Pets":{"spent":-12053.08},
   "Credit Card Payment":{"spent":-4364.48},
   "Transfer":{"spent":-3299.02},
   "Groceries":{"spent":-731.27},
   "Restaurants & Bars":{"spent":-1054.09}},
 "vs_prior_month":{"delta":-17970.23}}
```

`triage_uncategorized`:
```json
{"proposed_changes":[
  {"category":"Rent","id":"…","merchant":"Salmon Creek"},
  {"category":"Credit Card Payment","id":"…","merchant":"Apple"},
  {"category":"Credit Card Payment","id":"…","merchant":"Apple"}]}
```

`progress_vs_goals`:
```
MCP error -32603: Internal error: Goals file error:
cannot read /Users/mikelane/dev/family-finances/goals.toml: No such file or directory
```
