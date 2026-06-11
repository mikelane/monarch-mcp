//! Library facade — exposes internal modules for large (live) integration tests.
//!
//! The binary crate (`src/main.rs`) uses these modules directly. This file
//! re-exports them so `tests/live_integration.rs` can import them without
//! duplicating the module graph.

pub mod account_inventory;
pub mod asset_allocation;
pub mod budget_review;
pub mod cashflow_forecast;
pub mod client;
pub mod error;
pub mod financial_overview;
pub mod goals;
pub mod inspect_transactions;
pub mod net_worth_trend;
pub mod progress_vs_goals;
pub mod recurring_scan;
pub mod retirement_readiness;
pub mod savings_rate;
pub mod spending_history;
pub mod spending_report;
pub mod subscription_audit;
pub mod tools;
pub mod triage;
