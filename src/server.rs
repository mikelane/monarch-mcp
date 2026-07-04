//! Streamable-HTTP MCP transport (Phase 1 of Claude Cowork support — see issue #88).
//!
//! Binds loopback-only. Refuses to bind any other address with a hard error —
//! there is no silent fallback. This is the same capability-denial pattern as
//! the `apply_changeset` field allowlist in `triage.rs`: the only way to expose
//! this server beyond localhost is a future, separate, explicitly-designed
//! change (Phase 2 — OAuth2.1 + tunnel), not an accidental bind.
//!
//! Bind address is `MONARCH_HTTP_ADDR` (default `127.0.0.1:8770`). The MCP
//! endpoint is served at `/mcp`; `/healthz` returns a plain `ok` for basic
//! liveness checks.

use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    extract::Request,
    http::{header::ORIGIN, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

use crate::tools::MonarchTools;

/// Parses `addr` as a socket address and rejects anything that isn't loopback.
///
/// Accepts `127.0.0.1:PORT` (and other `127.0.0.0/8` addresses) and `[::1]:PORT`.
/// Rejects everything else, including `0.0.0.0` and real network interfaces,
/// with a clear error message instead of a silent bind.
pub fn validate_bind_addr(addr: &str) -> Result<SocketAddr, String> {
    let parsed: SocketAddr = addr
        .parse()
        .map_err(|e| format!("invalid bind address {addr:?}: {e}"))?;

    if !parsed.ip().is_loopback() {
        return Err(format!(
            "refusing to bind non-loopback address {addr:?}: monarch-mcp's HTTP transport \
             must never listen beyond localhost"
        ));
    }

    Ok(parsed)
}

/// Returns whether `origin` (the raw `Origin` header value, if any) is
/// allowed to reach `/mcp`.
///
/// A present, non-localhost `Origin` means a browser tab is making this
/// request (only browsers send `Origin` cross-origin) — reject it, since
/// that's exactly the DNS-rebinding attack shape the MCP spec warns about:
/// a malicious page rebinds an attacker-controlled hostname to 127.0.0.1
/// and has the browser POST to it. A missing `Origin` means a non-browser
/// MCP client (the normal case for this server) — allow it.
pub fn is_allowed_origin(origin: &str) -> bool {
    if origin.is_empty() {
        return true;
    }

    // Only `http://` origins can be loopback here; anything else is cross-site.
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };

    // A well-formed `Origin` header carries no path, but parse defensively:
    // trim anything from the first path/query/fragment delimiter so a crafted
    // value like `http://localhost/../@evil` can't smuggle a real host past us.
    let authority = authority.split(['/', '?', '#']).next().unwrap_or(authority);

    // Reject embedded userinfo (`localhost@evil.example`) — the host that the
    // browser actually connects to is *after* the `@`, so a prefix like
    // `localhost@` must never be treated as loopback.
    if authority.contains('@') {
        return false;
    }

    // Require an *exact* loopback host. `starts_with` here would let an
    // attacker-registrable name such as `localhost.evil.example` or
    // `127.0.0.1.evil.example` (rebound to 127.0.0.1) pass — the exact DNS
    // rebinding shape this guard exists to stop.
    matches!(
        host_without_port(authority),
        "localhost" | "127.0.0.1" | "[::1]"
    )
}

/// Returns the host portion of an `authority` (`host` or `host:port`),
/// handling bracketed IPv6 literals like `[::1]:8770`.
fn host_without_port(authority: &str) -> &str {
    if authority.starts_with('[') {
        // IPv6 literal: the host is everything up to and including `]`.
        return match authority.find(']') {
            Some(end) => &authority[..=end],
            None => authority,
        };
    }
    match authority.split_once(':') {
        Some((host, _port)) => host,
        None => authority,
    }
}

/// Axum middleware enforcing [`is_allowed_origin`] on every request to `/mcp`.
///
/// Rejects disallowed origins with `403 Forbidden` before the request reaches
/// the MCP service.
async fn reject_disallowed_origin(request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if !is_allowed_origin(origin) {
        return StatusCode::FORBIDDEN.into_response();
    }

    next.run(request).await
}

