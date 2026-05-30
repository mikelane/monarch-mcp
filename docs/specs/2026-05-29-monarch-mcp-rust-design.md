# Design Spec: Monarch MCP (Rust) — Agentic Budgeting Companion

**Date:** 2026-05-29
**Status:** Approved (design); implementation pending
**Owner:** Mike Lane

## Context

Mike wants Claude (in Cowork) to act as an ongoing **agentic budgeting companion /
financial advisor** for the household, fed by live data from **Monarch Money** (he is
a paying customer). Monarch has **no official public API**; all access is via its
private/internal GraphQL API. Rather than trust a community Python/TS MCP server with a
full Monarch session, we build our **own production-quality Rust MCP server** so that
dangerous capabilities (money movement, deletion) **do not exist in the code at all** —
capability-denial at the source, mirroring Mike's agent architecture.

The end state is not a reporting tool. It is an advisor that knows where the household
stands, keeps the books honest, measures progress against remembered goals, and looks
ahead — across daily → yearly cadences.

## Goals

- A local, auditable Rust MCP server exposing **compound, task-oriented tools** (never
  1:1 GraphQL passthroughs — see principle below).
- **Read + "tidy the books"** only: analyze, and categorize/tag/annotate transactions.
- A **goals/memory layer** so Claude advises against remembered targets, not just reports.
- Production quality: BDD (Python + behave) + TDD (Rust), strong error handling.

## Non-Goals (capability-denial by construction)

No tool exists for: creating/deleting transactions, transfers/payments, editing budgets,
or any money movement. These are **absent from the codebase**, not merely un-exposed.

## Core Principle: Compound Tools, Not Wrappers

The value of MCP is orchestration. Every tool combines multiple GraphQL ops + server-side
computation into one decision-ready result. If a proposed tool maps to a single API call,
it does not belong on the surface (the agent could `curl` it). See memory
`mcp-compound-tools-over-wrappers`.

## Architecture

Four layers, each independently testable:

1. **`monarch-client`** — ported HTTP/GraphQL client. Owns auth (login + MFA/TOTP),
   session persistence, the GraphQL operation set, and typed responses. Knows nothing
   about MCP. Unit-tested against a mock GraphQL server (wiremock).
2. **`tools`** — the compound tool implementations. Each composes `monarch-client` calls +
   aggregation/projection math + the goals layer. Pure-ish functions over a client trait,
   so they're testable with a faked client.
3. **`mcp-server`** — `rmcp` stdio server: registers tools, handles the MCP protocol,
   maps domain errors to MCP errors. Thin.
4. **`goals-store`** — local, human-editable goals/preferences (`goals.toml`/`goals.md`
   schema in the project dir) that Claude reads and measures against. Local state, NOT a
   Monarch feature.

```
Claude (Cowork) ⇄ stdio ⇄ mcp-server ─ tools ─ monarch-client ⇄ HTTPS ⇄ Monarch GraphQL
                                          └──── goals-store (local file)
```

### Auth & Session (ported from hammem/monarchmoney, to be confirmed in spike)

- `login` subcommand (interactive, run by Mike): POST `/auth/login/` with
  `{username, password, supports_mfa:true}`; on `403`, prompt + re-POST with `totp`
  (generated from the TOTP secret or entered manually); store returned `{token}`.
- Session token persisted to `~/.config/monarch-mcp/session.json` at mode `0600`.
  **Password and TOTP seed are never written to disk.** Re-auth = re-run `login`.
- All GraphQL requests send `Authorization: Token {token}` + standard headers.
- **Open risk (spike):** domain migration `api.monarchmoney.com` → `api.monarch.com`,
  and possible bot/device protection. Spike confirms the live domain + flow.

## Tool Surface (tiered, compound)

### Tier 1 — Most important (Phase 1)
- **`financial_overview`** — accounts by type + balances + net worth + MoM delta +
  this-month cashflow (in/out/net) + top categories. The "where we stand" snapshot.
- **`spending_report{period}`** — txns → grouped by category → vs budget w/ variance % →
  biggest merchants → vs prior period → anomalies (outliers, new merchants, likely dupes)
  → flags categories over budget.
