//! Typed error enum for monarch-mcp.
#![allow(dead_code)] // Public API consumed by A4–A7 tool implementations
//!
//! All domain errors funnel through here so that MCP error responses carry
//! consistent, human-readable messages. The most important variant is
//! `SessionExpired`: every tool handler maps HTTP 401 from Monarch to this
//! variant and surfaces the re-authentication message the BDD scenarios assert.

use rmcp::ErrorData as McpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonarchError {
    /// Monarch returned HTTP 401 or signalled an expired session.
    /// Tools surface this as an auth error so Claude knows to tell the user
    /// to re-run `monarch-mcp login`.
    #[error("Monarch session expired — please re-run `monarch-mcp login` to re-authenticate")]
    SessionExpired,

    /// The `MONARCH_GOALS_FILE` was missing, unreadable, or contained invalid TOML.
    #[error("Goals file error: {0}")]
    GoalsFile(String),

    /// A GraphQL response contained an `errors` array.
    #[error("Monarch GraphQL error: {0}")]
    GraphQL(String),

    /// An HTTP transport or serialization error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Caller-supplied input was malformed or logically invalid (e.g. a
    /// non-ISO date string, or start_date after end_date). Surfaces as a soft
    /// error payload so the advisor receives a clear message rather than empty
    /// months or a hard MCP fault.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Any other unexpected error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl MonarchError {
    /// Convert to an MCP error response suitable for returning from a tool handler.
    pub fn into_mcp_error(self) -> McpError {
        // Use MCP's internal_error code for all variants — the message is what matters.
        McpError::internal_error(self.to_string(), None)
    }

    /// Return true when the error indicates the session needs re-authentication.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, MonarchError::SessionExpired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_expired_message_contains_reauth_hint() {
        let err = MonarchError::SessionExpired;
        let msg = err.to_string();
        assert!(
            msg.contains("re-authenticate") || msg.contains("login"),
            "expected re-auth hint in: {msg}"
        );
    }

    #[test]
    fn session_expired_is_detected_as_auth_error() {
        assert!(MonarchError::SessionExpired.is_auth_error());
    }

    #[test]
    fn other_errors_are_not_auth_errors() {
        assert!(!MonarchError::Internal("boom".into()).is_auth_error());
        assert!(!MonarchError::GraphQL("bad query".into()).is_auth_error());
    }

    #[test]
    fn into_mcp_error_preserves_message() {
        let err = MonarchError::SessionExpired;
        let msg = err.to_string();
        let mcp = MonarchError::SessionExpired.into_mcp_error();
        assert!(mcp.message.contains("re-authenticate") || mcp.message.contains(&msg[..20]));
    }
}
