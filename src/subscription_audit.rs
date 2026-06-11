//! Subscription audit — full ranked inventory of recurring charge burn.
//!
//! ## What this tool answers
//! "What is my total subscription load, ranked by cost, annualized — what's
//! the fat to cut?" This is an *inventory* lens: every outflow stream ranked
//! by annual cost with totals.
//!
//! ## Relationship to `recurring_scan`
//! `recurring_scan` is an *anomaly* lens — it finds creeping amounts and
//! upcoming renewals. `subscription_audit` is a distinct output contract
//! that answers a different budgeting question. Both consume the same
//! underlying recurring data from Monarch. See ADR 0015.
//!
//! ## Cadence normalization (ADR 0015)
//! Each stream is normalized to a monthly-equivalent amount:
//!
//! | Monarch `frequency` | monthly factor |
//! |---------------------|----------------|
//! | `"monthly"`         | 1.0            |
//! | `"yearly"`          | 1/12 ≈ 0.0833  |
//! | `"weekly"`          | 52/12 ≈ 4.333  |
//! | `"biweekly"`        | 26/12 ≈ 2.167  |
//! | `"quarterly"`       | 1/3  ≈ 0.333   |
//! | `"semiannual"`      | 1/6  ≈ 0.167   |
//! | unknown             | 1.0 (treated as monthly) |
//!
//! `annualized_amount = monthly_equivalent * 12`
//!
//! ## Income exclusion
//! Streams with positive `stream_amount` (inflows) are excluded — only
//! outflows (negative amounts in Monarch convention) are audited.
//!
//! ## Approximate streams
//! Streams where `is_approximate = true` (utilities, variable charges) are
//! included with their expected `stream_amount`, but flagged
//! `approximate: true` so callers can distinguish fixed from variable costs.
//!
//! ## Sign convention
//! Input `stream_amount` follows Monarch convention (negative for outflows).
//! Output `monthly_amount` and `annualized_amount` are magnitudes (positive).

use serde::Serialize;

// ---------------------------------------------------------------------------
// Input type — populated by the client from the GraphQL response
// ---------------------------------------------------------------------------

