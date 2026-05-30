# Planning Board — Family Finances (local)

> Local pseudo-issue board (no GitHub yet). Hierarchy + methodology from
> `planning-milestones-and-epics`, adapted to markdown. Dependencies are expressed
> as **Blocked by** lines. Spec: `docs/specs/2026-05-29-monarch-mcp-rust-design.md`.
> ADRs: `docs/decisions/NNNN-*.md`.

**Status legend:** ☐ todo · ◐ in progress · ☑ done · ✂ disposable (never merged)
**Issue IDs:** `A#` = epic A items, etc.

---

## Milestone M1 — Agentic Budgeting Companion v1 (Monarch-powered)

A local Rust MCP server that lets Claude (in Cowork) advise the household: know where we
stand, keep the books honest, measure against remembered goals, look ahead. Done when the
Tier-1 + Tier-2 tools work against live Monarch and are wired into the Cowork project.

Epics: **A** (core: auth+read+goals+tidy) → **B** (forward-looking) → **C** (Cowork wiring).

---

## Epic A — Monarch MCP core: auth + read + goals + tidy  ◐

**Acceptance criteria (observable):**
- [ ] `login` authenticates to live Monarch (password + MFA) and persists a session.
- [ ] `financial_overview`, `spending_report`, `triage_uncategorized`/`apply_changeset`,
      `progress_vs_goals` callable over stdio and return correct results vs a mock Monarch.
- [ ] No money-movement/create/delete/budget-edit tool exists in the binary.
- [ ] BDD scenarios for all four tool jobs pass (GREEN); unit coverage on client + tool math.

### A0 — Spike: prove Rust auth + one read query  ✂ ☑ DONE (2026-05-29)
- **Result: PASSED.** Rust `reqwest` authenticated + read accounts against live
  `api.monarch.com`, no bot wall. Apple-OAuth required adding a password. See
  `docs/decisions/0001-monarch-auth-flow.md`. Spike branch kept locally as porting ref.
- Branch `spike/monarch-auth` (never merged). Time-box: one session. **No TDD.**
- Prove: POST `/auth/login/` (password) → handle `403` MFA → re-POST `totp` → persist
  `{token}` → `Authorization: Token {token}` → run `GetAccounts`. Confirm the **live
  domain** (`api.monarchmoney.com` vs `api.monarch.com`), required headers, and whether
  any bot/device protection blocks a Rust `reqwest` client.
- **Deliverable:** `docs/decisions/0001-monarch-auth-flow.md` (context / decision /
  consequences: confirmed endpoints, headers, MFA handling, domain, fallbacks).
- Done when: ADR committed to main, spike branch deleted.
- **Gate 0 (adversarial-qa):** review ADR — are the unknowns actually resolved, or assumed?

### A1 — BDD Bootstrap (BLOCKS A2–A8)  ☐
- Blocked by: A0 (ADR informs the interface).
- Python + **behave** (cross-language per rule; Rust prod → Python BDD). A mock Monarch
  GraphQL (wiremock-style fixture server) backs the scenarios; behave drives the compiled
  MCP binary over stdio and asserts on tool outputs.
- Scenarios for each Tier-1 job, tagged `@ISSUE-A4`..`@ISSUE-A7`, `@not_implemented`.
  Include edge/error paths: empty period, missing budget, negative net worth, auth-expired,
  partial categorization, anomaly/dupe detection.
- Tests RUN and FAIL (RED). **Gate 1 (adversarial-qa):** scenarios unfakeable, edge cases
  covered, every scenario `@ISSUE-A#` tagged.

### A2 — `monarch-client` (auth + session + GraphQL transport + typed ops)  ☐
- Blocked by: A1.
- Ports the confirmed flow from ADR 0001. Typed ops: accounts, transactions, holdings,
  budgets, cashflow, categories, tags, needs-review, set-category, set-tags, update-notes.
- Isolated from MCP. Unit-tested (TDD) against mock GraphQL: auth states, query building,
  error mapping, session load/persist (`~/.config/monarch-mcp/session.json`, `0600`).
- **No** mutation beyond category/tags/notes exists in this layer.

### A3 — `mcp-server` skeleton + `goals-store`  ☐
- Blocked by: A1.
- `rmcp` stdio server, tool registry, domain→MCP error mapping, `login` subcommand.
- `goals-store`: local `goals.toml`/`goals.md` schema — read/parse/validate the household's
  goals (savings rate, debt payoff, emergency-fund runway, investment targets). Local state.

### A4 — Tool: `financial_overview`  ☐
- Blocked by: A2, A3. TDD on the aggregation math (net worth, MoM delta, cashflow, top cats).

### A5 — Tool: `spending_report{period}`  ☐  `@ISSUE-A5`
- Blocked by: A2, A3. vs-budget variance, prior-period compare, anomaly/dupe detection,
  over-budget flags. Heaviest math — triangulate tests.

