//! Pure aggregation logic for the `savings_rate` tool.
//!
//! Computes per-month income, true spending, net savings, and savings rate
//! across a multi-month range. All arithmetic lives here, free of I/O, so it
//! can be unit-tested without standing up a mock server.
//!
//! ## Income definition
//!
//! Income = sum of positive amounts in `"income"` group transactions.
//! Refunds landing in expense categories are NOT income — they reduce spending
//! (via [`transaction_spend_magnitude`]) but are not treated as positive income.
//! This matches ADR 0012.
//!
//! ## Spending definition
//!
//! True spending reuses [`compute_true_spending`] from `spending_report.rs`
//! unchanged so the exclusion rules (Transfer / Credit Card Payment /
//! Paychecks / Other Income / Interest, refund-netting) are identical to
//! every other spend number in the server.
//!
//! ## Savings rate
//!
//! `savings_rate = (income − true_spending) / income`
//!
//! When `income == 0` the rate is `None` (not `inf`/`NaN`).

use crate::client::Transaction;
use crate::spending_report::compute_true_spending;
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Per-month income/spending/savings summary.
#[derive(Debug, Serialize, PartialEq)]
pub struct MonthlySavings {
    /// Calendar month in `YYYY-MM` format.
    pub month: String,
    /// Sum of positive income-group transaction amounts.
    pub income: f64,
    /// True spending (expense magnitudes, income/transfer excluded).
    pub true_spending: f64,
    /// `income − true_spending` (positive = net saver, negative = deficit).
    pub net_savings: f64,
    /// `(income − true_spending) / income` as a percentage (0–100 scale), or
    /// `None` when income is zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub savings_rate: Option<f64>,
}

/// The full savings-rate payload returned by the `savings_rate` tool.
#[derive(Debug, Serialize, PartialEq)]
pub struct SavingsRateResult {
    /// One entry per complete calendar month, oldest-first.
    pub months: Vec<MonthlySavings>,
    /// ISO-8601 start date of the range queried.
    pub range_start: String,
    /// ISO-8601 end date of the range queried (inclusive, last day of month).
    pub range_end: String,
    /// Average savings rate across all months that have non-zero income,
    /// expressed as a percentage (0–100 scale). `None` when no month has income.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_average_savings_rate: Option<f64>,
}

// ---------------------------------------------------------------------------
// Income helper
// ---------------------------------------------------------------------------

/// Sum income for a single transaction.
///
/// Income = positive amounts in `"income"` group only.
/// Refunds in expense categories (positive amounts, group_type "expense") do
/// NOT count as income — they reduce spending but are not a new income flow.
fn transaction_income_amount(txn: &Transaction) -> f64 {
    match txn.category.group_type.as_deref() {
        Some("income") => txn.amount.max(0.0),
        _ => 0.0,
    }
}

/// Sum income across all transactions in a slice.
fn compute_income(transactions: &[Transaction]) -> f64 {
    transactions.iter().map(transaction_income_amount).sum()
}

// ---------------------------------------------------------------------------
// Month-bucketing helpers (I/O-free, hand-rolled per CLAUDE.md)
// ---------------------------------------------------------------------------

/// Determine the YYYY-MM bucket for a transaction date string.
///
/// Returns `None` when the date is not a valid ISO-8601 date or contains
/// non-ASCII characters that would cause a byte-index panic.
fn month_bucket(date: &str) -> Option<String> {
    let bytes = date.as_bytes().get(..7)?;
    let prefix = std::str::from_utf8(bytes).ok()?;
    if prefix.as_bytes().get(4).copied() != Some(b'-') {
        return None;
    }
    Some(prefix.to_string())
}

