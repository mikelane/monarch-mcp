//! Recurring charge scan — surfaces "creeping" subscriptions and upcoming renewals.
//!
//! ## Creeping charge detection
//! A charge is creeping when its actual amount differs from the stream's expected
//! amount (`amount_diff != 0`) AND the stream is NOT marked approximate
//! (`is_approximate = false`). Approximate streams (utilities, variable charges)
//! are intentionally excluded because their amount varies by design.
//!
//! ## Sign convention
//! Monarch stores amounts as negative outflows. The `amount_diff` field is
//! `actual − stream` (positive = price increase, negative = price decrease).
//! This module works with absolute magnitudes for display but preserves the
//! signed `amount_change` in output so callers can tell direction.
//!
//! ## Separation of concerns
//! All detection logic lives in [`compute_scan`]. The tool handler in `tools.rs`
//! fetches data and delegates here — no I/O in this module.

use serde::Serialize;

// ---------------------------------------------------------------------------
// Input type — populated by the client from the GraphQL response
// ---------------------------------------------------------------------------

/// One recurring item as fetched from Monarch for the scan.
///
/// Fields map directly to the `recurringTransactionItems[]` schema (ADR 0003).
#[derive(Debug, Clone)]
pub struct RecurringScanItem {
    /// Display name of the merchant / subscription.
    pub merchant: String,
    /// Expected amount per period from the stream (negative = outflow, Monarch convention).
    /// Carried for callers and display; `compute_scan` uses `amount_diff` directly.
    #[allow(dead_code)]
    pub stream_amount: f64,
    /// Actual amount charged this period (negative = outflow, Monarch convention).
    /// Carried for callers and display; `compute_scan` uses `amount_diff` directly.
    #[allow(dead_code)]
    pub actual_amount: f64,
    /// `actual_amount − stream_amount`. Positive = price increase.
    pub amount_diff: f64,
    /// True when the stream's amount is intentionally variable (utilities, etc.).
    /// Approximate streams are never flagged as creeping.
    pub is_approximate: bool,
    /// True when this charge has already occurred in the current period.
    pub is_past: bool,
}

// ---------------------------------------------------------------------------
// Output types — must match what BDD Then-steps assert on
// ---------------------------------------------------------------------------

/// Result payload returned as JSON text inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct ScanResult {
    /// Charges whose amount has drifted from the stream's expected value.
    /// Does NOT include approximate streams regardless of drift.
    pub creeping_charges: Vec<CreepingCharge>,
    /// Items that are not yet past (upcoming renewals this period).
    pub upcoming_renewals: Vec<UpcomingRenewal>,
}

/// A charge flagged as creeping (amount differs from stream expectation).
#[derive(Debug, Serialize, PartialEq)]
pub struct CreepingCharge {
    pub merchant: String,
    /// Signed amount change: positive = increase, negative = decrease.
    pub amount_change: f64,
}

/// An upcoming recurring charge (not yet past this period).
#[derive(Debug, Serialize, PartialEq)]
pub struct UpcomingRenewal {
    pub merchant: String,
}

// ---------------------------------------------------------------------------
// Pure detection logic — no I/O
// ---------------------------------------------------------------------------

