# ADR 0018 — Streamable-HTTP transport: `--http` flag shape, default port, and loopback + Origin defense

**Date:** 2026-07-03
**Status:** Accepted
**Issue:** #88

---

## Context

monarch-mcp has always spoken MCP over stdio. Issue #88 is Phase 1 of 2
toward supporting Claude Cowork, which needs an MCP server reachable over
HTTP rather than a locally-spawned subprocess. Phase 2 (external OAuth2.1 +
a Cloudflare Tunnel) is explicitly out of scope here — this ADR only
covers making the server speak streamable-HTTP MCP *on the local machine*.

Per the prime directive (this server must never be able to move money,
and every capability is denied at the source unless explicitly designed
in), adding a network listener is not a decision to take lightly: a
listener that's reachable from anywhere is a materially different risk
profile than a subprocess spawned over stdin/stdout.

---

## Decision

### CLI shape

`monarch-mcp --http` is a new flag (not a subcommand) recognized by
`parse_mode` in `src/main.rs`. Default no-arg invocation still starts the
stdio server; `monarch-mcp login` is unchanged. `RunMode` gets a third
variant, `Http`, routed to `server::run_http_server()`.

Rejected alternative: a `serve --transport=http` subcommand. A flag was
chosen because it's the smaller surface change and matches how the issue
was scoped (`--http` was specified directly in the issue body).

### Bind address and port

Default bind address is `127.0.0.1:8770`, overridable via
`MONARCH_HTTP_ADDR` (e.g. `MONARCH_HTTP_ADDR=127.0.0.1:9000`). An unset
or empty env var falls back to the default — both cases are unit-tested
(`resolve_http_addr`).

8770 has no special significance beyond "not a well-known port, not
already claimed by anything else in this repo's tooling" — it is
recorded here so any future change to the default is a deliberate,
documented decision rather than an accidental drift.

### Loopback-only enforcement (hard error, no silent fallback)

`validate_bind_addr` parses the configured address and rejects anything
whose IP is not loopback (`SocketAddr::ip().is_loopback()`), covering
`127.0.0.0/8` and `::1`. Rejection is a hard `Result::Err` surfaced as a
process-exit error — never a silent rebind to loopback, never a warning
that lets the server start anyway. This mirrors the same
capability-denial pattern already used for `apply_changeset`'s field
allowlist (`src/triage.rs`): the only way to expose this server beyond
localhost is a future, separate, explicitly-designed change (Phase 2),
not a misconfigured env var.

### Loopback bind alone is insufficient — Origin validation is required too

A loopback bind stops *network* attackers (nothing outside the host can
open a TCP connection to `127.0.0.1`). It does **not** stop a
browser-based DNS-rebinding attack: a page the user's browser has open
can be served from an attacker-controlled hostname that initially
resolves to a real IP (passing any same-origin checks the attacker's own
server wants to pass), then rebinds that DNS record to `127.0.0.1` after
the page loads. The browser will still happily let JavaScript on that
page issue a `fetch()` to `http://localhost:8770/mcp`, because as far as
the browser's same-origin policy is concerned nothing about the origin
changed — only the DNS resolution did. The request lands on our loopback
listener looking exactly like a legitimate local client's request. This
is precisely the attack the MCP spec's Origin-validation requirement
exists to close, and it's why loopback-only binding is *necessary but
not sufficient*.

The fix: `is_allowed_origin` (`src/server.rs`) is a pure, unit-tested
function enforced as axum middleware in front of `/mcp` via
`tower::ServiceBuilder`:

- **Origin header present and not an exact loopback host** → reject with
  `403 Forbidden`. The allowed hosts are exactly `localhost`, `127.0.0.1`,
  or `[::1]` — each over `http://` and each with an optional `:port`. The
  match is on the *exact* host, not a prefix: a rebindable name like
  `localhost.evil.example` or `127.0.0.1.evil.example` (a prefix match would
  wave those through) is rejected, as is an embedded-userinfo trick like
  `http://localhost@evil.example`. Port validation is part of this exact-match
  enforcement: any port suffix must be non-empty and consist entirely of ASCII
  digits; a malformed port like `:8770evil` or an unclosed IPv6 bracket followed
  by arbitrary bytes (e.g., `[::1]evil`) causes the authority to be rejected.
  A present, non-localhost Origin can only come from a browser making a
  cross-origin request — exactly the DNS-rebinding shape above.
