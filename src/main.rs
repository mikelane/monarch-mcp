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
mod savings_rate;
mod spending_history;
mod spending_report;
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
    match args.get(1).map(String::as_str) {
        Some("login") => run_login().await,
        _ => run_server().await,
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
