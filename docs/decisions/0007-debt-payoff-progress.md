# 0007 — Debt-payoff progress in `progress_vs_goals`

- Status: accepted
- Date: 2026-06-01
- Issue: #27 (follow-up to #22, post-mortem Finding 2)

## Context

`DebtPayoffGoal { target_date, monthly_payment: Option<f64> }` is parsed by `goals.rs` but
never computed. `progress_vs_goals` reported a guidance string for debt-only configs (#22's
minimal guard). This ADR defines how debt-payoff progress is measured and surfaced.

The existing goals (`savings_rate`, `emergency_fund`) emit a minimal `GoalStatus { status }`
via `classify_goal(actual, goal)` (on track ≥ goal; off < goal/2; else drifting).

## Decision

Compute **two** complementary debt-payoff signals and surface **enriched** numbers, not just a
status label.

### Debt identification
Debt accounts are those whose `account_type.name ∈ {"credit", "loan"}` (Monarch liability
types; balances are **negative**). Debt owed per account = `(-current_balance).max(0.0)` (a
positive balance on a credit account = overpaid → contributes 0). `total_debt` = sum over debt
accounts.

### Signal 1 — On schedule? (plan-based, uses already-fetched accounts)
- `months_to_target` = whole months from **today (local)** to `target_date` (uses the ADR-0006
  local clock; may be ≤ 0 if the date has passed).
- `required_monthly` = `total_debt / months_to_target` (only when `months_to_target > 0` and
  `total_debt > 0`).
- `on_schedule` = `classify_goal(planned_monthly, required_monthly)` where `planned_monthly`
  = `goal.monthly_payment`. Requires both values; `None` otherwise.

### Signal 2 — On pace? (behavioral, needs prior-month debt balance)
- Fetch `get_snapshots_by_account_type(prior_month_start)`; sum debt-type balances per month to
  get `prior_month_debt` and `current_month_debt` (owed magnitudes).
- `actual_paydown` = `prior_month_debt − current_month_debt` (positive = paid down). `None` when
  no prior snapshot exists.
- `on_pace` = `classify_goal(actual_paydown, planned_monthly)`. Requires both; `None` otherwise.

### Output — `debt_payoff: Option<DebtPayoffStatus>` (enriched)
```
DebtPayoffStatus {
  status: String,                 // overall (see blend)
  total_debt: f64,
  months_to_target: i64,
  required_monthly: Option<f64>,
  planned_monthly:  Option<f64>,
  on_schedule:      Option<String>,
  actual_paydown:   Option<f64>,
  on_pace:          Option<String>,
}
```
`Option` fields are `skip_serializing_if = "Option::is_none"`.

### Overall `status` blend
The conservative (worst) of the computable sub-statuses, ranked `off > drifting > on track`:
- both `on_schedule` and `on_pace` present → the worse of the two.
- only one present → that one.
- neither present → `"unknown"` (e.g. no `monthly_payment` set **and** no prior snapshot) — but
  the enriched numbers (`total_debt`, `months_to_target`, `required_monthly`) still surface so the
  advisor has something actionable.

### Edge cases (all unit-tested)
- `total_debt == 0` → already paid off → overall `"on track"`; `required_monthly = Some(0.0)`.
- `target_date` in the past with `total_debt > 0` → missed → `on_schedule = "off"`,
  `required_monthly = None`, `months_to_target ≤ 0`.
- `monthly_payment` absent → `on_schedule`/`on_pace` `None`; numbers still surface; overall may be
  `"unknown"`.
- no prior snapshot → `actual_paydown`/`on_pace` `None`.
- zero/negative paydown (debt grew) → `on_pace = "off"`.

### #22 guard removal
With real computation, a debt-only config now returns a populated `debt_payoff` rather than the
`#22`/`#27` guidance wording. The guidance branch that special-cased debt-payoff is removed; the
`has_no_computable_goals` guidance now fires only when **no** goal of any kind is set.

## Consequences
- `progress_vs_goals` gains a conditional snapshot fetch (only when a debt-payoff goal is set), so
  the no-debt-goal path keeps its current fetch cost.
- `compute_progress` takes the additional debt-history input (snapshots) and stays pure/testable.
- Live tier: a gated test exercises the real snapshot op for debt account types.
