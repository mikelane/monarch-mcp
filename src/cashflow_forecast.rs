//! Cash flow forecast — projects the household's month-end position.
//!
//! Combines current account balances with upcoming (not-yet-past) recurring
//! charges to produce a projected month-end balance and a shortfall flag.

use serde::Serialize;

/// One upcoming (or past) recurring charge as fetched from Monarch.
///
/// `amount` follows the Monarch sign convention: negative for outflows
/// (bills/subscriptions), positive for inflows (income streams).
#[derive(Debug)]
pub struct RecurringItem {
    pub merchant: String,
    /// Negative for outflows, positive for inflows — Monarch convention.
    pub amount: f64,
    pub is_past: bool,
}

/// Output of `compute_forecast`.
#[derive(Debug, Serialize)]
pub struct ForecastResult {
    pub projected_month_end_balance: f64,
    pub shortfall: bool,
    pub shortfall_amount: f64,
    pub shortfall_drivers: Vec<String>,
}

/// Project month-end position from current balance and upcoming recurring charges.
///
/// Past items (`is_past = true`) are excluded — they already cleared and are
/// captured in the current balance. Outflows have negative amounts; inflows
/// have positive amounts (Monarch sign convention).
pub fn compute_forecast(current_balance: f64, items: &[RecurringItem]) -> ForecastResult {
    let upcoming: Vec<&RecurringItem> = items.iter().filter(|i| !i.is_past).collect();

    let projected = upcoming
        .iter()
        .fold(current_balance, |acc, item| acc + item.amount);

    let shortfall = projected < 0.0;
    let shortfall_amount = if shortfall { projected.abs() } else { 0.0 };
    let shortfall_drivers = if shortfall {
        upcoming
            .iter()
            .filter(|i| i.amount < 0.0)
            .map(|i| i.merchant.clone())
            .collect()
    } else {
        vec![]
    };

    ForecastResult {
        projected_month_end_balance: projected,
        shortfall,
        shortfall_amount,
        shortfall_drivers,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn bill(merchant: &str, amount: f64) -> RecurringItem {
        RecurringItem {
            merchant: merchant.to_string(),
            amount: -amount.abs(),
            is_past: false,
        }
    }

    fn past_bill(merchant: &str, amount: f64) -> RecurringItem {
        RecurringItem {
            merchant: merchant.to_string(),
            amount: -amount.abs(),
            is_past: true,
        }
    }

    // empty bill list leaves balance unchanged
    #[test]
    fn no_upcoming_bills_leaves_balance_unchanged() {
        let result = compute_forecast(2000.0, &[]);
        assert!((result.projected_month_end_balance - 2000.0).abs() < 0.01);
        assert!(!result.shortfall);
        assert_eq!(result.shortfall_amount, 0.0);
        assert!(result.shortfall_drivers.is_empty());
    }

    // 9a RED → 9b GREEN: basic projection with upcoming bills reduces balance
    #[test]
    fn positive_projection_when_balance_covers_all_bills() {
        let items = vec![
            bill("Rent", 1500.0),
            bill("Electric", 120.0),
            bill("Internet", 80.0),
        ];
        let result = compute_forecast(3000.0, &items);
        assert!((result.projected_month_end_balance - 1300.0).abs() < 0.01);
        assert!(!result.shortfall);
    }

    // 9c TRIANGULATE: shortfall when bills exceed balance
    #[test]
    fn shortfall_flagged_when_upcoming_bills_exceed_balance() {
        let items = vec![
            bill("Rent", 1500.0),
            bill("Subscription", 15.0),
        ];
        let result = compute_forecast(500.0, &items);
        assert!(result.shortfall, "expected shortfall flag");
        assert!((result.shortfall_amount - 1015.0).abs() < 0.01);
        assert!(result.shortfall_drivers.contains(&"Rent".to_string()));
        assert!(result.shortfall_drivers.contains(&"Subscription".to_string()));
    }

    // 9c TRIANGULATE: past items excluded from projection
    #[test]
    fn past_recurring_items_are_excluded_from_projection() {
        let items = vec![
            past_bill("Rent", 1200.0),
            bill("Electric", 100.0),
        ];
        let result = compute_forecast(1800.0, &items);
        assert!((result.projected_month_end_balance - 1700.0).abs() < 0.01);
        assert!(!result.shortfall);
    }
}
