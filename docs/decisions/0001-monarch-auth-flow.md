# 0001 — Monarch authentication & read access from Rust

- **Status:** Accepted
- **Date:** 2026-05-29
- **Spike:** `spike/monarch-auth` (branch kept locally as a porting reference; never merged)

## Context

We are building our own Rust MCP server for Monarch Money. Monarch has **no official
public API**; all access is via its private/internal GraphQL. Before committing to the
architecture in `docs/specs/2026-05-29-monarch-mcp-rust-design.md`, we needed to answer one
question: **can a plain Rust `reqwest` client complete Monarch's login and run a read query
against the live service, or does bot/device protection (e.g. Cloudflare) block a
non-browser client?**

A wrinkle surfaced mid-spike: the account uses **"Sign in with Apple"**, which the Monarch
API does not support. Per Monarch's ecosystem guidance, SSO users must **add a password** to
the account; this was done (Apple sign-in still works in the app alongside it).

## Decision / Findings (confirmed by live run, 2026-05-29)

The spike **PASSED**: a Rust `reqwest` client authenticated and read account data end to end.

Confirmed flow, to be ported into the production `monarch-client`:

- **Base URL:** `https://api.monarch.com` (the post-migration domain) — worked on the
  default run; no fallback to the legacy `api.monarchmoney.com` was needed. Keep it
  env-overridable.
- **Login:** `POST {base}/auth/login/` with JSON
  `{ username, password, supports_mfa: true, trusted_device: true }`.
  On an MFA challenge (HTTP `403` / MFA signal), re-POST the same body plus `totp: <code>`.
  Success returns `{ "token": "..." }`.
- **Authenticated requests:** header `Authorization: Token {token}` (DRF-style, **not**
  Bearer).
- **Required headers (browser-shaped):** `Accept: application/json`,
  `Content-Type: application/json`, `Client-Platform: web`, `Origin: https://app.monarch.com`,
  a generated `device-uuid`, and a realistic browser `User-Agent`.
- **No bot/Cloudflare wall** was encountered with the above. This is the critical finding —
  the Rust approach is viable.
- **Read:** `POST {base}/graphql`, `operationName: "GetAccounts"`,
  `query { accounts { id displayName currentBalance type { name } } }` →
  `data.accounts[]`. The `currentBalance` and `type.name` fields resolve, so the schema
  supports the fields the Tier-1 tools need.
- **Sessions are long-lived** (reported to last months); the token is the only secret we
  persist.

## Consequences

- `monarch-client` will: default to `https://api.monarch.com` (env override), send the
  confirmed header set incl. a generated `device-uuid` + browser-like UA, authenticate with
  `Authorization: Token …`, and persist the token to `~/.config/monarch-mcp/session.json`
  at mode `0600`.
- **`login` is interactive** (password + a 6-digit MFA code when challenged). We do **not**
  store the password or the MFA Base32 seed — re-auth is re-running `login` when the token
  eventually expires. Good hygiene; acceptable cadence given multi-month sessions.
- Apple/Google SSO accounts must keep the added password for API access; document this in
  the README.
- This **unblocks A1 (BDD bootstrap)** and **A2 (`monarch-client`)**. The spike binary on
  `spike/monarch-auth` is the reference implementation for porting; the branch is disposable
  and can be deleted with `git branch -D spike/monarch-auth` once `monarch-client` lands.

### Residual risks (carried into the design, isolated in `monarch-client`)

- Unofficial API: schema, domain, or header expectations can change without notice. Pin
  behavior in one layer; expect occasional fixes.
- Session expiry surfaces as auth errors in scheduled runs → tools must fail clearly and
  tell the user to re-run `login`.
- TOS gray area (reverse-engineered private API) — accepted, single-user personal use.
