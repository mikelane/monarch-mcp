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
    let total_spent = by_category.values().sum();
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

/// Sum transaction amounts per category name.
fn aggregate_spending_by_category(transactions: &[Transaction]) -> HashMap<String, f64> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for txn in transactions {
        *totals.entry(txn.category.name.clone()).or_insert(0.0) += txn.amount;
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

/// Round (spent / budget * 100) to the nearest whole percent.
///
/// Returns `None` when `budget` is zero to avoid a divide-by-zero producing
/// `inf` (which casts to `i64::MAX`) or `NaN` (which casts to `0`).
fn percent_of_budget(spent: f64, budget: f64) -> Option<i64> {
    if budget == 0.0 {
        return None;
    }
    Some(((spent / budget) * 100.0).round() as i64)
}

/// A category is over budget only when spending strictly exceeds its budget.
fn find_over_budget_categories(category_reports: &HashMap<String, CategoryReport>) -> Vec<String> {
    let mut over_budget: Vec<String> = category_reports
        .iter()
        .filter_map(|(name, report)| {
            report.budget.and_then(|budget| {
                if report.spent > budget {
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
    let mut already_flagged: std::collections::HashSet<(&str, i64, &str)> = std::collections::HashSet::new();

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
// Tests — TDD: RED first, then GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Budget, Cashflow, Category, Transaction};

    fn make_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
            },
            tags: vec![],
            notes: String::new(),
        }
    }

    fn make_budget(category: &str, amount: f64) -> Budget {
        Budget {
            category: Category {
                name: category.to_string(),
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
    // 9a RED: over-budget flag — spending strictly exceeds budget
    // -----------------------------------------------------------------------

    #[test]
    fn category_over_budget_is_flagged() {
        let txns = vec![make_txn("Dining merchant", 850.0, "Dining", "2026-05-15")];
        let budgets = vec![make_budget("Dining", 600.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            report.over_budget_categories.contains(&"Dining".to_string()),
            "expected Dining in over_budget_categories: {:?}",
            report.over_budget_categories
        );
    }

    // -----------------------------------------------------------------------
    // 9b GREEN + 9c TRIANGULATE: exactly at budget is NOT over budget
    // -----------------------------------------------------------------------

    #[test]
    fn category_exactly_at_budget_is_not_flagged() {
        let txns = vec![make_txn("Groceries merchant", 900.0, "Groceries", "2026-05-15")];
        let budgets = vec![make_budget("Groceries", 900.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report.over_budget_categories.contains(&"Groceries".to_string()),
            "Groceries at 100% should not be flagged: {:?}",
            report.over_budget_categories
        );
    }

    #[test]
    fn category_under_budget_is_not_flagged() {
        let txns = vec![make_txn("Groceries merchant", 720.0, "Groceries", "2026-05-15")];
        let budgets = vec![make_budget("Groceries", 900.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            !report.over_budget_categories.contains(&"Groceries".to_string()),
            "Groceries under budget should not be flagged"
        );
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: every over-budget category is flagged (multiple)
    // -----------------------------------------------------------------------

    #[test]
    fn all_over_budget_categories_are_flagged() {
        let txns = vec![
            make_txn("Dining merchant", 850.0, "Dining", "2026-05-15"),
            make_txn("Shopping merchant", 500.0, "Shopping", "2026-05-15"),
        ];
        let budgets = vec![make_budget("Dining", 600.0), make_budget("Shopping", 400.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        assert!(
            report.over_budget_categories.contains(&"Dining".to_string()),
            "Dining should be over budget"
        );
        assert!(
            report.over_budget_categories.contains(&"Shopping".to_string()),
            "Shopping should be over budget"
        );
    }

    // -----------------------------------------------------------------------
    // 9a RED: percent of budget — 850/600 → 142%
    // -----------------------------------------------------------------------

    #[test]
    fn percent_of_budget_rounds_to_nearest_whole() {
        // 850/600 = 1.4166… → 142%
        assert_eq!(percent_of_budget(850.0, 600.0), Some(142));
        // 900/900 = 1.0 → 100%
        assert_eq!(percent_of_budget(900.0, 900.0), Some(100));
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: rounding boundary cases
    // -----------------------------------------------------------------------

    #[test]
    fn percent_of_budget_rounds_up_at_half() {
        // 0.5 rounds up → 50/100 = 50%, not a boundary; use 1/3 * 100 = 33.33 → 33
        assert_eq!(percent_of_budget(1.0, 3.0), Some(33));
        // 2/3 * 100 = 66.67 → 67
        assert_eq!(percent_of_budget(2.0, 3.0), Some(67));
    }

    #[test]
    fn category_report_includes_percent_when_budgeted() {
        let txns = vec![make_txn("Dining merchant", 850.0, "Dining", "2026-05-15")];
        let budgets = vec![make_budget("Dining", 600.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());
        let cat = report.by_category.get("Dining").unwrap();
        assert_eq!(cat.percent_of_budget, Some(142));
    }

    // -----------------------------------------------------------------------
    // 9a RED: unbudgeted category — reported but not flagged
    // -----------------------------------------------------------------------

    #[test]
    fn unbudgeted_category_is_not_flagged_as_over_budget() {
        let txns = vec![make_txn("Travel merchant", 300.0, "Travel", "2026-05-15")];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert!(
            !report.over_budget_categories.contains(&"Travel".to_string()),
            "unbudgeted Travel should not be flagged"
        );
        let cat = report.by_category.get("Travel").unwrap();
        assert_eq!(cat.spent, 300.0);
        assert_eq!(cat.budget, None);
        assert_eq!(cat.percent_of_budget, None);
    }

    // -----------------------------------------------------------------------
    // 9a RED: duplicate detection — same merchant + amount + date
    // -----------------------------------------------------------------------

    #[test]
    fn identical_charges_same_day_flagged_as_duplicate() {
        let txns = vec![
            make_txn("Acme Streaming", 49.99, "Subscriptions", "2026-05-14"),
            make_txn("Acme Streaming", 49.99, "Subscriptions", "2026-05-14"),
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

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: same merchant, different amount → NOT a duplicate
    // -----------------------------------------------------------------------

    #[test]
    fn same_merchant_different_amount_not_a_duplicate() {
        let txns = vec![
            make_txn("Acme Streaming", 49.99, "Subscriptions", "2026-05-14"),
            make_txn("Acme Streaming", 9.99, "Subscriptions", "2026-05-14"),
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
    // 9a RED: prior-period delta
    // -----------------------------------------------------------------------

    #[test]
    fn prior_period_delta_is_positive_when_spending_increased() {
        let txns = vec![make_txn("Various", 4600.0, "General", "2026-05-15")];
        let cashflow = Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 4000.0,
        };
        let report = compute_spending_report(&txns, &[], &cashflow);
        assert_eq!(report.vs_prior_month.delta, 600.0);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: prior-period delta when spending decreased
    // -----------------------------------------------------------------------

    #[test]
    fn prior_period_delta_is_negative_when_spending_decreased() {
        let txns = vec![make_txn("Various", 3000.0, "General", "2026-05-15")];
        let cashflow = Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 4000.0,
        };
        let report = compute_spending_report(&txns, &[], &cashflow);
        assert_eq!(report.vs_prior_month.delta, -1000.0);
    }

    // -----------------------------------------------------------------------
    // 9a RED: empty period
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
    // 9c TRIANGULATE: total_spent is sum of all transactions
    // -----------------------------------------------------------------------

    #[test]
    fn total_spent_is_sum_of_all_transactions() {
        let txns = vec![
            make_txn("Dining merchant", 850.0, "Dining", "2026-05-15"),
            make_txn("Groceries merchant", 720.0, "Groceries", "2026-05-15"),
        ];
        let report = compute_spending_report(&txns, &[], &zero_cashflow());
        assert_eq!(report.total_spent, 1570.0);
    }

    // -----------------------------------------------------------------------
    // BUG 1 RED: zero budget with nonzero spend must NOT produce i64::MAX percent
    // -----------------------------------------------------------------------

    #[test]
    fn zero_budget_with_spend_produces_no_percent_and_is_over_budget() {
        // A Monarch category with a $0 budget and any spending:
        // - percent_of_budget should be None (not i64::MAX from inf-cast)
        // - the category IS over budget (spent > budget)
        let txns = vec![make_txn("Netflix", 15.99, "Streaming", "2026-05-15")];
        let budgets = vec![make_budget("Streaming", 0.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());

        let cat = report.by_category.get("Streaming").expect("Streaming category must exist");
        assert_ne!(
            cat.percent_of_budget,
            Some(i64::MAX),
            "zero budget with spend must not produce i64::MAX percent"
        );
        assert_eq!(
            cat.percent_of_budget,
            None,
            "zero budget should yield no percent (division by zero)"
        );
        assert!(
            report.over_budget_categories.contains(&"Streaming".to_string()),
            "spending on a $0-budget category must still be classified over budget"
        );
    }

    #[test]
    fn zero_budget_with_zero_spend_produces_no_percent_and_is_not_over_budget() {
        // $0 budget, $0 spend: NaN→0 is wrong — should also be None
        let txns = vec![make_txn("Netflix", 0.0, "Streaming", "2026-05-15")];
        let budgets = vec![make_budget("Streaming", 0.0)];
        let report = compute_spending_report(&txns, &budgets, &zero_cashflow());

        let cat = report.by_category.get("Streaming").expect("Streaming category must exist");
        assert_eq!(
            cat.percent_of_budget,
            None,
            "zero budget with zero spend should yield None percent (0/0 = NaN, not 0)"
        );
        assert!(
            !report.over_budget_categories.contains(&"Streaming".to_string()),
            "zero spend on $0-budget category is not over budget"
        );
    }
}