/// Default bind address when `MONARCH_HTTP_ADDR` is unset.
const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8770";

/// Resolves the bind address from `MONARCH_HTTP_ADDR`, falling back to
/// [`DEFAULT_HTTP_ADDR`] when unset or empty.
fn resolve_http_addr() -> String {
    std::env::var("MONARCH_HTTP_ADDR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_HTTP_ADDR.to_string())
}

/// Runs the streamable-HTTP MCP server. Binds loopback-only (see
/// [`validate_bind_addr`]); refuses to start otherwise.
pub async fn run_http_server() -> anyhow::Result<()> {
    let bind_addr = resolve_http_addr();
    let socket_addr = validate_bind_addr(&bind_addr).map_err(|e| anyhow::anyhow!(e))?;

    let mcp_service = StreamableHttpService::new(
        || Ok(MonarchTools::new()),
        std::sync::Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let guarded_mcp_service = tower::ServiceBuilder::new()
        .layer(middleware::from_fn(reject_disallowed_origin))
        .service(mcp_service);

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest_service("/mcp", guarded_mcp_service);

    tracing::info!("monarch-mcp starting (streamable-HTTP MCP server on {socket_addr})");

    // Attach the address to bind/serve failures: the raw io::Error (e.g.
    // "Address already in use") carries no address, and the "starting on
    // {socket_addr}" line above is info-level (filtered out by the default
    // WARN threshold), so without this context an operator sees an
    // address-less error with no way to tell which port collided.
    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .with_context(|| format!("failed to bind HTTP MCP server to {socket_addr}"))?;
    axum::serve(listener, app.into_make_service())
        .await
        .with_context(|| format!("HTTP MCP server on {socket_addr} exited with an error"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_rejects_a_cross_site_origin() {
        let result = is_allowed_origin("https://evil.example");

        assert!(!result);
    }

    #[test]
    fn it_allows_a_request_with_no_origin_header() {
        let result = is_allowed_origin("");

        assert!(result);
    }

    #[test]
    fn it_allows_a_localhost_origin() {
        let result = is_allowed_origin("http://localhost:8770");

        assert!(result);
    }

    #[test]
    fn it_allows_an_ipv4_loopback_origin() {
        let result = is_allowed_origin("http://127.0.0.1:8770");

        assert!(result);
    }

    #[test]
    fn it_allows_an_ipv6_loopback_origin() {
        let result = is_allowed_origin("http://[::1]:8770");

        assert!(result);
    }

    #[test]
    fn it_rejects_a_subdomain_masquerading_as_localhost() {
        let result = is_allowed_origin("http://localhost.evil.example");

        assert!(!result);
    }

    #[test]
    fn it_rejects_a_subdomain_masquerading_as_loopback_ip() {
        let result = is_allowed_origin("http://127.0.0.1.evil.example");

        assert!(!result);
    }

    #[test]
    fn it_rejects_userinfo_masquerading_as_localhost() {
        let result = is_allowed_origin("http://localhost@evil.example");

        assert!(!result);
    }

    #[test]
    fn it_rejects_a_bare_prefix_extension_of_localhost() {
        let result = is_allowed_origin("http://localhostevil.example");

        assert!(!result);
    }

    #[test]
    fn it_allows_a_loopback_origin_that_carries_a_path() {
        // An `Origin` header never carries a path per spec, but the guard
        // strips anything from the first `/`, `?`, or `#` defensively (the
        // `split(['/', '?', '#'])` in `is_allowed_origin`). This pins that the
        // authority is isolated before the exact-host check, so a real loopback
        // host with a trailing path is still recognized as loopback. Without
        // that split, `host_without_port` would see `localhost/mcp` and reject
        // it — this test fails, catching a mutation that drops the split.
        let result = is_allowed_origin("http://localhost/mcp");

        assert!(result);
    }

    #[test]
    fn it_rejects_a_cross_site_host_hiding_loopback_in_a_path() {
        // The real host the browser connects to is `evil.example`; a `/localhost`
        // path segment must not launder it into the loopback allowlist.
        let result = is_allowed_origin("http://evil.example/localhost");

        assert!(!result);
    }

    #[test]
    fn it_rejects_a_cross_site_host_hiding_loopback_in_a_query() {
        // Same laundering attempt via a query string rather than a path.
        let result = is_allowed_origin("http://evil.example?next=localhost");

        assert!(!result);
    }

    #[test]
    fn it_allows_a_loopback_ip_origin_without_a_port() {
        let result = is_allowed_origin("http://127.0.0.1");

        assert!(result);
    }

    #[test]
    fn it_allows_an_ipv6_loopback_origin_without_a_port() {
        let result = is_allowed_origin("http://[::1]");

        assert!(result);
    }

    #[test]
    fn it_rejects_an_https_localhost_origin() {
        // A browser only sends a cross-origin `Origin` for a page it loaded;
        // an `https://` scheme cannot be this loopback http server, so treat it
        // as cross-site and reject.
        let result = is_allowed_origin("https://localhost:8770");

        assert!(!result);
    }

    #[test]
    fn it_rejects_a_non_loopback_address() {
        let result = validate_bind_addr("0.0.0.0:8770");

        assert!(result.is_err());
    }

    #[test]
    fn it_rejects_a_real_network_interface_address() {
        let result = validate_bind_addr("192.168.1.10:8770");

        assert!(result.is_err());
    }

    #[test]
    fn it_accepts_ipv4_loopback_and_returns_that_exact_socket_addr() {
        // Pin the returned `SocketAddr`, not just `is_ok()`: the returned value
        // is the address that actually gets bound. A mutation that returns
        // `Ok(0.0.0.0:8770)` on the accept path would pass an `is_ok()` check
        // while silently binding a public interface — this equality catches it.
        let result = validate_bind_addr("127.0.0.1:8770");

        assert_eq!(result, Ok("127.0.0.1:8770".parse().unwrap()));
    }

    #[test]
    fn it_accepts_ipv6_loopback_and_returns_that_exact_socket_addr() {
        let result = validate_bind_addr("[::1]:8770");

        assert_eq!(result, Ok("[::1]:8770".parse().unwrap()));
    }

    #[test]
    fn it_rejects_a_malformed_address() {
        let result = validate_bind_addr("not-an-address");

        assert!(result.is_err());
    }

    #[test]
    fn it_rejects_an_ipv4_mapped_ipv6_loopback_address() {
        // SAFETY / invariant: `::ffff:127.0.0.1` is an IPv4-mapped IPv6 address.
        // Rust's `Ipv6Addr::is_loopback()` is *only* true for `::1`, so this
        // maps to a real IPv4 interface and `validate_bind_addr` must reject it.
        // This test guards against a future "helpful" change that treats mapped
        // loopback as loopback and silently exposes the server off 127.0.0.1.
        let result = validate_bind_addr("[::ffff:127.0.0.1]:8770");

        assert!(result.is_err());
    }

    #[test]
    fn it_uses_the_default_addr_when_env_var_is_unset() {
        temp_env::with_var_unset("MONARCH_HTTP_ADDR", || {
            assert_eq!(resolve_http_addr(), DEFAULT_HTTP_ADDR);
        });
    }

    #[test]
    fn it_uses_the_env_var_when_set() {
        temp_env::with_var("MONARCH_HTTP_ADDR", Some("127.0.0.1:9999"), || {
            assert_eq!(resolve_http_addr(), "127.0.0.1:9999");
        });
    }

    #[test]
    fn it_falls_back_to_default_when_env_var_is_empty() {
        temp_env::with_var("MONARCH_HTTP_ADDR", Some(""), || {
            assert_eq!(resolve_http_addr(), DEFAULT_HTTP_ADDR);
        });
    }
}