/// Scan recurring items for creeping charges and upcoming renewals.
///
/// Creeping: `amount_diff != 0.0` AND `is_approximate == false`.
/// Upcoming: `is_past == false`.
pub fn compute_scan(items: &[RecurringScanItem]) -> ScanResult {
    let creeping_charges = items
        .iter()
        .filter(|item| !item.is_approximate && item.amount_diff.abs() > f64::EPSILON)
        .map(|item| CreepingCharge {
            merchant: item.merchant.clone(),
            amount_change: item.amount_diff,
        })
        .collect();

    let upcoming_renewals = items
        .iter()
        .filter(|item| !item.is_past)
        .map(|item| UpcomingRenewal {
            merchant: item.merchant.clone(),
        })
        .collect();

    ScanResult {
        creeping_charges,
        upcoming_renewals,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED → GREEN → TRIANGULATE → GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        merchant: &str,
        stream_amount: f64,
        actual_amount: f64,
        is_approximate: bool,
        is_past: bool,
    ) -> RecurringScanItem {
        RecurringScanItem {
            merchant: merchant.to_string(),
            stream_amount: -stream_amount.abs(),
            actual_amount: -actual_amount.abs(),
            amount_diff: actual_amount - stream_amount,
            is_approximate,
            is_past,
        }
    }

    // 9a RED: price increase → flagged as creeping
    #[test]
    fn price_increase_is_flagged_as_creeping() {
        let items = vec![item("StreamingCo", 9.99, 13.99, false, false)];
        let result = compute_scan(&items);
        let names: Vec<&str> = result
            .creeping_charges
            .iter()
            .map(|c| c.merchant.as_str())
            .collect();
        assert!(
            names.contains(&"StreamingCo"),
            "expected StreamingCo in creeping_charges"
        );
    }

    // 9b GREEN extension: amount_change is correct (positive for increase)
    #[test]
    fn price_increase_amount_change_is_positive() {
        let items = vec![item("StreamingCo", 9.99, 13.99, false, false)];
        let result = compute_scan(&items);
        let entry = result
            .creeping_charges
            .iter()
            .find(|c| c.merchant == "StreamingCo")
            .unwrap();
        assert!(
            (entry.amount_change - 4.0).abs() < 0.01,
            "expected +4.00, got {}",
            entry.amount_change
        );
    }

    // 9c TRIANGULATE: price decrease → also flagged (amount_change is negative)
    #[test]
    fn price_decrease_is_flagged_as_creeping() {
        let items = vec![item("MusicService", 10.99, 7.99, false, false)];
        let result = compute_scan(&items);
        let names: Vec<&str> = result
            .creeping_charges
            .iter()
            .map(|c| c.merchant.as_str())
            .collect();
        assert!(
            names.contains(&"MusicService"),
            "expected MusicService in creeping_charges"
        );
    }

    // 9c TRIANGULATE: amount_change for decrease has correct absolute magnitude
    #[test]
    fn price_decrease_amount_change_magnitude_is_correct() {
        let items = vec![item("MusicService", 10.99, 7.99, false, false)];
        let result = compute_scan(&items);
        let entry = result
            .creeping_charges
            .iter()
            .find(|c| c.merchant == "MusicService")
            .unwrap();
        assert!(
            (entry.amount_change.abs() - 3.0).abs() < 0.01,
            "expected |amount_change| 3.00, got {}",
            entry.amount_change
        );
    }

    // 9c TRIANGULATE: approximate charge → NOT flagged even when amount differs
    #[test]
    fn approximate_charge_is_not_flagged_as_creeping() {
        let items = vec![item("ElectricUtil", 120.0, 134.50, true, false)];
        let result = compute_scan(&items);
        let names: Vec<&str> = result
            .creeping_charges
            .iter()
            .map(|c| c.merchant.as_str())
            .collect();
        assert!(
            !names.contains(&"ElectricUtil"),
            "ElectricUtil must NOT be in creeping_charges"
        );
    }

    // 9c TRIANGULATE: stable charges → NOT flagged
    #[test]
    fn stable_charge_is_not_flagged_as_creeping() {
        let items = vec![
            item("CloudStorage", 2.99, 2.99, false, false),
            item("NewsFeed", 14.99, 14.99, false, false),
        ];
        let result = compute_scan(&items);
        assert!(
            result.creeping_charges.is_empty(),
            "stable charges must not be flagged: {:?}",
            result.creeping_charges
        );
    }

    // 9c TRIANGULATE: stable subscription alongside creeping one — only creeping flagged
    #[test]
    fn stable_subscription_not_flagged_when_creeping_one_also_present() {
        let items = vec![
            item("StreamingCo", 9.99, 13.99, false, false),
            item("GymMembership", 50.0, 50.0, false, false),
        ];
        let result = compute_scan(&items);
        let names: Vec<&str> = result
            .creeping_charges
            .iter()
            .map(|c| c.merchant.as_str())
            .collect();
        assert!(
            names.contains(&"StreamingCo"),
            "StreamingCo must be flagged"
        );
        assert!(
            !names.contains(&"GymMembership"),
            "GymMembership must NOT be flagged"
        );
    }

    // 9a RED: upcoming renewals — future items appear, past do not
    #[test]
    fn upcoming_renewals_excludes_past_items() {
        let items = vec![
            item("RentPayment", 1500.0, 1500.0, false, true), // past
            item("AnnualBackup", 99.0, 99.0, false, false),   // upcoming
            item("StreamingCo", 13.99, 13.99, false, false),  // upcoming
        ];
        let result = compute_scan(&items);
        let upcoming: Vec<&str> = result
            .upcoming_renewals
            .iter()
            .map(|u| u.merchant.as_str())
            .collect();
        assert!(
            upcoming.contains(&"AnnualBackup"),
            "AnnualBackup must be upcoming"
        );
        assert!(
            upcoming.contains(&"StreamingCo"),
            "StreamingCo must be upcoming"
        );
        assert!(
            !upcoming.contains(&"RentPayment"),
            "RentPayment must NOT be upcoming (is_past=true)"
        );
    }

    // 9c TRIANGULATE: empty input → empty results
    #[test]
    fn empty_items_produce_empty_scan() {
        let result = compute_scan(&[]);
        assert!(
            result.creeping_charges.is_empty(),
            "expected no creeping charges"
        );
        assert!(
            result.upcoming_renewals.is_empty(),
            "expected no upcoming renewals"
        );
    }
}
