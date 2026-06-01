# CLAUDE.md — working on monarch-mcp

Guidance for AI agents (and humans) contributing to this repository. Read this before
touching code. For *using* the server as an advisor, see [README.md](README.md).

## What this is

A Rust MCP server exposing **compound, task-oriented tools** over Monarch Money's unofficial
GraphQL API. Read + categorize only.

## Prime directive — capability denial at the source

**This server must never be able to move money.** There is no transfer/payment/withdrawal/
create/delete code, and there must never be. The only mutation is `apply_changeset`, which
may change **only** a transaction's `category`, `tags`, or `notes` — enforced by an allowlist
(`src/triage.rs`): any other field is rejected and reported, never sent.

If a change you're making would add a write capability beyond category/tags/notes, **stop** —
that's out of scope by design, not a feature request.

## Architecture

Layers, each independently testable (`src/`):

- **`client.rs`** — `MonarchClient`: auth (login + MFA + session persistence), the GraphQL
  transport, and typed read/mutation operations. Knows nothing about MCP. The *only* place
  that talks to Monarch.
- **`tools.rs`** — the MCP tool handlers (`rmcp`). Each handler is *thin*: fetch via the
  client, hand off to a pure compute function, map the result to an MCP response.
- **`financial_overview.rs` / `spending_report.rs` / `triage.rs` / `progress_vs_goals.rs` /
  `cashflow_forecast.rs` / `net_worth_trend.rs` / `recurring_scan.rs`** — the per-tool
  **pure compute functions** (aggregation/classification math) + their unit tests. This is
  where the logic and most tests live. Keep them free of I/O so they're *small* tests.
- **`goals.rs`** — parses the TOML goals file (`MONARCH_GOALS_FILE`). Missing goals are
  absent, not errors.
- **`error.rs`** — `MonarchError`. A `401` maps to `SessionExpired`, which tools surface as a
  **soft re-auth payload** (a successful result whose body says re-authentication is required)
  — never a panic or misleading zeros.
- **`server.rs` / `main.rs`** — the stdio server and the `login` CLI subcommand.

**Pattern for a new tool:** pure `compute_x(fetched_data) -> Result` (TDD'd) + a thin handler
in `tools.rs` that fetches and calls it. Follow an existing tool.

**Date arithmetic note:** `chrono` is a direct `[dependencies]` entry (feature `clock` only,
`default-features = false`). It is used solely in `today_epoch_day()` to obtain the current
date in the host's local timezone so "this month" matches what the user sees locally (ADR 0006).
All month/range arithmetic (`civil_to_epoch_day`, `epoch_days_to_ymd`, `days_in_month`,
`*_for_day`) remains hand-rolled and free of I/O — keep it that way.

## The test pyramid (non-negotiable)

We require a real [Google test-size mix](https://testing.googleblog.com/2010/12/test-sizes.html)
— **never doubles-only.** Mocks gave false-green twice (an invented GraphQL schema, then a
negative-budget shape); only the live tier caught it.

| Tier | Where | Run |
|------|-------|-----|
| **Small** (hermetic, no I/O) | `#[cfg(test)]` in `src/**` | `cargo test` |
| **Medium** (loopback mock server) | `bdd/features/*.feature` + `bdd/mock_monarch/` | `cd bdd && uv run behave` |
| **Large** (real Monarch, gated) | `tests/live_integration.rs` | `MONARCH_LIVE=1 cargo test --test live_integration` |

Rules:
- **A new client operation gets a gated large test** that hits real Monarch.
- **Build mocks from *captured real* shapes, not imagination** — and include the
  documented-nullable cases (ADR 0003 lists which fields can be `null`). A fully-populated
  fixture that omits real nulls is how bugs slip past green suites here.
- **TDD**: write the failing test first. **BDD** (`@ISSUE-XX` Gherkin) describes tool behavior
  at the MCP boundary. Don't weaken a scenario to make code pass — if a scenario's expected
  value is wrong, fix the scenario *and explain why in the commit*, never hardcode around it.

## Domain conventions

- **Monarch sign convention:** outflows and liabilities are **negative**. Budgets for expense
  categories are negative; comparisons use magnitudes (`budget.abs()`). Net worth sums signed
  balances directly.
- **Real GraphQL schema** is documented in `docs/decisions/0002-*` and `0003-*` — the field
  names are *not* guessable; consult the ADRs (queries were lifted from the
  `monarchmoneycommunity` library + community MCP servers).

## Environment contract

`MONARCH_BASE` (API base, default real Monarch) · `MONARCH_TOKEN` (use directly, skip login) ·
`MONARCH_GOALS_FILE` (goals TOML) · `MONARCH_CONFIG_DIR` / `XDG_CONFIG_HOME` (session dir —
tests set this to a temp dir so they never touch a real session) ·
`MONARCH_NOW` (**test-only** ISO `YYYY-MM-DD` clock override — pinned in `bdd/environment.py`
so the Rust client and the Python mock share the same "today"; never set in production).

## Secrets & data hygiene (keeps the history publishable)

- The real session token lives **only** at `~/.config/monarch-mcp/session.json` — never in the
  repo. Real Monarch captures go to **`/tmp`** — never committed.
- Test fixtures and ADR examples use **synthetic** values only. Never commit real balances,
  account names, or a real token (the ADRs document *shapes*, with placeholders).
- `.gitignore` already covers `session.json`, `.env*`, `.mm/`. Don't undo that.

## Dev setup & local checks

```bash
mise install            # toolchain (or use rustup; see rust-toolchain.toml)
mise run setup          # installs git hooks (lefthook) + deps
cargo fmt --all         # format
cargo clippy --all-targets --all-features   # lint (must be clean)
cargo test              # small tier
```

`lefthook` runs fmt-check + clippy + tests on pre-commit/pre-push. Don't bypass it. Commits
follow [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`,
`test:`, `chore:`) — the release pipeline derives versions from them. **Conventional Commit
format is enforced in CI** on both the PR title (which becomes the squash-merge subject on
`main`) and every individual commit. Allowed types match `release-plz.toml`:
`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `style`, `ci`, `build`, `revert`.

## Definition of done

Typecheck/build clean · `cargo test` green · `cargo clippy --all-targets` clean ·
`cargo fmt` applied · new behavior covered at the right tier(s) · ADR added if a non-obvious
decision was made (`docs/decisions/NNNN-*.md`).
