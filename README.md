# monarch-mcp

**An agentic budgeting companion.** A local-first [Model Context Protocol](https://modelcontextprotocol.io)
server that turns Claude (or any MCP client) into a financial advisor over your
[Monarch Money](https://www.monarchmoney.com) data — it knows where your household stands,
keeps the books honest, measures progress against your goals, and looks ahead.

[![CI](https://github.com/mikelane/monarch-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/mikelane/monarch-mcp/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/monarch-mcp.svg)](https://crates.io/crates/monarch-mcp)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

> [!IMPORTANT]
> **Unofficial.** Monarch Money has no public API. This server talks to Monarch's private
> GraphQL API and is **not affiliated with, endorsed by, or supported by Monarch Money**.
> Use at your own risk. See [DISCLAIMER.md](DISCLAIMER.md).

---

## Why this exists

Monarch has no official API, so every Claude↔Monarch integration is a community-built wrapper.
Rather than hand a third-party server full access to a live banking session, this is a
**purpose-built server you can read end to end**, with one defining property:

> **It cannot move money — by construction.** There is no transfer, payment, withdrawal,
> create, or delete code anywhere in it. The *only* write path can change a transaction's
> **category, tags, or notes** — nothing else. Capability denial is enforced by the code's
> *absence* of those tools, not by configuration you can misset.

It's read-and-categorize only, your session token never leaves your machine, and the whole
thing is validated against real Monarch by a [proper test pyramid](#how-its-built).

## The tools

Compound, task-oriented tools — each does a *job*, combining several API calls + computation
into one decision-ready result (not 1:1 API wrappers you could replace with `curl`):

| Tool | What it answers |
|------|-----------------|
| `financial_overview` | "Where do we stand?" — net worth, this-month cash flow, month-over-month change |
| `spending_report` | Spending by category vs budget, over-budget flags, likely duplicates, vs prior period |
| `progress_vs_goals` | Actuals vs your stored goals — on track / drifting / off |
| `cashflow_forecast` | Upcoming bills + income → projected month-end position + shortfall warnings |
| `net_worth_trend` | Net worth over time, deltas by account type, biggest movers |
| `recurring_scan` | New / changed / "creeping" subscriptions and upcoming renewals |
| `account_inventory` | All accounts bucketed by retirement-planning role — tax-advantaged, taxable brokerage, cash, other assets, liabilities — with a net-worth rollup |
| `triage_uncategorized` + `apply_changeset` | Propose categories from your own history, then commit only what you approve |

## Quick start

### 1. Install

```bash
# One-line shell installer (macOS / Linux)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/mikelane/monarch-mcp/releases/latest/download/monarch-mcp-installer.sh | sh

# PowerShell (Windows)
irm https://github.com/mikelane/monarch-mcp/releases/latest/download/monarch-mcp-installer.ps1 | iex

# Cargo, prebuilt binary — no compile (fetches the release artifacts)
cargo binstall monarch-mcp

# Cargo (from crates.io) — compiles from source
cargo install monarch-mcp

# Or build from source
git clone https://github.com/mikelane/monarch-mcp
cd monarch-mcp
cargo install --path .
```

### 2. Authenticate (one time)

```bash
monarch-mcp login
```

Prompts for your Monarch email, password, and MFA code. The session token is written to
`~/.config/monarch-mcp/session.json` (mode `0600`) and reused for months — your password and
MFA secret are **never** stored.

> **Sign in with Apple/Google?** Monarch's API needs email+password. Add a password in
> Monarch → Settings → Security; your SSO login keeps working alongside it.

### 3. Register with your MCP client

**Claude Code** (works today, local stdio):

```bash
claude mcp add monarch-mcp -- monarch-mcp
```

…or copy [`.mcp.json.example`](.mcp.json.example) to `.mcp.json` in your project.

> [!NOTE]
> **Claude Cowork** runs MCP servers in an isolated VM and currently can't reach a local
> stdio server ([known issue](https://github.com/anthropics/claude-code/issues/23424)).
> Use it from Claude Code on your machine; for Cowork you'd need to expose it as a remote
> HTTP MCP. See [docs](docs/) for the trade-offs.

### 4. Use it

> "Give me a financial overview." · "How's our spending vs budget this month?" ·
> "Any creeping subscriptions?" · "Are we on track for our goals?"

### Goals (optional)

`progress_vs_goals` measures against a TOML file you point to with `MONARCH_GOALS_FILE`.
See [`goals.example.toml`](goals.example.toml):

```toml
[savings_rate]
target_percent = 20.0

[emergency_fund]
target_months = 6.0
```

## Security model

- **No money movement exists in the binary.** The mutating path is an allowlist of
  `category` / `tags` / `notes`; any other field in a change request is rejected and reported.
- **Your token stays local.** Auth happens on your machine; the session lives in
  `~/.config/monarch-mcp/session.json` (`0600`). Credentials are never logged.
- **Human-in-the-loop writes.** `triage_uncategorized` only *proposes*; nothing is written
  until you approve a changeset and `apply_changeset` commits exactly that.

Reporting a vulnerability: see [SECURITY.md](SECURITY.md).

## How it's built

Production Rust (`rmcp` + `reqwest` + `tokio`), built TDD/BDD-first, with a deliberate
[Google test-size pyramid](https://testing.googleblog.com/2010/12/test-sizes.html) — because
testing only against mocks ships false confidence (it did, twice — see the ADRs):

| Tier | What | Count |
|------|------|------:|
| **Small** | Hermetic, in-process unit tests (the aggregation/classification math) | ~270 |
| **Medium** | Behave BDD against a mock Monarch GraphQL server over loopback | ~57 |
| **Large** | Gated (`MONARCH_LIVE=1`) integration tests against **real Monarch** | 8 |

```bash
cargo test                                   # small
cd bdd && uv run behave                       # medium (needs uv + the built binary)
MONARCH_LIVE=1 cargo test --test live_integration   # large (needs a real session)
```

The real GraphQL schema and the design decisions are documented in
[`docs/decisions/`](docs/decisions/) (ADRs 0001–0003).

## Project layout

```
src/            Rust MCP server — client, tools, goals, server, error
bdd/            Python + behave acceptance suite + mock Monarch GraphQL server
tests/          Large/live integration tests (gated)
docs/decisions/ ADRs (auth flow, real schema, tier-2 schema)
docs/specs/     Design spec
```

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the dev setup
(`mise` / `lefthook` / `clippy` / `rustfmt`), the test-pyramid expectations, and the
TDD/BDD workflow. Also read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

For the release process (secrets, merge→publish flow), see [docs/RELEASING.md](docs/RELEASING.md).

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your
option. Unless you explicitly state otherwise, any contribution you submit shall be
dual-licensed as above, without additional terms.
