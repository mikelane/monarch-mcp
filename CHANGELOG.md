# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
