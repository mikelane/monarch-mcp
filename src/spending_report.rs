//! Pure aggregation logic for the `spending_report` tool.
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_spending_report`].

use crate::client::{Budget, Cashflow, Transaction};
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Input types — subsets of client types we actually need
// ---------------------------------------------------------------------------

/// Spending and budget data for one category.
#[derive(Debug, Serialize, PartialEq)]
pub struct CategoryReport {
    pub spent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_of_budget: Option<i64>,
}

/// A possible duplicate charge pair.
#[derive(Debug, Serialize, PartialEq)]
pub struct DuplicateCharge {
    pub merchant: String,
    pub amount: f64,
    pub date: String,
}

/// Month-over-month spending comparison.
#[derive(Debug, Serialize, PartialEq)]
pub struct PriorPeriodComparison {
    /// Positive = spending went up, negative = spending went down.
    pub delta: f64,
}

/// The full spending report payload returned as JSON inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct SpendingReport {
    pub total_spent: f64,
    pub over_budget_categories: Vec<String>,
    pub by_category: HashMap<String, CategoryReport>,
    pub possible_duplicates: Vec<DuplicateCharge>,
    pub vs_prior_month: PriorPeriodComparison,
}

// ---------------------------------------------------------------------------
// Pure computation — no I/O
// ---------------------------------------------------------------------------

/// Compute the spending report from already-fetched Monarch data.
pub fn compute_spending_report(
    transactions: &[Transaction],
    budgets: &[Budget],
    cashflow: &Cashflow,
) -> SpendingReport {
    let by_category = aggregate_spending_by_category(transactions);
    let budget_map = build_budget_map(budgets);
    let category_reports = build_category_reports(&by_category, &budget_map);
    let over_budget_categories = find_over_budget_categories(&category_reports);
    // Delegate to the shared helper so spending_report and financial_overview
    // always agree on the definition of "true spending".
    let total_spent = compute_true_spending(transactions);
    let possible_duplicates = find_possible_duplicates(transactions);
    let prior_period = PriorPeriodComparison {
        delta: total_spent - cashflow.prior_month_spending,
    };

    SpendingReport {
        total_spent,
        over_budget_categories,
        by_category: category_reports,
        possible_duplicates,
        vs_prior_month: prior_period,
    }
}

/// Compute spend magnitude for a single transaction.
///
/// Monarch sign convention: expense outflows are **negative**. We convert to a
/// positive magnitude so `total_spent` and `percent_of_budget` are always ≥ 0.
///
/// Classification by `group_type`:
/// - `"expense"`: include magnitude `(-amount).max(0.0)`. A positive amount in
///   an expense category is a refund and contributes zero spend.
/// - `"income"` or `"transfer"`: always 0 — excluded from spending entirely.
/// - `None` (unknown group): defensive fallback — negative amounts count as
///   expense magnitude so real spending is never silently hidden. Positive
///   amounts contribute zero (treated as income/refund). See ADR 0004.
fn transaction_spend_magnitude(txn: &Transaction) -> f64 {
    match txn.category.group_type.as_deref() {
        Some("expense") => (-txn.amount).max(0.0),
        Some("income") | Some("transfer") => 0.0,
        _ => (-txn.amount).max(0.0), // defensive: sign-based fallback for unknown types
    }
}

/// Sum expense magnitudes per category name, excluding income and transfer categories.
///
/// Only categories whose `group_type` is `"expense"` (or unknown, via the
/// defensive fallback in `transaction_spend_magnitude`) contribute to the totals.
fn aggregate_spending_by_category(transactions: &[Transaction]) -> HashMap<String, f64> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for txn in transactions {
        let magnitude = transaction_spend_magnitude(txn);
        // Only include expense categories (and unknown-group fallback) in the map.
        // Income and transfer categories are excluded entirely so they never appear
        // in by_category, percent_of_budget, or over_budget_categories.
        if matches!(txn.category.group_type.as_deref(), Some("expense") | None) {
            *totals.entry(txn.category.name.clone()).or_insert(0.0) += magnitude;
        }
    }
    totals
}

/// Build a lookup map from category name to budget amount.
fn build_budget_map(budgets: &[Budget]) -> HashMap<String, f64> {
    budgets
        .iter()
        .map(|b| (b.category.name.clone(), b.amount))
        .collect()
}

/// Build per-category reports, attaching budget and percent-of-budget when available.
fn build_category_reports(
    spending: &HashMap<String, f64>,
    budget_map: &HashMap<String, f64>,
) -> HashMap<String, CategoryReport> {
    spending
        .iter()
        .map(|(category, &spent)| {
            let report = match budget_map.get(category) {
                Some(&budget) => CategoryReport {
                    spent,
                    budget: Some(budget),
                    percent_of_budget: percent_of_budget(spent, budget),
                },
                None => CategoryReport {
                    spent,
                    budget: None,
                    percent_of_budget: None,
                },
            };
            (category.clone(), report)
        })
        .collect()
}

/// Round (spent / budget_magnitude * 100) to the nearest whole percent.
///
/// Monarch stores expense budgets as negative `plannedCashFlowAmount` values
/// (e.g., "Loan Repayment" → `-1280`). We compare magnitudes so that a budget
/// of either `1280` or `-1280` yields the same percent for a given spend.
///
/// Returns `None` when `budget` is zero to avoid a divide-by-zero producing
/// `inf` (which casts to `i64::MAX`) or `NaN` (which casts to `0`).
fn percent_of_budget(spent: f64, budget: f64) -> Option<i64> {
    let magnitude = budget.abs();
    if magnitude == 0.0 {
        return None;
    }
    Some(((spent / magnitude) * 100.0).round() as i64)
}

/// A category is over budget only when spending strictly exceeds the budget magnitude.
///
/// Monarch stores expense budgets as negative `plannedCashFlowAmount` values
/// (e.g., "Loan Repayment" → `-1280`). We compare `spent` against `budget.abs()`
/// so that both positive-budget and negative-budget categories use the same
/// magnitude comparison. "Over budget" means "spent more than the planned amount".
fn find_over_budget_categories(category_reports: &HashMap<String, CategoryReport>) -> Vec<String> {
    let mut over_budget: Vec<String> = category_reports
        .iter()
        .filter_map(|(name, report)| {
            report.budget.and_then(|budget| {
                if report.spent > budget.abs() {
                    Some(name.clone())
                } else {
                    None
                }
            })
        })
        .collect();
    // Sort for deterministic output
    over_budget.sort();
    over_budget
}

/// Find transactions with the same merchant, same amount, and same date.
/// Each such group is reported as one `DuplicateCharge` entry.
fn find_possible_duplicates(transactions: &[Transaction]) -> Vec<DuplicateCharge> {
    let mut seen: HashMap<(&str, i64, &str), usize> = HashMap::new();
    let mut duplicates: Vec<DuplicateCharge> = Vec::new();
    let mut already_flagged: std::collections::HashSet<(&str, i64, &str)> =
        std::collections::HashSet::new();

    for txn in transactions {
        // Use integer representation of amount (cents) to avoid float key issues
        let amount_cents = (txn.amount * 100.0).round() as i64;
        let key = (txn.merchant_name.as_str(), amount_cents, txn.date.as_str());

        let count = seen.entry(key).or_insert(0);
        *count += 1;

        if *count == 2 && !already_flagged.contains(&key) {
            already_flagged.insert(key);
            duplicates.push(DuplicateCharge {
                merchant: txn.merchant_name.clone(),
                amount: txn.amount,
                date: txn.date.clone(),
            });
        }
    }

    duplicates
}

// ---------------------------------------------------------------------------
// Shared true-spending helper — used by both spending_report and financial_overview
// ---------------------------------------------------------------------------

/// Sum expense magnitudes across transactions, excluding income and transfer groups.
///
/// "True spending" = money that actually left the family. Credit card payments
/// and transfers move money between the family's own accounts — they are
/// inter-account movements, not consumption. Income is an inflow.
///
/// Classification follows the same rules as [`transaction_spend_magnitude`]:
/// - `"expense"` group: magnitude of outflow (negative amount → positive value)
/// - `"income"` or `"transfer"` group: 0 (excluded entirely)
/// - `None` (unknown group): sign-based fallback — negative amounts count as spend
pub fn compute_true_spending(transactions: &[Transaction]) -> f64 {
    transactions.iter().map(transaction_spend_magnitude).sum()
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
//
// Fixtures use the REAL Monarch sign convention:
//   - expense outflows: NEGATIVE amounts (e.g. -850.0 for dining)
//   - income: POSITIVE amounts (e.g. +5000.0 for a paycheck)
//   - refunds landing in expense categories: POSITIVE (e.g. +50.0 for a medical refund)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Budget, Cashflow, Category, Transaction};

    /// Build an expense transaction (negative amount, group_type = "expense").
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

    /// Build an income transaction (positive amount, group_type = "income").
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

    /// Build a transfer transaction (positive on the receiving side, group_type = "transfer").
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

    fn make_budget(category: &str, amount: f64) -> Budget {
        Budget {
            category: Category {
                name: category.to_string(),
                group_type: None,
            },
            amount,
        }
    }

    fn zero_cashflow() -> Cashflow {
        Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 0.0,
        }
    }

    // -----------------------------------------------------------------------
    // Real sign convention: expense amounts are NEGATIVE in Monarch.
    // over-budget flag — spending magnitude strictly exceeds budget magnitude.
    // -----------------------------------------------------------------------

    #[test]
    fn negative_expense_over_budget_is_flagged() {
        // -850.0 dining against a 600.0 budget → magnitude 850 > 600 → over budget.
        let txns = vec![make_expense_txn(
            "Dining merchant",
            -850.0,
            "Dining",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Dining", 600.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            report
                .over_budget_categories
                .contains(&"Dining".to_string()),
            "expected Dining in over_budget_categories: {:?}",
            report.over_budget_categories
        );
    }

    #[test]
    fn negative_expense_exactly_at_budget_is_not_flagged() {
        let txns = vec![make_expense_txn(
            "Groceries merchant",
            -900.0,
            "Groceries",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Groceries", 900.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Groceries".to_string()),
            "Groceries at 100% should not be flagged: {:?}",
            report.over_budget_categories
        );
    }

    #[test]
    fn negative_expense_under_budget_is_not_flagged() {
        let txns = vec![make_expense_txn(
            "Groceries merchant",
            -720.0,
            "Groceries",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Groceries", 900.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Groceries".to_string()),
            "Groceries under budget should not be flagged"
        );
    }

    #[test]
    fn all_over_budget_expense_categories_are_flagged() {
        let txns = vec![
            make_expense_txn("Dining merchant", -850.0, "Dining", "2026-05-15"),
            make_expense_txn("Shopping merchant", -500.0, "Shopping", "2026-05-15"),
        ];
        let budgets = vec![make_budget("Dining", 600.0), make_budget("Shopping", 400.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            report
                .over_budget_categories
                .contains(&"Dining".to_string()),
            "Dining should be over budget"
        );
        assert!(
            report
                .over_budget_categories
                .contains(&"Shopping".to_string()),
            "Shopping should be over budget"
        );
    }

    // -----------------------------------------------------------------------
    // Income categories are NEVER flagged as over-budget (#24 core fix).
    // -----------------------------------------------------------------------

    #[test]
    fn income_category_is_never_flagged_as_over_budget() {
        // A large paycheck — positive amount, group_type = "income".
        // Even if there's a budget entry for it, income must not appear in over_budget.
        let txns = vec![make_income_txn(
            "Employer",
            5000.0,
            "Paychecks",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Paychecks", 100.0)]; // contrived tiny budget
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Paychecks".to_string()),
            "income category Paychecks must never appear in over_budget_categories"
        );
    }

    // -----------------------------------------------------------------------
    // Transfer categories are NEVER included in total_spent or over-budget.
    // -----------------------------------------------------------------------

    #[test]
    fn transfer_category_is_excluded_from_spending() {
        // Credit-card payment — transfer group.
        let txns = vec![make_transfer_txn(
            "Chase CC Payment",
            2000.0,
            "Credit Card Payment",
            "2026-05-15",
        )];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert_eq!(
            report.total_spent, 0.0,
            "transfer category must not contribute to total_spent"
        );
        assert!(
            !report
                .over_budget_categories
                .contains(&"Credit Card Payment".to_string()),
            "transfer category must not appear in over_budget_categories"
        );
    }

    // -----------------------------------------------------------------------
    // total_spent reflects ONLY expense magnitudes (#24 core fix).
    // -----------------------------------------------------------------------

    #[test]
    fn total_spent_excludes_income_and_includes_only_expense_magnitudes() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-05-15"),
            make_expense_txn("Whole Foods", -365.0, "Groceries", "2026-05-16"),
        ];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert_eq!(
            report.total_spent, 365.0,
            "total_spent must equal grocery magnitude (365), not net 5000-365=4635"
        );
    }

    #[test]
    fn total_spent_is_sum_of_expense_magnitudes_only() {
        let txns = vec![
            make_expense_txn("Dining merchant", -850.0, "Dining", "2026-05-15"),
            make_expense_txn("Groceries merchant", -720.0, "Groceries", "2026-05-15"),
        ];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert_eq!(report.total_spent, 1570.0);
    }

    // -----------------------------------------------------------------------
    // Refund landing in an expense category (positive amount) contributes
    // zero spend magnitude — not flagged as over-budget (#24 core fix).
    // -----------------------------------------------------------------------

    #[test]
    fn expense_category_with_only_a_refund_is_not_flagged() {
        // Medical refund: positive amount but group_type = "expense".
        // The (-amount).max(0.0) logic produces 0 for this transaction.
        let txns = vec![make_expense_txn("Insurer", 50.0, "Medical", "2026-05-15")];
        let budgets = vec![make_budget("Medical", 200.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Medical".to_string()),
            "expense category with only a positive refund must not be over-budget"
        );
        let cat = report.by_category.get("Medical").unwrap();
        assert_eq!(
            cat.spent, 0.0,
            "refund-only category must report 0 spend, not negative"
        );
    }

    // -----------------------------------------------------------------------
    // percent_of_budget is positive for real expense transactions (#24).
    // -----------------------------------------------------------------------

    #[test]
    fn percent_of_budget_is_positive_for_negative_expense() {
        // -850 spent against 600 budget → magnitude 850 / 600 = 141.67 → 142%.
        // Not -142% as the old code produced.
        assert_eq!(percent_of_budget(850.0, 600.0), Some(142));
        assert_eq!(percent_of_budget(900.0, 900.0), Some(100));
    }

    #[test]
    fn percent_of_budget_rounds_correctly() {
        assert_eq!(percent_of_budget(1.0, 3.0), Some(33));
        assert_eq!(percent_of_budget(2.0, 3.0), Some(67));
    }

    #[test]
    fn expense_category_report_includes_positive_percent() {
        let txns = vec![make_expense_txn(
            "Dining merchant",
            -850.0,
            "Dining",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Dining", 600.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        let cat = report.by_category.get("Dining").unwrap();
        assert_eq!(cat.percent_of_budget, Some(142));
        assert!(
            cat.spent >= 0.0,
            "spent must be non-negative magnitude, got {}",
            cat.spent
        );
    }

    // -----------------------------------------------------------------------
    // Unbudgeted expense category — reported but not flagged.
    // -----------------------------------------------------------------------

    #[test]
    fn unbudgeted_expense_category_is_not_flagged_as_over_budget() {
        let txns = vec![make_expense_txn(
            "Travel merchant",
            -300.0,
            "Travel",
            "2026-05-15",
        )];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Travel".to_string()),
            "unbudgeted Travel should not be flagged"
        );
        let cat = report.by_category.get("Travel").unwrap();
        assert_eq!(cat.spent, 300.0);
        assert_eq!(cat.budget, None);
        assert_eq!(cat.percent_of_budget, None);
    }

    // -----------------------------------------------------------------------
    // Duplicate detection — same merchant + amount + date.
    // -----------------------------------------------------------------------

    #[test]
    fn identical_charges_same_day_flagged_as_duplicate() {
        let txns = vec![
            make_expense_txn("Acme Streaming", -49.99, "Subscriptions", "2026-05-14"),
            make_expense_txn("Acme Streaming", -49.99, "Subscriptions", "2026-05-14"),
        ];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        let merchants: Vec<&str> = report
            .possible_duplicates
            .iter()
            .map(|d| d.merchant.as_str())
            .collect();
        assert!(
            merchants.contains(&"Acme Streaming"),
            "expected Acme Streaming in duplicates: {:?}",
            merchants
        );
    }

    #[test]
    fn same_merchant_different_amount_not_a_duplicate() {
        let txns = vec![
            make_expense_txn("Acme Streaming", -49.99, "Subscriptions", "2026-05-14"),
            make_expense_txn("Acme Streaming", -9.99, "Subscriptions", "2026-05-14"),
        ];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        let merchants: Vec<&str> = report
            .possible_duplicates
            .iter()
            .map(|d| d.merchant.as_str())
            .collect();
        assert!(
            !merchants.contains(&"Acme Streaming"),
            "different amounts should not be a duplicate: {:?}",
            merchants
        );
    }

    // -----------------------------------------------------------------------
    // Prior-period delta — uses total_spent (expense magnitudes only).
    // -----------------------------------------------------------------------

    #[test]
    fn prior_period_delta_is_positive_when_spending_increased() {
        let txns = vec![make_expense_txn(
            "Various",
            -4600.0,
            "General",
            "2026-05-15",
        )];
        let cashflow = Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 4000.0,
        };
        let report = compute_spending_report(&txns, &[], &cashflow);
        assert_eq!(report.vs_prior_month.delta, 600.0);
    }

    #[test]
    fn prior_period_delta_is_negative_when_spending_decreased() {
        let txns = vec![make_expense_txn(
            "Various",
            -3000.0,
            "General",
            "2026-05-15",
        )];
        let cashflow = Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 4000.0,
        };
        let report = compute_spending_report(&txns, &[], &cashflow);
        assert_eq!(report.vs_prior_month.delta, -1000.0);
    }

    // -----------------------------------------------------------------------
    // compute_true_spending — shared helper, tested directly.
    // -----------------------------------------------------------------------

    #[test]
    fn true_spending_sums_expense_magnitudes() {
        let txns = vec![
            make_expense_txn("Groceries", -365.0, "Groceries", "2026-05-10"),
            make_expense_txn("Dining", -200.0, "Dining", "2026-05-15"),
        ];
        assert_eq!(compute_true_spending(&txns), 565.0);
    }

    #[test]
    fn true_spending_excludes_income_group() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-05-01"),
            make_expense_txn("Groceries", -365.0, "Groceries", "2026-05-10"),
        ];
        assert_eq!(compute_true_spending(&txns), 365.0);
    }

    #[test]
    fn true_spending_excludes_transfer_group() {
        let txns = vec![
            make_transfer_txn("Chase", -3299.02, "Credit Card Payment", "2026-05-05"),
            make_expense_txn("Groceries", -731.27, "Groceries", "2026-05-10"),
        ];
        let result = compute_true_spending(&txns);
        // Only the grocery transaction should contribute
        assert!(
            (result - 731.27).abs() < 0.001,
            "expected 731.27, got {result}"
        );
    }

    #[test]
    fn true_spending_refund_in_expense_category_contributes_zero() {
        // A positive amount in an expense category is a refund → 0 spend
        let txns = vec![make_expense_txn("Insurer", 50.0, "Medical", "2026-05-15")];
        assert_eq!(compute_true_spending(&txns), 0.0);
    }

    #[test]
    fn true_spending_empty_slice_is_zero() {
        assert_eq!(compute_true_spending(&[]), 0.0);
    }

    // -----------------------------------------------------------------------
    // Empty period.
    // -----------------------------------------------------------------------

    #[test]
    fn empty_period_reports_zero_spend_and_no_flags() {
        let report = compute_spending_report(&[], &[], &zero_cashflow());
        assert_eq!(report.total_spent, 0.0);
        assert!(report.over_budget_categories.is_empty());
        assert!(report.by_category.is_empty());
        assert!(report.possible_duplicates.is_empty());
    }

    // -----------------------------------------------------------------------
    // Zero budget with nonzero expense spend.
    // -----------------------------------------------------------------------

    #[test]
    fn zero_budget_with_expense_spend_produces_no_percent_and_is_over_budget() {
        let txns = vec![make_expense_txn(
            "Netflix",
            -15.99,
            "Streaming",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Streaming", 0.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());

        let cat = report
            .by_category
            .get("Streaming")
            .expect("Streaming category must exist");
        assert_eq!(
            cat.percent_of_budget, None,
            "zero budget should yield no percent (division by zero)"
        );
        assert!(
            report
                .over_budget_categories
                .contains(&"Streaming".to_string()),
            "spending on a $0-budget category must still be classified over budget"
        );
    }

    // -----------------------------------------------------------------------
    // Negative-budget (loan-repayment style) — Monarch stores planned
    // outflows as negative plannedCashFlowAmount values. Transactions for
    // expense categories are negative amounts in the real sign convention.
    // -----------------------------------------------------------------------

    #[test]
    fn negative_budget_under_magnitude_is_not_over_budget() {
        // Planned: -1280 (loan repayment). Actual spend: -1000 → magnitude 1000 < 1280.
        let txns = vec![make_expense_txn(
            "Loan Co",
            -1000.0,
            "Loan Repayment",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Loan Repayment", -1280.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Loan Repayment".to_string()),
            "spending 1000 against a -1280 budget should NOT be over budget (1000 < 1280)"
        );
    }

    #[test]
    fn negative_budget_over_magnitude_is_flagged_as_over_budget() {
        // Planned: -1280. Actual spend: -1400 → magnitude 1400 > 1280 → over budget.
        let txns = vec![make_expense_txn(
            "Loan Co",
            -1400.0,
            "Loan Repayment",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Loan Repayment", -1280.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            report
                .over_budget_categories
                .contains(&"Loan Repayment".to_string()),
            "spending 1400 against a -1280 budget SHOULD be over budget (1400 > 1280)"
        );
    }

    #[test]
    fn negative_budget_percent_of_budget_is_positive_and_sane() {
        // 1000 spent against -1280 budget → 1000/1280 * 100 = 78%  (not negative)
        assert_eq!(percent_of_budget(1000.0, -1280.0), Some(78));
    }

    #[test]
    fn negative_budget_exactly_at_magnitude_is_not_over_budget() {
        // Planned: -1280. Actual spend: -1280 → at budget, not over.
        let txns = vec![make_expense_txn(
            "Loan Co",
            -1280.0,
            "Loan Repayment",
            "2026-05-15",
        )];
        let budgets = vec![make_budget("Loan Repayment", -1280.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Loan Repayment".to_string()),
            "spending exactly 1280 against a -1280 budget should NOT be over budget"
        );
        let cat = report.by_category.get("Loan Repayment").unwrap();
        assert_eq!(cat.percent_of_budget, Some(100));
    }

    #[test]
    fn negative_budget_zero_spend_is_not_over_budget() {
        let budgets = vec![make_budget("Loan Repayment", -1280.0)];
        let report = compute_spending_report(&[], &budgets, &zero_cashflow());
        assert!(
            !report
                .over_budget_categories
                .contains(&"Loan Repayment".to_string()),
            "zero spend against a negative budget should not be over budget"
        );
    }

    // -----------------------------------------------------------------------
    // Defensive fallback: group_type = None → treat sign alone.
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_group_type_negative_amount_counts_as_expense_spend() {
        // A transaction whose group_type is missing (None) but whose amount is
        // negative falls back to the sign-based heuristic: treated as expense.
        let txn = Transaction {
            id: "unknown-1".into(),
            amount: -100.0,
            date: "2026-05-15".into(),
            merchant_name: "Mystery Co".into(),
            category: Category {
                name: "Unknown".into(),
                group_type: None,
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        };
        let report = compute_spending_report(&[txn], &[], &zero_cashflow());
        assert_eq!(
            report.total_spent, 100.0,
            "negative-amount transaction with no group_type must count as 100 spend"
        );
    }

    #[test]
    fn unknown_group_type_positive_amount_is_not_counted_as_spend() {
        // Unknown group_type with positive amount → treated as income/refund, 0 spend.
        let txn = Transaction {
            id: "unknown-2".into(),
            amount: 200.0,
            date: "2026-05-15".into(),
            merchant_name: "Mystery Refund".into(),
            category: Category {
                name: "Unknown".into(),
                group_type: None,
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        };
        let report = compute_spending_report(&[txn], &[], &zero_cashflow());
        assert_eq!(
            report.total_spent, 0.0,
            "positive-amount transaction with no group_type must not count as spend"
        );
    }

    // -----------------------------------------------------------------------
    // Zero budget with zero spend — no percent, not over-budget.
    // -----------------------------------------------------------------------

    #[test]
    fn zero_budget_with_zero_spend_produces_no_percent_and_is_not_over_budget() {
        let txns = vec![make_expense_txn("Netflix", 0.0, "Streaming", "2026-05-15")];
        let budgets = vec![make_budget("Streaming", 0.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());

        let cat = report
            .by_category
            .get("Streaming")
            .expect("Streaming category must exist");
        assert_eq!(
            cat.percent_of_budget, None,
            "zero budget with zero spend should yield None percent (0/0 = NaN, not 0)"
        );
        assert!(
            !report
                .over_budget_categories
                .contains(&"Streaming".to_string()),
            "zero spend on $0-budget category is not over budget"
        );
    }
}