### A6 — Tools: `triage_uncategorized` (suggest) + `apply_changeset` (commit)  ☐  `@ISSUE-A6`
- Blocked by: A2, A3. Two-step human-in-loop; `apply_changeset` is the **only** mutating
  tool and commits only an approved changeset (category/tags/notes).

### A7 — Tool: `progress_vs_goals`  ☐  `@ISSUE-A7`
- Blocked by: A2, A3, A4 (reuses overview), goals-store. Fuses actuals + goals + projection
  → on-track/drifting/off + the lever. *The advisor-defining tool.*

### A8 — (reserved) hardening + adversarial pass  ☐
- Blocked by: A4–A7. **Gate 2** (TDD history/triangulation/coverage) during A2–A7;
  **Gate 3** (adversarial-qa: boundary/invalid/error injection) here.

### A9 — Capstone: monthly-review demo  ☐
- Blocked by: A4–A8. Demonstrate a full monthly review end-to-end vs mock + one gated live
  run, including an error case (expired session). **Gate 4.** Narrated `.mp4` is optional
  for now (needs `ELEVENLABS_*`); a screen recording suffices for a personal tool unless
  Mike wants the full pipeline.

---

## Epic B — Forward-looking tools (Tier 2)  ☐
Own spec→plan→build cycle when reached.
- `cashflow_forecast` (recurring + income timing → month-end position → shortfall warning)
- `net_worth_trend{period}` (deltas by account type, asset/liability split, movers)
- `recurring_scan` (subscription creep, new/changed recurring, renewals, fraud tripwire)
- Spike likely **skippable** (deps proven in A); BDD bootstrap + impl + capstone still apply.

---

## Epic C — Cowork wiring + cadence + goals session  ☐
- Register MCP at project scope (`.mcp.json`) in `~/dev/family-finances`.
- Allowlist read + categorize/tag/notes (defense in depth even though dangerous tools
  don't exist).
- Scheduled tasks: weekly (floor), monthly, quarterly/yearly **+ calendar reminders**
  (Cowork schedules only fire when the Mac is awake + app open).
- Deep **goal-setting session** → populate `goals.*` that `progress_vs_goals` consumes.

---

## Tier 3 backlog (later): `investment_review`, `scenario_model`, `debt_payoff_planner`, `annual_summary`.

## Deferred bugs (Gate 3 adversarial findings, MEDIUM)
- **B1 — emergency-fund reserve detection too narrow** (`src/progress_vs_goals.rs`): counts only `account_type.name == "savings"`; a money-market/HYSA/brokerage emergency fund reads as $0 → false "off". Broaden the asset set or classify by sign (as `financial_overview` does).
- **B2 — no graceful degradation on partial/null Monarch responses** (`src/client.rs`): bare `f64` fields lack `#[serde(default)]`; a single `null` balance (unsynced account) fails the whole accounts parse. Add defaults / per-element skip per the spec's "degrade gracefully" goal.
- **B3 — test runs pollute the real user session file** — ✅ FIXED in C1 (`config_dir()` honors `MONARCH_CONFIG_DIR`/`XDG_CONFIG_HOME`).
- **D-NWT — net_worth_trend fabricates a swing for accounts opened/closed mid-window** (`src/net_worth_trend.rs`): a type absent in the earliest month gets a 0 baseline, so `change = latest − 0` (e.g. a brokerage opened mid-window reports a +$50k "move" and wins biggest_mover). Needs a domain decision: use the type's first-seen month as its baseline, or label it "new account". Gate 3 finding (deferred).

### Systemic lesson (recurring): mocks must include documented-nullable shapes
Three times now (C1 invented cashflow query, C1 negative budget, Tier-2 null `amountDiff`/`merchant`) a fully-populated mock hid a real-Monarch shape. **When building any mock fixture, include the null/edge cases the ADR documents** — and rely on the live tier to catch the rest.

## Progress
- **Epic A — DONE.** All 4 Tier-1 tools built (TDD), Gate 1 + Gate 3 run/remediated.
- **Epic C1 — DONE.** Aligned to REAL Monarch (ADR 0002), test pyramid established
  (203 small / 36 medium / 5 live). Validated on real data (net worth $510k). Merged to main.
- **Epic B — IN PROGRESS.** Tier-2 tools. Lesson applied: capture real shapes first;
  mock from real captures; small+medium+large tiers per tool (no doubles-only).

## Immediate next action
**Epic B bootstrap**: capture real Monarch shapes for the new ops (recurring transactions,
net-worth snapshots over a range, forecast inputs) → ADR 0003 → Gherkin (@ISSUE-B1..B3
@not_implemented) for `cashflow_forecast`/`net_worth_trend`/`recurring_scan` → extend the
mock with REAL shapes → RED. Then implement each tool with all three tiers.
