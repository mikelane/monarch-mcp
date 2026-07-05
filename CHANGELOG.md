# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.3](https://github.com/mikelane/monarch-mcp/compare/v0.4.2...v0.4.3) - 2026-07-05

### Added

- native loopback-only streamable-HTTP transport (--http) ([#89](https://github.com/mikelane/monarch-mcp/pull/89))

### Miscellaneous

- gitignore demo/ capstone scaffolding ([#21](https://github.com/mikelane/monarch-mcp/pull/21)) ([#81](https://github.com/mikelane/monarch-mcp/pull/81))
- batch post-review cleanups (#52, #49, #43, #30) ([#79](https://github.com/mikelane/monarch-mcp/pull/79))
- test-quality polish for retirement_readiness (#69 review deferrals) ([#77](https://github.com/mikelane/monarch-mcp/pull/77))

### Testing

- harden transaction fetch layer and verify no Monarch pagination cap ([#80](https://github.com/mikelane/monarch-mcp/pull/80))

## [0.4.2](https://github.com/mikelane/monarch-mcp/compare/v0.4.1...v0.4.2) - 2026-06-11

### Added

- add retirement_readiness tool for SWR coverage vs baseline spend ([#74](https://github.com/mikelane/monarch-mcp/pull/74))
- add subscription_audit tool for ranked annualized recurring burn ([#73](https://github.com/mikelane/monarch-mcp/pull/73))
- add asset_allocation tool for net worth by asset class ([#72](https://github.com/mikelane/monarch-mcp/pull/72))
- add budget_review tool for mid-month budget pacing per category ([#71](https://github.com/mikelane/monarch-mcp/pull/71))
- add savings_rate tool for monthly income vs true-spending savings rate ([#70](https://github.com/mikelane/monarch-mcp/pull/70))

## [0.4.1](https://github.com/mikelane/monarch-mcp/compare/v0.4.0...v0.4.1) - 2026-06-11

### Documentation

- remove duplicate account_inventory entry from CHANGELOG [0.4.0] ([#61](https://github.com/mikelane/monarch-mcp/pull/61))

### Fixed

- deterministic spending_history outliers + date-helper and subtract_months cleanups ([#63](https://github.com/mikelane/monarch-mcp/pull/63))

## [0.4.0](https://github.com/mikelane/monarch-mcp/compare/v0.3.0...v0.4.0) - 2026-06-09

### Added

- add spending_history tool for multi-month true-spending baseline ([#56](https://github.com/mikelane/monarch-mcp/pull/56))
- add account_inventory tool for per-account planning buckets ([#51](https://github.com/mikelane/monarch-mcp/pull/51))

### Documentation

- add post-mortem from first live budgeting session ([#45](https://github.com/mikelane/monarch-mcp/pull/45))

### Fixed

- resolve category names to UUIDs before apply_changeset mutation ([#55](https://github.com/mikelane/monarch-mcp/pull/55))
- cap transaction limit at i32::MAX to avoid GraphQL Int32 overflow ([#48](https://github.com/mikelane/monarch-mcp/pull/48))

## [0.3.0](https://github.com/mikelane/monarch-mcp/compare/v0.2.0...v0.3.0) - 2026-06-01

### Added

- compute debt-payoff goal progress in progress_vs_goals ([#27](https://github.com/mikelane/monarch-mcp/pull/27)) ([#42](https://github.com/mikelane/monarch-mcp/pull/42))

### Miscellaneous

- enforce Conventional Commits in CI (PR title + all commits) ([#39](https://github.com/mikelane/monarch-mcp/pull/39)) ([#40](https://github.com/mikelane/monarch-mcp/pull/40))

## [0.2.0](https://github.com/mikelane/monarch-mcp/compare/v0.1.1...v0.2.0) - 2026-06-01

### Added

- add inspect_transactions advisor tool with drill-down and re-categorization ([#29](https://github.com/mikelane/monarch-mcp/pull/29))

### Changed

- dedupe date helpers, drop vestigial fetch, purge mock dead code ([#26](https://github.com/mikelane/monarch-mcp/pull/26)) ([#35](https://github.com/mikelane/monarch-mcp/pull/35))

### Documentation

- rename duplicate v0.1.0 changelog '### Added' prose block to '### Overview' ([#19](https://github.com/mikelane/monarch-mcp/pull/19))

### Fixed

- reckon month boundaries in the host's local timezone so the monthly tools no longer mislabel the current month late on the last day of a month for users behind UTC ([#36](https://github.com/mikelane/monarch-mcp/pull/38))
- make datetime handling deterministic and hermetic, fixing a flaky prior-month spending delta at month boundaries ([#34](https://github.com/mikelane/monarch-mcp/pull/37))
- compute true spending from transactions in both tools; surface refund pairs ([#32](https://github.com/mikelane/monarch-mcp/pull/32))
- honor Monarch sign convention in spending_report (expenses are negative) ([#31](https://github.com/mikelane/monarch-mcp/pull/31))
- treat missing goals file as no goals configured, so `progress_vs_goals` no longer crashes with MCP error -32603 when `MONARCH_GOALS_FILE` points at a path that does not exist yet ([#28](https://github.com/mikelane/monarch-mcp/pull/28), [#22](https://github.com/mikelane/monarch-mcp/issues/22))

## [0.1.1](https://github.com/mikelane/monarch-mcp/compare/v0.1.0...v0.1.1) - 2026-05-31

### Documentation

- document cargo binstall as a no-compile install path ([#16](https://github.com/mikelane/monarch-mcp/pull/16))

### Fixed

- single changelog headers + skip chore: release commits (closes #14) ([#17](https://github.com/mikelane/monarch-mcp/pull/17))

### Miscellaneous

- *(deps)* bump schemars from 0.8.22 to 1.2.1 ([#9](https://github.com/mikelane/monarch-mcp/pull/9))
- *(deps)* bump toml from 0.8.23 to 1.1.2+spec-1.1.0 ([#8](https://github.com/mikelane/monarch-mcp/pull/8))
- *(deps)* bump astral-sh/setup-uv from 5 to 7 ([#7](https://github.com/mikelane/monarch-mcp/pull/7))
- *(deps)* bump actions/checkout from 4 to 6 ([#5](https://github.com/mikelane/monarch-mcp/pull/5))
- *(deps)* bump gitleaks/gitleaks-action from 2 to 3 ([#4](https://github.com/mikelane/monarch-mcp/pull/4))

## [0.1.0](https://github.com/mikelane/monarch-mcp/releases/tag/v0.1.0) - 2026-05-30

### Added

- implement recurring_scan compound tool (ISSUE-B3)
- implement net_worth_trend tool (ISSUE-B2)
- implement cashflow_forecast tool handler — all 5 @ISSUE-B1 BDD scenarios GREEN
- add get_recurring() client op for Web_GetUpcomingRecurringTransactionItems
- add cashflow_forecast compute module with TDD
- register Epic B stub tools and real-shaped mock handlers (ISSUE-B1/B2/B3)
- add large integration test tier and fix session isolation (B5)
- port client to real Monarch GraphQL operations (ADR 0002)
- implement progress_vs_goals tool (issue A7)
- implement triage_uncategorized and apply_changeset tools (issue A6)
- implement spending_report tool (issue A5)
- implement financial_overview tool (issue A4)
- add monarch-mcp core scaffold (client, goals, tools, server)

### Documentation

- record D-NWT deferred bug + systemic mock-nulls lesson (Gate 3)
- capture real Tier-2 Monarch schema shapes (ADR 0003)
- mark Epic A + C1 done, kick off Epic B (Tier-2 tools)
- document test pyramid tiers and MONARCH_CONFIG_DIR env var
- capture real Monarch GraphQL schema in ADR 0002
- log B3 test-isolation bug (tests clobber real session file)
- record Gate 3 deferred bugs (B1 reserve detection, B2 partial responses)
- board updates + ignore spike scratch (A1 follow-up)
- ADR 0001 — Monarch auth flow confirmed (spike passed)
- design spec and planning board for Monarch MCP advisor

### Fixed

- drop invalid changelog_commit_message from release-plz.toml ([#10](https://github.com/mikelane/monarch-mcp/pull/10))
- replace unsafe env mutations with temp_env scoped guards
- add null-to-zero deserialization for numeric fields in Monarch responses
- broaden emergency-fund reserves to all liquid cash-equivalent types
- use first-seen-month baseline in net_worth_trend to prevent fabricated swings
- handle null amountDiff/merchant in recurring items, deterministic biggest_mover tie-break
- correct arithmetic typo in @ISSUE-B2 net_worth_change assertion
- align step text with feature file (trailing colons, singular/plural)
- handle negative Monarch budget amounts in spending_report
- realign mock server and BDD harness to real Monarch response shapes
- reject forbidden fields in apply_changeset via allowlist + safe parse
- return None from percent_of_budget when budget is zero
- reset behave context underscore attrs between scenarios

### Miscellaneous

- hands-off release automation (release-plz + cargo-dist, no Homebrew tap) ([#2](https://github.com/mikelane/monarch-mcp/pull/2))
- add PR checks ([#1](https://github.com/mikelane/monarch-mcp/pull/1))
- prepare for open-source release
- apply rustfmt across the codebase
- register monarch-mcp in Cowork (.mcp.json) + advisor instructions
- add Cargo.lock for reproducible builds

### Testing

- add null-bearing BDD fixtures and null-handling scenario for recurring scan
- remove @not_implemented from @ISSUE-B3 scenarios (RED)
- remove @not_implemented from @ISSUE-B2 scenarios (RED)
- add live integration test for get_recurring (MONARCH_LIVE=1 gated)
- add @not_implemented Gherkin for Epic B (ISSUE-B1/B2/B3)
- add BDD scenarios for negative-budget (loan-repayment) classification
- clear @not_implemented on A4/A6 features (all tools GREEN)
- add financial_overview aggregation math with TDD unit tests
- extend BDD harness to cover Gate-1 adversarial review additions
- stand up Python + behave BDD harness (A1 bootstrap, RED state)

### Overview

- Initial release of the `monarch-mcp` MCP server with seven compound tools:
  `financial_overview`, `spending_report`, `progress_vs_goals`, `cashflow_forecast`,
  `net_worth_trend`, `recurring_scan`, and `triage_uncategorized` + `apply_changeset`.
- Interactive `login` subcommand (email/password/MFA) with a locally-stored, reusable session.
- Read + categorize-only design: no money-movement code; the changeset path is an allowlist of
  `category`/`tags`/`notes`.
- A Google test-size pyramid: hermetic unit tests, a Python + behave BDD suite against a mock
  Monarch GraphQL server, and gated (`MONARCH_LIVE=1`) live integration tests against real
  Monarch.
- Architecture Decision Records documenting the auth flow and the real Monarch GraphQL schema
  (`docs/decisions/0001`–`0003`).

[Unreleased]: https://github.com/mikelane/monarch-mcp/commits/main