- **`triage_uncategorized`** (read/suggest) + **`apply_changeset`** (the single mutating
  tool) — keep the books honest. `triage` returns a proposed changeset
  (category/tags/notes) from the user's own history; `apply_changeset` commits only what
  was approved. **Garbage-in-garbage-out guard for everything else.**
- **`progress_vs_goals`** — fuses Monarch actuals + the local goals store + a projection:
  savings rate, debt payoff, emergency-fund runway, investment targets → on-track /
  drifting / off + the lever to pull. *This is what makes Claude an advisor.*

### Tier 2 — Important (Phase 2 / early)
- **`cashflow_forecast`** — recurring bills + income timing → projected month-end position
  → shortfall warnings.
- **`net_worth_trend{period}`** — trajectory + deltas by account type + asset/liability
  split + which accounts moved the needle.
- **`recurring_scan`** — subscription creep, new/changed recurring charges, upcoming
  renewals; doubles as a fraud tripwire.

### Tier 3 — Nice to have (later)
`investment_review` (allocation/drift/fees/perf), `scenario_model` (what-ifs),
`debt_payoff_planner` (avalanche/snowball), `annual_summary` (yearly roll-up, tax-aware).

### Cadence mapping
- Daily (best-effort): thin `spending_report` anomaly glance.
- Weekly: `spending_report` + `recurring_scan`.
- Monthly: `financial_overview` + `progress_vs_goals` + `cashflow_forecast`.
- Quarterly: `net_worth_trend` + `progress_vs_goals` (goal reset).
- Yearly: `annual_summary` + `investment_review`.

## Testing Strategy

- **Unit / TDD (Rust):** `monarch-client` ops + each tool's aggregation/projection math
  against a **mock Monarch GraphQL** (wiremock). Cover auth states, error mapping, edge
  math (empty periods, negative net worth, missing budgets).
- **BDD (Python + behave** — cross-language per Mike's rule, Rust prod → Python BDD):
  scenarios drive the compiled MCP binary over stdio against the mock server, asserting on
  tool outputs. `@ISSUE-XX` tagged; `@not_implemented` until implemented.
- **Live smoke:** tiny suite gated by `MONARCH_LIVE=1`, run manually, never in CI.

## Phasing

- **Phase 0 — Spike (disposable):** Rust binary proves login + MFA + session persist +
  one read query (`GetAccounts`) against the **live** domain. Output: ADR with confirmed
  auth flow, domain, headers, and any bot-protection findings.
- **Phase 1 — Core + read + goals (Tier 1):** `monarch-client`, `rmcp` stdio server,
  `goals-store`, and `financial_overview`, `spending_report`, `triage_uncategorized` +
  `apply_changeset`, `progress_vs_goals`. First genuinely-advising version.
- **Phase 2 — Forward-looking (Tier 2):** `cashflow_forecast`, `net_worth_trend`,
  `recurring_scan`.
- **Phase 3 — Cowork wiring:** register MCP in the family-finances project, scheduled
  tasks (weekly floor; quarterly/yearly backed by calendar reminders), deep goal-setting
  session.

## Risks

- **Unofficial API fragility / TOS gray area** — Monarch can change the GraphQL schema or
  domain without notice; pin deps, isolate all of it in `monarch-client`, expect periodic
  fixes. Spike-confirm the domain.
- **Session expiry / re-auth** — interactive only; document the re-`login` procedure;
  scheduled tasks fail gracefully and tell Mike to re-auth.
- **Bot/device protection** — unknown until spike; if blocked, fall back to mirroring the
  community lib's exact headers/flow.
- **Local-run scheduling** — Cowork tasks only run when the Mac is awake + app open;
  mitigated with calendar reminders for low-frequency cadences.

## Local Artifacts Layout

```
~/dev/family-finances/
├── docs/specs/2026-05-29-monarch-mcp-rust-design.md   # this spec
├── docs/decisions/                                    # ADRs (spike output, etc.)
├── PLANNING.md (or docs/board.md)                     # local pseudo-issue board
├── monarch-mcp/                                        # the Rust crate (created in spike/P1)
├── bdd/                                                # behave features + steps (P1+)
├── CLAUDE.md, goals.* , reports/                       # Cowork project (P3)
```
No GitHub for now — local git + a markdown board, driven by the
`planning-milestones-and-epics` skill.