/// Enumerate all `YYYY-MM` labels between `range_start` and `range_end`
/// (inclusive), oldest-first.
fn enumerate_months_in_range(range_start: &str, range_end: &str) -> Vec<String> {
    let start_prefix = match range_start
        .as_bytes()
        .get(..7)
        .and_then(|b| std::str::from_utf8(b).ok())
    {
        Some(p) => p,
        None => return vec![],
    };
    let end_prefix = match range_end
        .as_bytes()
        .get(..7)
        .and_then(|b| std::str::from_utf8(b).ok())
    {
        Some(p) => p,
        None => return vec![],
    };

    let parse_ym = |s: &str| -> Option<(i64, u32)> {
        let mut parts = s.splitn(2, '-');
        let y: i64 = parts.next()?.parse().ok()?;
        let m: u32 = parts.next()?.parse().ok()?;
        Some((y, m))
    };

    let (mut y, mut m) = match parse_ym(start_prefix) {
        Some(v) => v,
        None => return vec![],
    };
    let (end_y, end_m) = match parse_ym(end_prefix) {
        Some(v) => v,
        None => return vec![],
    };

    let mut labels = Vec::new();
    loop {
        if y > end_y || (y == end_y && m > end_m) {
            break;
        }
        labels.push(format!("{y:04}-{m:02}"));
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    labels
}

// ---------------------------------------------------------------------------
// Core computation
// ---------------------------------------------------------------------------

/// Compute savings rate summaries from already-fetched transactions.
///
/// Transactions are bucketed by `YYYY-MM` date prefix. Only months within
/// `[range_start, range_end]` are included.
///
/// Per-month:
/// - Income = positive amounts in `"income"` group.
/// - True spending = [`compute_true_spending`] (same rules as spending_report).
/// - Net savings = income − true_spending.
/// - Savings rate = net_savings / income × 100, or None when income = 0.
///
/// Window average = mean of per-month savings rates over months with income.
pub fn compute_savings_rate(
    transactions: &[Transaction],
    range_start: &str,
    range_end: &str,
) -> SavingsRateResult {
    let month_labels = enumerate_months_in_range(range_start, range_end);

    // Group transactions by their YYYY-MM bucket (owned, not refs).
    let mut bucket_map: HashMap<String, Vec<Transaction>> = HashMap::new();
    for txn in transactions {
        if let Some(bucket) = month_bucket(&txn.date) {
            if month_labels.contains(&bucket) {
                bucket_map.entry(bucket).or_default().push(txn.clone());
            }
        }
    }

    let mut months: Vec<MonthlySavings> = month_labels
        .into_iter()
        .map(|label| {
            let txns = bucket_map.get(&label).map_or(&[][..], Vec::as_slice);
            build_monthly_savings(label, txns)
        })
        .collect();

    // Sort oldest-first (enumerate_months_in_range already produces this order,
    // but be explicit).
    months.sort_by(|a, b| a.month.cmp(&b.month));

    let window_average_savings_rate = compute_window_average(&months);

    SavingsRateResult {
        months,
        range_start: range_start.to_string(),
        range_end: range_end.to_string(),
        window_average_savings_rate,
    }
}

/// Build one [`MonthlySavings`] entry from a slice of transactions.
fn build_monthly_savings(month: String, transactions: &[Transaction]) -> MonthlySavings {
    let income = compute_income(transactions);
    let true_spending = compute_true_spending(transactions);
    let net_savings = income - true_spending;
    // SAFETY: Monarch amounts are bounded by API constraints; this guard defends
    // against overflow-to-inf, which would produce NaN/null in JSON. Non-finite
    // income → None (omitted), same as zero-income, keeping the JSON contract NaN-free.
    let savings_rate = if income > 0.0 && income.is_finite() {
        Some((net_savings / income) * 100.0)
    } else {
        None
    };

    MonthlySavings {
        month,
        income,
        true_spending,
        net_savings,
        savings_rate,
    }
}

/// Compute the average savings rate over months that have non-zero income.
///
/// Returns `None` when no month has income (no finite rate exists).
fn compute_window_average(months: &[MonthlySavings]) -> Option<f64> {
    let rates: Vec<f64> = months.iter().filter_map(|m| m.savings_rate).collect();
    if rates.is_empty() {
        None
    } else {
        Some(rates.iter().sum::<f64>() / rates.len() as f64)
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
//
// Fixtures use the REAL Monarch sign convention:
//   - expense outflows: NEGATIVE amounts (e.g. -850.0 for dining)
//   - income: POSITIVE amounts (e.g. +5000.0 for a paycheck)
//   - refunds landing in expense categories: POSITIVE (e.g. +50.0 for medical refund)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Category, Transaction};

    fn make_expense_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("expense".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    fn make_income_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("income".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    fn make_transfer_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("transfer".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    // -----------------------------------------------------------------------
    // Income computation
    // -----------------------------------------------------------------------

    #[test]
    fn income_sums_positive_income_group_amounts() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-04-01"),
            make_income_txn("Freelance", 1000.0, "Other Income", "2026-04-15"),
        ];
        assert_eq!(compute_income(&txns), 6000.0);
    }

    #[test]
    fn income_excludes_expense_group_transactions() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("Whole Foods", -365.0, "Groceries", "2026-04-10"),
        ];
        assert_eq!(compute_income(&txns), 5000.0);
    }

    #[test]
    fn income_excludes_transfer_group_transactions() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-04-01"),
            make_transfer_txn("Chase", 2000.0, "Credit Card Payment", "2026-04-05"),
        ];
        assert_eq!(compute_income(&txns), 5000.0);
    }

    #[test]
    fn income_does_not_count_expense_category_refunds_as_income() {
        // A refund in an expense category (positive amount, group_type "expense")
        // must NOT be counted as income — it reduces spending, not adds income.
        let txns = vec![make_expense_txn("Insurer", 200.0, "Medical", "2026-04-15")];
        assert_eq!(compute_income(&txns), 0.0);
    }

    #[test]
    fn income_zero_for_empty_slice() {
        assert_eq!(compute_income(&[]), 0.0);
    }

    // -----------------------------------------------------------------------
    // Rate math
    // -----------------------------------------------------------------------

    #[test]
    fn savings_rate_is_25_percent_for_6000_income_4500_spending() {
        // income=6000, spending=4500, net=1500, rate=1500/6000=25%
        let txns = vec![
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("Various", -4500.0, "Expenses", "2026-04-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        assert_eq!(result.months.len(), 1);
        let m = &result.months[0];
        assert_eq!(m.income, 6000.0);
        assert_eq!(m.true_spending, 4500.0);
        assert_eq!(m.net_savings, 1500.0);
        let rate = m
            .savings_rate
            .expect("savings_rate must be Some when income > 0");
        assert!((rate - 25.0).abs() < 0.001, "expected 25%, got {rate}");
    }

    #[test]
    fn savings_rate_is_none_when_income_is_zero() {
        let txns = vec![make_expense_txn(
            "Various",
            -2000.0,
            "Expenses",
            "2026-04-15",
        )];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        assert_eq!(result.months.len(), 1);
        let m = &result.months[0];
        assert_eq!(m.income, 0.0);
        assert!(
            m.savings_rate.is_none(),
            "savings_rate must be None when income is zero, not inf/NaN"
        );
    }

    #[test]
    fn savings_rate_is_none_for_empty_month() {
        let result = compute_savings_rate(&[], "2026-04-01", "2026-04-30");
        assert_eq!(result.months.len(), 1);
        let m = &result.months[0];
        assert_eq!(m.income, 0.0);
        assert_eq!(m.true_spending, 0.0);
        assert_eq!(m.net_savings, 0.0);
        assert!(m.savings_rate.is_none());
    }

    // -----------------------------------------------------------------------
    // Transfer / credit-card exclusion from spending
    // -----------------------------------------------------------------------

    #[test]
    fn credit_card_payment_excluded_from_true_spending() {
        // 6000 income, 4000 expense, 2000 CC payment (transfer) → spending=4000
        let txns = vec![
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("Various", -4000.0, "Expenses", "2026-04-15"),
            make_transfer_txn("Chase", -2000.0, "Credit Card Payment", "2026-04-20"),
        ];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        let m = &result.months[0];
        assert_eq!(
            m.true_spending, 4000.0,
            "CC payment must not inflate true_spending"
        );
        // rate = (6000-4000)/6000 = 33.33%
        let rate = m.savings_rate.unwrap();
        assert!(
            (rate - 100.0 / 3.0).abs() < 0.01,
            "expected ~33.33%, got {rate}"
        );
    }

    // -----------------------------------------------------------------------
    // Multi-month window
    // -----------------------------------------------------------------------

    #[test]
    fn three_months_produces_three_entries_oldest_first() {
        let txns = vec![
            // Feb
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-02-01"),
            make_expense_txn("G1", -3000.0, "Groceries", "2026-02-15"),
            // Mar
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-03-01"),
            make_expense_txn("G2", -3600.0, "Groceries", "2026-03-15"),
            // Apr
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("G3", -4500.0, "Groceries", "2026-04-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-02-01", "2026-04-30");
        assert_eq!(result.months.len(), 3);
        assert_eq!(result.months[0].month, "2026-02");
        assert_eq!(result.months[1].month, "2026-03");
        assert_eq!(result.months[2].month, "2026-04");
    }

    #[test]
    fn window_average_is_mean_of_monthly_rates() {
        // Feb: 6000 income, 3000 spend → 50% rate
        // Mar: 6000 income, 3600 spend → 40% rate
        // Apr: 6000 income, 4500 spend → 25% rate
        // Average = (50+40+25)/3 = 38.33%
        let txns = vec![
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-02-01"),
            make_expense_txn("G1", -3000.0, "Groceries", "2026-02-15"),
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-03-01"),
            make_expense_txn("G2", -3600.0, "Groceries", "2026-03-15"),
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("G3", -4500.0, "Groceries", "2026-04-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-02-01", "2026-04-30");
        let avg = result
            .window_average_savings_rate
            .expect("window_average must be Some when months have income");
        let expected = (50.0 + 40.0 + 25.0) / 3.0;
        assert!(
            (avg - expected).abs() < 0.01,
            "expected {expected:.2}%, got {avg:.2}%"
        );
    }

    #[test]
    fn window_average_is_none_when_no_month_has_income() {
        let txns = vec![make_expense_txn("G1", -3000.0, "Groceries", "2026-04-15")];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        assert!(
            result.window_average_savings_rate.is_none(),
            "window_average must be None when no month has income"
        );
    }

    #[test]
    fn window_average_skips_zero_income_months() {
        // Jan: no income → rate=None (skipped)
        // Feb: 6000 income, 3000 spend → 50%
        // Average over income-months = 50%
        let txns = vec![
            make_expense_txn("G1", -1000.0, "Groceries", "2026-01-15"),
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-02-01"),
            make_expense_txn("G2", -3000.0, "Groceries", "2026-02-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-01-01", "2026-02-28");
        let avg = result
            .window_average_savings_rate
            .expect("window_average must be Some for Feb");
        assert!((avg - 50.0).abs() < 0.01, "expected 50%, got {avg}");
    }

    // -----------------------------------------------------------------------
    // Range metadata
    // -----------------------------------------------------------------------

    #[test]
    fn result_preserves_range_start_and_end() {
        let result = compute_savings_rate(&[], "2026-03-01", "2026-04-30");
        assert_eq!(result.range_start, "2026-03-01");
        assert_eq!(result.range_end, "2026-04-30");
    }

    // -----------------------------------------------------------------------
    // No raw transactions in output (structural)
    // -----------------------------------------------------------------------

    #[test]
    fn result_months_contain_only_aggregates_not_transaction_lists() {
        // The MonthlySavings struct has no transactions field — this is
        // enforced at the type level. We verify serde output has no transactions key.
        let txns = vec![
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("G1", -4500.0, "Groceries", "2026-04-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        let json = serde_json::to_value(&result).unwrap();
        let months = json["months"].as_array().unwrap();
        for m in months {
            assert!(
                m.get("transactions").is_none(),
                "monthly entry must not contain a transactions key: {m}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Negative savings rate (spending exceeds income)
    // -----------------------------------------------------------------------

    #[test]
    fn negative_savings_rate_when_spending_exceeds_income() {
        // income=3000, spending=5000, net=-2000, rate=-66.67%
        let txns = vec![
            make_income_txn("Employer", 3000.0, "Paychecks", "2026-04-01"),
            make_expense_txn("Various", -5000.0, "Expenses", "2026-04-15"),
        ];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        let m = &result.months[0];
        assert_eq!(m.net_savings, -2000.0);
        let rate = m.savings_rate.unwrap();
        assert!(
            rate < 0.0,
            "rate must be negative when spending > income, got {rate}"
        );
        // Pin exact value: -2000/3000*100 = -66.666...%
        assert!(
            (rate - (-2000.0 / 3000.0 * 100.0)).abs() < 0.001,
            "expected -66.67%, got {rate}"
        );
    }

    // -----------------------------------------------------------------------
    // Transactions outside range are excluded
    // -----------------------------------------------------------------------

    #[test]
    fn transactions_outside_range_are_excluded() {
        let txns = vec![
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-03-01"), // before range
            make_income_txn("Employer", 6000.0, "Paychecks", "2026-04-01"), // in range
            make_expense_txn("G1", -4500.0, "Groceries", "2026-04-15"),     // in range
            make_expense_txn("G2", -1000.0, "Groceries", "2026-05-05"),     // after range
        ];
        let result = compute_savings_rate(&txns, "2026-04-01", "2026-04-30");
        assert_eq!(result.months.len(), 1);
        let m = &result.months[0];
        assert_eq!(m.income, 6000.0, "only in-range income counted");
        assert_eq!(m.true_spending, 4500.0, "only in-range spending counted");
    }
}
