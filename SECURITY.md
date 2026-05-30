# Security Policy

This project handles sensitive financial data and a live account session. Security reports are
taken seriously.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately via GitHub's [Security Advisories](https://github.com/mikelane/monarch-mcp/security/advisories/new)
("Report a vulnerability"), or email **mikelane@gmail.com** with subject `SECURITY: monarch-mcp`.

Please include: a description, affected version/commit, reproduction steps, and impact. You'll
get an acknowledgement within a few days. Coordinated disclosure is appreciated — give us a
reasonable window to ship a fix before any public write-up.

## Scope

In scope: authentication/session handling, credential or token leakage, the GraphQL client,
the changeset allowlist (anything that could let a write escape `category`/`tags`/`notes`),
and dependency vulnerabilities.

Out of scope: the fact that Monarch has no official API (see [DISCLAIMER.md](DISCLAIMER.md));
issues requiring a fully compromised local machine; social engineering.

## Security model (what to expect)

- **No money-movement code exists.** The only write path is an allowlist of
  `category`/`tags`/`notes`; other fields are rejected and reported. A way to bypass that
  allowlist *is* a vulnerability — report it.
- **Credentials stay local.** Login happens on your machine; the session token is written to
  `~/.config/monarch-mcp/session.json` with mode `0600`. The password and MFA secret are never
  persisted. Credentials are never written to logs.
- **You own the trust boundary.** Connecting this server to a remote/cloud MCP client means the
  client can invoke its (read + categorize) tools. Only connect it to clients you trust.

## Supported versions

Until 1.0, only the latest release receives security fixes.
