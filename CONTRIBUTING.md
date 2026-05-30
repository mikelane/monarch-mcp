# Contributing to monarch-mcp

Thanks for your interest! This guide gets you (and your coding agent) set up and productive.
Please also read [CLAUDE.md](CLAUDE.md) (architecture + invariants) and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## The one hard rule

**This server must never be able to move money.** It is read + categorize only — the only
write path can change a transaction's `category`/`tags`/`notes`. PRs that add transfer,
payment, withdrawal, create, or delete capabilities will be declined: that's a deliberate
design boundary, not a missing feature.

## Dev environment

You need **Rust** (toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml)) and,
for the medium (BDD) tier, **[uv](https://docs.astral.sh/uv/)** (Python).

Easiest path uses [mise](https://mise.jdx.dev) to manage both plus task shortcuts:

```bash
mise install        # installs the pinned Rust toolchain + uv
mise run setup      # installs git hooks (lefthook) and BDD deps
```

Prefer not to use mise? Use `rustup` (it honors `rust-toolchain.toml`), install `uv` and
`lefthook` yourself, then `lefthook install`.

## The test pyramid (please respect it)

We require a real test-size mix and **do not accept doubles-only changes** — mocks have shipped
false-green here twice. See [CLAUDE.md](CLAUDE.md#the-test-pyramid-non-negotiable).

```bash
mise run test          # small: cargo test
mise run bdd           # medium: behave against the mock Monarch server
mise run lint          # cargo fmt --check + clippy --all-targets
# large tier (needs your own real Monarch session — never run in CI):
MONARCH_LIVE=1 cargo test --test live_integration
```

- New logic → **small** unit tests (TDD: failing test first).
- New tool behavior → a **medium** `@ISSUE-XX` Gherkin scenario in `bdd/features/`.
- New client operation → a **large** gated test in `tests/live_integration.rs`.
- Build mock fixtures from the **real captured shapes** in the ADRs, including the
  **documented-nullable** fields — not from imagination.

## Workflow

1. Open (or comment on) an issue describing the change.
2. Branch, write the failing test first, make it pass, refactor.
3. `mise run lint && mise run test && mise run bdd` all green; `cargo fmt --all` applied.
4. Commit using [Conventional Commits](https://www.conventionalcommits.org)
   (`feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`) — the release pipeline derives
   the next version and changelog from these.
5. Open a PR. CI runs fmt/clippy/tests; keep it green. A non-obvious decision should land an
   ADR in `docs/decisions/NNNN-*.md`.

`lefthook` runs format-check, clippy, and tests on pre-commit/pre-push — please don't bypass it.

## Data hygiene (this keeps the project publishable)

- **Never commit secrets or real financial data.** Your session token lives only at
  `~/.config/monarch-mcp/session.json`; real Monarch captures go to `/tmp`. Fixtures and ADR
  examples use **synthetic** values only.
- `.gitignore` covers `session.json`, `.env*`, `.mm/`, `goals.toml`, `reports/`. Don't undo it.

## Licensing of contributions

By contributing, you agree your contributions are dual-licensed under
[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), matching the project, with no additional
terms (per the Apache-2.0 §5 inbound=outbound convention).
