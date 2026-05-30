# BDD Acceptance Tests — Monarch MCP

Python + behave acceptance tests for the Monarch MCP Rust server.
Scenarios drive the compiled binary over stdio against a local mock Monarch
GraphQL server, asserting on tool outputs.

## Structure

```
bdd/
├── features/               # Gherkin scenarios (do not modify — these are the spec)
│   ├── financial_overview.feature   @ISSUE-A4
│   ├── spending_report.feature      @ISSUE-A5
│   ├── triage_uncategorized.feature @ISSUE-A6
│   └── progress_vs_goals.feature    @ISSUE-A7
├── mock_monarch/           # Flask-based mock Monarch GraphQL + auth server
│   ├── __init__.py
│   └── server.py           # POST /graphql, POST /auth/login/, /reset, /configure
├── support/                # Shared test infrastructure
│   ├── __init__.py
│   └── mcp_client.py       # MCP stdio client (JSON-RPC over subprocess stdin/stdout)
├── steps/                  # Step definitions (one file per feature)
│   ├── __init__.py
│   ├── common.py           # Shared "connected" step + call_tool helper
│   ├── financial_overview_steps.py
│   ├── spending_report_steps.py
│   ├── triage_steps.py
│   └── progress_vs_goals_steps.py
├── environment.py          # before_all / before_scenario / after_scenario hooks
├── behave.ini              # Default tag filter: ~@not_implemented
├── pyproject.toml          # uv-managed deps: behave, flask, requests, tomli-w
└── README.md               # this file
```

## Setup

```bash
cd bdd
uv sync           # creates .venv and installs all deps
```

Requires Python 3.11+ and `uv`. Install uv: `curl -LsSf https://astral.sh/uv/install.sh | sh`

## Running the tests

### CI / normal mode — excludes not-yet-implemented scenarios

```bash
cd bdd
uv run behave
# or equivalently:
uv run behave --tags=~@not_implemented
```

Expected output: **0 failures, all scenarios skipped** (because every scenario is
currently tagged `@not_implemented`). This is the green baseline for CI before any
production code lands.

### RED mode — run a specific issue's scenarios (proves harness fails correctly)

```bash
cd bdd
uv run behave --tags=@ISSUE-A4   # financial_overview
uv run behave --tags=@ISSUE-A5   # spending_report
uv run behave --tags=@ISSUE-A6   # triage_uncategorized
uv run behave --tags=@ISSUE-A7   # progress_vs_goals
```

**Expected output:** All scenarios FAIL. The failure message will be:

```
ASSERT FAILED: Cannot call tool '<tool_name>': MCP server failed to start —
MCP binary not found: '.../target/debug/monarch-mcp'.
Build it with `cargo build` or set MONARCH_MCP_BIN.
```

This is the correct RED state. The mock Monarch server starts, Given steps
configure fixture data, and the When step fails at tool-call time because the
production binary does not exist yet. There are **no undefined step** errors.

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `MONARCH_MCP_BIN` | `../target/debug/monarch-mcp` | Path to the compiled Rust MCP binary |
| `MONARCH_BASE` | set by harness | Base URL of the mock (or live) Monarch server — passed to the MCP binary |
| `MONARCH_GOALS_FILE` | temp file per scenario | Path to the goals TOML file the MCP binary reads for `progress_vs_goals` |

To run against a custom binary:

```bash
MONARCH_MCP_BIN=/path/to/my/binary uv run behave --tags=@ISSUE-A4
```

## How the harness works

1. **`before_all`** — starts the mock Monarch GraphQL server on a random free port.
2. **`before_scenario`** — resets all fixture state; creates a fresh temp goals file;
   attempts to start the MCP binary subprocess. If the binary is missing, the error is
   stored on `context.mcp_start_error` and surfaces in the first When step.
3. **Given steps** — configure the mock server's fixture data via `POST /configure`
   (accounts, transactions, budgets, cashflow, goals TOML file).
4. **When steps** — call the real MCP tool via `mcp_client.call_tool()` over stdio
   JSON-RPC. Fail with a clear assertion error if the binary is missing.
5. **Then steps** — assert on the JSON returned by the tool.
6. **`after_scenario`** — stops the MCP subprocess; deletes the temp goals file.

## Advancing from RED to GREEN

For each implementation issue:

1. Remove `@not_implemented` from the relevant `@ISSUE-XX` scenarios.
2. Build the Rust binary: `cargo build` (from the repo root).
3. Run `uv run behave --tags=@ISSUE-AX` — should now pass.
4. CI runs `uv run behave` (which excludes `@not_implemented`) and stays green.