- **Origin header absent** → allow. Only browsers attach `Origin` to
  requests; non-browser MCP clients (including the Phase 2 tunnel path,
  which is a server-to-server hop, not a browser) never send one. Absent
  Origin is the legitimate-client case.

This was flagged mid-implementation as a required fix for #88 (not scope
creep) precisely because "binds loopback-only" was being treated as the
complete defense; it is one half of it.

### Serving the existing tool set

`StreamableHttpService::new(|| Ok(MonarchTools::new()), Arc::new(LocalSessionManager::default()), StreamableHttpServerConfig::default())`
nested at `/mcp` via `axum::Router::nest_service`. This reuses the exact
same `MonarchTools` registry the stdio transport uses — confirmed
empirically by inspecting the `initialize` response's `instructions`
field, which lists all 16 tools. No new Monarch client operations, no
new write capability: the HTTP transport is purely an additional way to
reach the same read/categorize surface.

`StreamableHttpServerConfig::default()` (`stateful_mode: true,
json_response: false`) was kept rather than opting into a stateless or
JSON-only variant — confirmed via manual curl smoke test that responses
are SSE-framed with an `Mcp-Session-Id` response header issued on the
first request and expected on subsequent ones. A `/healthz` endpoint
returning a bare `ok` was added alongside `/mcp` for basic liveness
checks.

### SessionExpired over HTTP

`SessionExpired` (a 401 from Monarch) surfaces as the same soft re-auth
payload over HTTP as it does over stdio — this is enforced by the tool
handlers in `tools.rs`, which are transport-agnostic and shared by both
`run_server()` (stdio) and `run_http_server()` (HTTP). Verified with a
BDD scenario (`@ISSUE-88`, "An expired session over HTTP asks the
household to re-authenticate") reusing the existing
`the advisor reports that re-authentication is required` step.

---

## Consequences

- `src/server.rs` is a new module: `validate_bind_addr`,
  `resolve_http_addr`, `is_allowed_origin`, `reject_disallowed_origin`
  (axum middleware), and `run_http_server`. All pure/hermetic logic is
  unit-tested; `run_http_server` itself is the thin I/O wrapper.
- New direct dependencies: `axum = "0.8"`, `tower = "0.5"` (feature
  `util`, for `ServiceBuilder`), and the `transport-streamable-http-server`
  feature on `rmcp`. `tokio` gained the `net` feature for `TcpListener`.
- BDD coverage lives in `bdd/features/http_transport.feature`
  (`@ISSUE-88`) with a new `HttpMcpClient` support class
  (`bdd/support/http_mcp_client.py`) that drives the binary via `--http`
  and parses the SSE-framed JSON-RPC responses over real HTTP.
- Phase 2 (OAuth2.1 + Cloudflare Tunnel, for reaching this server from
  outside the local machine) is a separate, future, explicitly-designed
  change — this ADR and the current implementation intentionally do not
  anticipate it beyond leaving the loopback + Origin checks as the
  documented boundary that any such change must reason about explicitly.

---

## Related

- Issue #88 — this feature
- `src/server.rs` — `validate_bind_addr`, `resolve_http_addr`,
  `is_allowed_origin`, `reject_disallowed_origin`, `run_http_server`
- `src/main.rs` — `RunMode`, `parse_mode`
- `src/triage.rs` — the `apply_changeset` field allowlist, the same
  capability-denial pattern this ADR's loopback enforcement follows
- `bdd/features/http_transport.feature`, `bdd/support/http_mcp_client.py`
- MCP spec — streamable-HTTP transport security requirements (Origin
  validation, loopback binding) that motivate this design
