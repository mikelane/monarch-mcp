<!-- Thanks for contributing! Keep the title in Conventional Commits style, e.g. `feat: add X`. -->

## What & why

<!-- What does this change and why? Link issues: Closes #NN -->

## How it was tested

<!-- Which tiers? small (cargo test) / medium (behave) / large (live). New behavior should be
covered at the appropriate tier — and mocks built from real captured shapes, including nulls. -->

## Checklist

- [ ] `cargo fmt --all` applied and `cargo clippy --all-targets` is clean
- [ ] Tests added/updated at the right tier(s) and passing (`cargo test`, and `behave` if tool behavior changed)
- [ ] **No money-movement capability added** (read + `category`/`tags`/`notes` only)
- [ ] No secrets or real financial data committed (synthetic fixtures only)
- [ ] ADR added under `docs/decisions/` if a non-obvious decision was made
- [ ] CHANGELOG updated under `## [Unreleased]` (if user-facing)
