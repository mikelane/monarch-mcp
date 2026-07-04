//! monarch-mcp — Monarch Money agentic budgeting MCP server.
//!
//! Usage:
//!   monarch-mcp          — run the stdio MCP server (default)
//!   monarch-mcp login    — interactive login: prompt email/password/MFA,
//!                          persist session to ~/.config/monarch-mcp/session.json
//!
//! Environment variables (see bdd/README.md for the full contract):
//!   MONARCH_BASE         — Monarch API base URL (default: https://api.monarch.com)
//!   MONARCH_TOKEN        — if set, use as session token and skip interactive login
//!   MONARCH_GOALS_FILE   — path to the TOML goals file for progress_vs_goals

mod account_inventory;
mod asset_allocation;
mod budget_review;
mod cashflow_forecast;
mod client;
mod error;
mod financial_overview;
mod goals;
mod inspect_transactions;
mod net_worth_trend;
mod progress_vs_goals;
mod recurring_scan;
mod retirement_readiness;
mod savings_rate;
mod server;
mod spending_history;
mod spending_report;
mod subscription_audit;
mod tools;
mod triage;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tools::MonarchTools;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let args: Vec<String> = std::env::args().collect();
    match parse_mode(&args) {
        RunMode::Login => run_login().await,
        RunMode::Stdio => run_server().await,
        RunMode::Http => server::run_http_server().await,
    }
}

/// Which mode to start the server in, derived from CLI arguments.
#[derive(Debug, PartialEq, Eq)]
enum RunMode {
    /// Default: stdio MCP server.
    Stdio,
    /// `login` subcommand: interactive credential capture.
    Login,
    /// `--http` flag: streamable-HTTP MCP server, loopback-only.
    Http,
}

/// Parses CLI args (including argv\[0\]) into a [`RunMode`]. Pure — no I/O.
fn parse_mode(args: &[String]) -> RunMode {
    match args.get(1).map(String::as_str) {
        Some("login") => RunMode::Login,
        Some("--http") => RunMode::Http,
        _ => RunMode::Stdio,
    }
}

/// Run the stdio MCP server. Reads MONARCH_BASE and MONARCH_TOKEN from env.
async fn run_server() -> Result<()> {
    tracing::info!("monarch-mcp starting (stdio MCP server)");

    let service = MonarchTools::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Interactive login subcommand — prompts for credentials, persists the token.
async fn run_login() -> Result<()> {
    use client::MonarchClient;
    use std::io::{self, Write};

    let base = std::env::var("MONARCH_BASE").ok().filter(|s| !s.is_empty());

    let mut monarch = MonarchClient::new(base);

    // Prompt for email
    print!("Monarch email: ");
    io::stdout().flush()?;
    let mut email = String::new();
    io::stdin().read_line(&mut email)?;
    let email = email.trim().to_string();

    // Prompt for password (hidden)
    let password = rpassword::prompt_password("Monarch password (hidden): ")?;

    // Attempt login
    match monarch.login_password(&email, &password).await {
        Ok(token) => {
            eprintln!("Authenticated. Token length: {}", token.len());
            eprintln!("Session saved to ~/.config/monarch-mcp/session.json");
        }
        Err(client::LoginError::MfaRequired) => {
            print!("MFA code (6 digits): ");
            io::stdout().flush()?;
            let mut totp = String::new();
            io::stdin().read_line(&mut totp)?;
            let totp = totp.trim().to_string();

            let token = monarch
                .login_totp(&email, &password, &totp)
                .await
                .map_err(|e| anyhow::anyhow!("Login failed: {e}"))?;
            eprintln!("Authenticated (MFA). Token length: {}", token.len());
            eprintln!("Session saved to ~/.config/monarch-mcp/session.json");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Login failed: {e}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod parse_mode_tests {
    use super::*;

    #[test]
    fn it_returns_login_mode_for_the_login_subcommand() {
        let args = vec!["monarch-mcp".to_string(), "login".to_string()];

        assert_eq!(parse_mode(&args), RunMode::Login);
    }

    #[test]
    fn it_returns_stdio_mode_when_no_args_are_given() {
        let args = vec!["monarch-mcp".to_string()];

        assert_eq!(parse_mode(&args), RunMode::Stdio);
    }

    #[test]
    fn it_returns_http_mode_for_the_http_flag() {
        let args = vec!["monarch-mcp".to_string(), "--http".to_string()];

        assert_eq!(parse_mode(&args), RunMode::Http);
    }
}
