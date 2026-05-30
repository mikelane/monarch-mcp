//! Library facade — exposes internal modules for large (live) integration tests.
//!
//! The binary crate (`src/main.rs`) uses these modules directly. This file
//! re-exports them so `tests/live_integration.rs` can import them without
//! duplicating the module graph.

pub mod client;
pub mod error;
pub mod financial_overview;
pub mod goals;
pub mod progress_vs_goals;
pub mod spending_report;
pub mod triage;
pub mod tools;
