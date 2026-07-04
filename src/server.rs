//! Streamable-HTTP MCP transport (Phase 1 of Claude Cowork support — see issue #88).
//!
//! Binds loopback-only. Refuses to bind any other address with a hard error —
//! there is no silent fallback. This is the same capability-denial pattern as
//! the `apply_changeset` field allowlist in `triage.rs`: the only way to expose
//! this server beyond localhost is a future, separate, explicitly-designed
//! change (Phase 2 — OAuth2.1 + tunnel), not an accidental bind.

use std::net::SocketAddr;

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

/// Runs the streamable-HTTP MCP server. Binds loopback-only (see
/// [`validate_bind_addr`]); refuses to start otherwise.
pub async fn run_http_server() -> anyhow::Result<()> {
    todo!("HTTP transport not yet implemented")
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
}
