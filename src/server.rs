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

use axum::{routing::get, Router};
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

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest_service("/mcp", mcp_service);

    tracing::info!("monarch-mcp starting (streamable-HTTP MCP server on {socket_addr})");

    let listener = tokio::net::TcpListener::bind(socket_addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn it_accepts_ipv4_loopback() {
        let result = validate_bind_addr("127.0.0.1:8770");

        assert!(result.is_ok());
    }

    #[test]
    fn it_accepts_ipv6_loopback() {
        let result = validate_bind_addr("[::1]:8770");

        assert!(result.is_ok());
    }

    #[test]
    fn it_rejects_a_malformed_address() {
        let result = validate_bind_addr("not-an-address");

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