/// One recurring stream item as fetched for the subscription audit.
///
/// Fields map to `recurringTransactionItems[].stream` (ADR 0003 + 0015).
#[derive(Debug, Clone)]
pub struct SubscriptionAuditItem {
    /// Display name of the merchant / subscription.
    pub merchant: String,
    /// Expected amount per period from the stream (negative = outflow, Monarch convention).
    pub stream_amount: f64,
    /// Monarch cadence string: "monthly", "yearly", "weekly", "biweekly",
    /// "quarterly", "semiannual", or unknown values (treated as monthly).
    pub frequency: String,
    /// True when the stream's amount is intentionally variable (utilities, etc.).
    pub is_approximate: bool,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// One subscription entry in the ranked audit output.
#[derive(Debug, Serialize, PartialEq)]
pub struct SubscriptionEntry {
    pub merchant: String,
    /// Monthly-equivalent cost (magnitude, always positive).
    pub monthly_amount: f64,
    /// Annualized cost = monthly_amount * 12 (magnitude, always positive).
    pub annualized_amount: f64,
    /// Monarch cadence string for this stream.
    pub cadence: String,
    /// True when this stream's amount varies by design (utility-style charges).
    pub approximate: bool,
}

/// Full subscription audit result.
#[derive(Debug, Serialize, PartialEq)]
pub struct AuditResult {
    /// All outflow streams, sorted by `annualized_amount` descending.
    pub subscriptions: Vec<SubscriptionEntry>,
    /// Sum of all `monthly_amount` values.
    pub total_monthly: f64,
    /// Sum of all `annualized_amount` values (= total_monthly * 12).
    pub total_annual: f64,
}

// ---------------------------------------------------------------------------
// Cadence normalization
// ---------------------------------------------------------------------------

/// Convert a Monarch frequency string to a monthly multiplier.
///
/// The monthly multiplier scales the stream's per-period amount to a
/// per-month basis. See ADR 0015 for the factor table and rationale.
fn cadence_to_monthly_factor(frequency: &str) -> f64 {
    match frequency {
        "monthly" => 1.0,
        "yearly" | "annually" => 1.0 / 12.0,
        "weekly" => 52.0 / 12.0,
        "biweekly" | "every_two_weeks" => 26.0 / 12.0,
        "quarterly" | "every_three_months" => 1.0 / 3.0,
        "semiannual" | "twice_a_year" => 1.0 / 6.0,
        // Unknown cadences default to monthly to avoid silently zeroing a stream.
        _ => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Pure audit logic — no I/O
// ---------------------------------------------------------------------------

/// Compute a ranked subscription audit from recurring stream items.
///
/// Excludes income streams (positive `stream_amount`).
/// Includes approximate streams flagged as such.
/// Sorts output by `annualized_amount` descending.
pub fn compute_subscription_audit(items: &[SubscriptionAuditItem]) -> AuditResult {
    let mut subscriptions: Vec<SubscriptionEntry> = items
        .iter()
        .filter(|item| item.stream_amount < 0.0) // outflows only
        .map(|item| {
            let factor = cadence_to_monthly_factor(&item.frequency);
            let monthly_amount = item.stream_amount.abs() * factor;
            let annualized_amount = monthly_amount * 12.0;
            SubscriptionEntry {
                merchant: item.merchant.clone(),
                monthly_amount,
                annualized_amount,
                cadence: item.frequency.clone(),
                approximate: item.is_approximate,
            }
        })
        .collect();

    // Sort by annualized cost descending (highest cost first).
    subscriptions.sort_by(|a, b| {
        b.annualized_amount
            .partial_cmp(&a.annualized_amount)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_monthly: f64 = subscriptions.iter().map(|s| s.monthly_amount).sum();
    let total_annual: f64 = subscriptions.iter().map(|s| s.annualized_amount).sum();

    AuditResult {
        subscriptions,
        total_monthly,
        total_annual,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED → GREEN → TRIANGULATE → GREEN → REFACTOR
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a monthly outflow item (the common case).
    fn monthly(merchant: &str, amount: f64) -> SubscriptionAuditItem {
        SubscriptionAuditItem {
            merchant: merchant.to_string(),
            stream_amount: -amount.abs(), // Monarch convention: negative = outflow
            frequency: "monthly".to_string(),
            is_approximate: false,
        }
    }

    /// Helper: build an outflow item with explicit cadence.
    fn item_with_cadence(
        merchant: &str,
        amount: f64,
        frequency: &str,
        is_approximate: bool,
    ) -> SubscriptionAuditItem {
        SubscriptionAuditItem {
            merchant: merchant.to_string(),
            stream_amount: -amount.abs(),
            frequency: frequency.to_string(),
            is_approximate,
        }
    }

    /// Helper: build an income inflow item.
    fn income(merchant: &str, amount: f64) -> SubscriptionAuditItem {
        SubscriptionAuditItem {
            merchant: merchant.to_string(),
            stream_amount: amount.abs(), // positive = inflow
            frequency: "monthly".to_string(),
            is_approximate: false,
        }
    }

    // 9a RED: monthly subscription → monthly_amount equals stream magnitude
    #[test]
    fn monthly_subscription_monthly_amount_equals_stream_magnitude() {
        let items = vec![monthly("StreamBundle", 50.0)];
        let result = compute_subscription_audit(&items);
        assert_eq!(result.subscriptions.len(), 1);
        assert!(
            (result.subscriptions[0].monthly_amount - 50.0).abs() < 0.01,
            "expected monthly_amount 50.00, got {}",
            result.subscriptions[0].monthly_amount
        );
    }

    // 9b GREEN extension: monthly → annualized = monthly * 12
    #[test]
    fn monthly_subscription_annualized_is_monthly_times_12() {
        let items = vec![monthly("StreamBundle", 50.0)];
        let result = compute_subscription_audit(&items);
        assert!(
            (result.subscriptions[0].annualized_amount - 600.0).abs() < 0.01,
            "expected annualized_amount 600.00, got {}",
            result.subscriptions[0].annualized_amount
        );
    }

    // 9c TRIANGULATE: yearly subscription → monthly_amount = yearly / 12
    #[test]
    fn yearly_subscription_monthly_amount_is_annual_divided_by_12() {
        let items = vec![item_with_cadence("NewsCo", 120.0, "yearly", false)];
        let result = compute_subscription_audit(&items);
        assert_eq!(result.subscriptions.len(), 1);
        assert!(
            (result.subscriptions[0].monthly_amount - 10.0).abs() < 0.01,
            "expected monthly_amount 10.00 for yearly 120, got {}",
            result.subscriptions[0].monthly_amount
        );
        assert!(
            (result.subscriptions[0].annualized_amount - 120.0).abs() < 0.01,
            "expected annualized_amount 120.00 for yearly 120, got {}",
            result.subscriptions[0].annualized_amount
        );
    }

    // 9c TRIANGULATE: weekly subscription → monthly_amount = weekly * 52/12
    #[test]
    fn weekly_subscription_monthly_amount_normalizes_to_52_over_12() {
        // $10/week → $10 * 52 / 12 ≈ $43.33/month
        let items = vec![item_with_cadence("WeeklyService", 10.0, "weekly", false)];
        let result = compute_subscription_audit(&items);
        let expected_monthly = 10.0 * 52.0 / 12.0;
        assert!(
            (result.subscriptions[0].monthly_amount - expected_monthly).abs() < 0.01,
            "expected monthly_amount {:.4}, got {}",
            expected_monthly,
            result.subscriptions[0].monthly_amount
        );
    }

    // 9c TRIANGULATE: quarterly subscription → monthly_amount = quarterly / 3
    #[test]
    fn quarterly_subscription_monthly_amount_is_quarterly_divided_by_3() {
        // $90/quarter → $30/month
        let items = vec![item_with_cadence("QuarterlyMag", 90.0, "quarterly", false)];
        let result = compute_subscription_audit(&items);
        assert!(
            (result.subscriptions[0].monthly_amount - 30.0).abs() < 0.01,
            "expected monthly_amount 30.00 for quarterly 90, got {}",
            result.subscriptions[0].monthly_amount
        );
    }

    // 9c TRIANGULATE: ranking — higher annualized cost ranked first
    #[test]
    fn subscriptions_ranked_by_annualized_cost_descending() {
        let items = vec![
            // $120/year = $10/month equivalent (lower)
            item_with_cadence("NewsCo", 120.0, "yearly", false),
            // $50/month = $600/year (higher)
            monthly("StreamBundle", 50.0),
        ];
        let result = compute_subscription_audit(&items);
        assert_eq!(result.subscriptions.len(), 2);
        assert_eq!(
            result.subscriptions[0].merchant, "StreamBundle",
            "StreamBundle (600/year) must be ranked above NewsCo (120/year)"
        );
        assert_eq!(result.subscriptions[1].merchant, "NewsCo");
    }

    // 9c TRIANGULATE: totals sum all subscriptions
    #[test]
    fn totals_sum_all_monthly_and_annual_costs() {
        let items = vec![
            monthly("StreamBundle", 50.0),                       // 50/mo, 600/yr
            item_with_cadence("NewsCo", 120.0, "yearly", false), // 10/mo, 120/yr
        ];
        let result = compute_subscription_audit(&items);
        assert!(
            (result.total_monthly - 60.0).abs() < 0.01,
            "expected total_monthly 60.00, got {}",
            result.total_monthly
        );
        assert!(
            (result.total_annual - 720.0).abs() < 0.01,
            "expected total_annual 720.00, got {}",
            result.total_annual
        );
    }

    // 9c TRIANGULATE: income streams excluded
    #[test]
    fn income_streams_excluded_from_audit() {
        let items = vec![income("Employer", 5000.0), monthly("StreamBundle", 50.0)];
        let result = compute_subscription_audit(&items);
        assert_eq!(
            result.subscriptions.len(),
            1,
            "income stream must not appear in subscriptions"
        );
        assert_eq!(result.subscriptions[0].merchant, "StreamBundle");
    }

    // 9c TRIANGULATE: approximate streams included but flagged
    #[test]
    fn approximate_streams_included_and_flagged() {
        let items = vec![item_with_cadence("ElectricUtil", 120.0, "monthly", true)];
        let result = compute_subscription_audit(&items);
        assert_eq!(result.subscriptions.len(), 1);
        assert!(
            result.subscriptions[0].approximate,
            "approximate stream must be flagged approximate=true"
        );
        assert_eq!(result.subscriptions[0].merchant, "ElectricUtil");
    }

    // 9c TRIANGULATE: empty input → well-formed zero result
    #[test]
    fn empty_items_produce_zero_totals() {
        let result = compute_subscription_audit(&[]);
        assert!(result.subscriptions.is_empty());
        assert_eq!(result.total_monthly, 0.0);
        assert_eq!(result.total_annual, 0.0);
    }

    // 9c TRIANGULATE: unknown cadence treated as monthly (safe fallback)
    #[test]
    fn unknown_cadence_treated_as_monthly() {
        let items = vec![item_with_cadence(
            "UnknownService",
            25.0,
            "fortnightly",
            false,
        )];
        let result = compute_subscription_audit(&items);
        // Unknown cadence → factor 1.0 → monthly_amount = 25.0
        assert!(
            (result.subscriptions[0].monthly_amount - 25.0).abs() < 0.01,
            "unknown cadence must default to monthly factor (1.0), got {}",
            result.subscriptions[0].monthly_amount
        );
    }

    // 9c TRIANGULATE: cadence preserved in output
    #[test]
    fn cadence_preserved_in_subscription_entry() {
        let items = vec![item_with_cadence("NewsCo", 120.0, "yearly", false)];
        let result = compute_subscription_audit(&items);
        assert_eq!(result.subscriptions[0].cadence, "yearly");
    }

    // 9c TRIANGULATE: biweekly normalization
    #[test]
    fn biweekly_subscription_monthly_amount_normalizes_to_26_over_12() {
        // $100/biweekly → $100 * 26 / 12 ≈ $216.67/month
        let items = vec![item_with_cadence(
            "BiweeklyService",
            100.0,
            "biweekly",
            false,
        )];
        let result = compute_subscription_audit(&items);
        let expected_monthly = 100.0 * 26.0 / 12.0;
        assert!(
            (result.subscriptions[0].monthly_amount - expected_monthly).abs() < 0.01,
            "expected monthly_amount {:.4}, got {}",
            expected_monthly,
            result.subscriptions[0].monthly_amount
        );
    }
}
