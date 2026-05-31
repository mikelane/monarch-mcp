//! Pure aggregation logic for the `financial_overview` tool.
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_overview`].

use crate::client::{Account, Cashflow, NetWorthHistory, Transaction};
use crate::spending_report::compute_true_spending;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Output type — must match what the BDD Then-steps assert on
// ---------------------------------------------------------------------------

/// The result payload returned as JSON text inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct OverviewResult {
    /// Net worth = sum(asset balances) − sum(liability balances), in USD.
    pub net_worth: f64,
    /// This month's cash-flow breakdown.
    pub cashflow: CashflowSummary,
    /// Net-worth movement vs. the prior month (positive = growth).
    pub net_worth_change: f64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CashflowSummary {
    pub income: f64,
    pub spending: f64,
    /// net = income − spending
    pub net: f64,
}

// ---------------------------------------------------------------------------
// Aggregation — pure function, no I/O
// ---------------------------------------------------------------------------

/// Compute the overview from already-fetched Monarch data.
///
/// Account classification: the mock server uses `type.name` values such as
/// `"checking"`, `"savings"`, `"investment"` for assets and `"credit"`,
/// `"loan"`, `"mortgage"` for liabilities. Rather than maintaining an
/// exhaustive list, we use the sign of `currentBalance`: negative balances
/// represent money owed (liabilities); positive balances represent money
/// owned (assets). This matches the mock fixture where credit accounts carry
/// a negative balance equal to the owed amount.
pub fn compute_overview(
    accounts: &[Account],
    cashflow: &Cashflow,
    transactions: &[Transaction],
    history: &NetWorthHistory,
) -> OverviewResult {
    let net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    // Use our own classification logic rather than Monarch's opaque sumExpense aggregate.
    // This guarantees financial_overview.spending and spending_report.total_spent agree.
    // See ADR 0005 for rationale.
    let true_spending = compute_true_spending(transactions);
    let cash_net = cashflow.income - true_spending;
    let net_worth_change = net_worth - history.prior_month_net_worth;

    OverviewResult {
        net_worth,
        cashflow: CashflowSummary {
            income: cashflow.income,
            spending: true_spending,
            net: cash_net,
        },
        net_worth_change,
    }
}

// ---------------------------------------------------------------------------
// Tests — RED first, then GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{AccountType, Cashflow, Category, NetWorthHistory, Transaction};

    fn account(balance: f64) -> Account {
        Account {
            id: "id".to_string(),
            display_name: "Test".to_string(),
            current_balance: balance,
            account_type: AccountType {
                name: "checking".to_string(),
            },
        }
    }

    fn zero_cashflow() -> Cashflow {
        Cashflow {
            income: 0.0,
            spending: 0.0,
            prior_month_spending: 0.0,
        }
    }

    fn zero_history() -> NetWorthHistory {
        NetWorthHistory {
            prior_month_net_worth: 0.0,
        }
    }

    // 9a RED: assets minus liabilities gives correct net worth
    #[test]
    fn net_worth_is_assets_minus_liabilities() {
        let accounts = vec![
            account(100_000.0), // asset
            account(-30_000.0), // liability (credit card stores as negative)
        ];
        let result = compute_overview(&accounts, &zero_cashflow(), &[], &zero_history());
        assert_eq!(result.net_worth, 70_000.0);
    }

    // 9c TRIANGULATE: liabilities exceed assets → negative net worth
    #[test]
    fn net_worth_is_negative_when_liabilities_exceed_assets() {
        let accounts = vec![account(10_000.0), account(-30_000.0)];
        let result = compute_overview(&accounts, &zero_cashflow(), &[], &zero_history());
        assert_eq!(result.net_worth, -20_000.0);
    }

    // 9c TRIANGULATE: empty account list → zero net worth
    #[test]
    fn net_worth_is_zero_with_no_accounts() {
        let result = compute_overview(&[], &zero_cashflow(), &[], &zero_history());
        assert_eq!(result.net_worth, 0.0);
    }

    // 9a RED: cashflow net = income − spending (from transactions now)
    #[test]
    fn cashflow_net_is_income_minus_spending() {
        // Pass expense transactions so true_spending drives the output,
        // not the Monarch aggregate stored in cashflow.spending.
        let txns = vec![
            make_expense_txn(-3_500.0, "Groceries"),
            make_expense_txn(-3_000.0, "Dining"),
        ];
        let cashflow = Cashflow {
            income: 8_000.0,
            spending: 9_999.0, // Monarch aggregate — should be ignored
            prior_month_spending: 0.0,
        };
        let result = compute_overview(&[], &cashflow, &txns, &zero_history());
        assert_eq!(result.cashflow.income, 8_000.0);
        assert_eq!(result.cashflow.spending, 6_500.0);
        assert_eq!(result.cashflow.net, 1_500.0);
    }

    // 9c TRIANGULATE: zero cashflow → net is zero
    #[test]
    fn cashflow_net_is_zero_when_no_activity() {
        let result = compute_overview(&[], &zero_cashflow(), &[], &zero_history());
        assert_eq!(result.cashflow.net, 0.0);
    }

    // 9a RED: month-over-month change = current − prior
    #[test]
    fn net_worth_change_is_positive_when_worth_grew() {
        let accounts = vec![account(70_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 68_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &[], &history);
        assert_eq!(result.net_worth_change, 2_000.0);
    }

    // 9c TRIANGULATE: negative change when worth shrank
    #[test]
    fn net_worth_change_is_negative_when_worth_shrank() {
        let accounts = vec![account(60_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 68_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &[], &history);
        assert_eq!(result.net_worth_change, -8_000.0);
    }

    // 9c TRIANGULATE: zero change when no movement
    #[test]
    fn net_worth_change_is_zero_when_unchanged() {
        let accounts = vec![account(50_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 50_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &[], &history);
        assert_eq!(result.net_worth_change, 0.0);
    }

    // -----------------------------------------------------------------------
    // Commit 2 RED tests: financial_overview computes spending from transactions.
    // -----------------------------------------------------------------------

    fn make_expense_txn(amount: f64, category: &str) -> Transaction {
        Transaction {
            id: format!("{category}-{amount}"),
            amount,
            date: "2026-05-15".to_string(),
            merchant_name: "Merchant".to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("expense".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    fn make_transfer_txn(amount: f64, category: &str) -> Transaction {
        Transaction {
            id: format!("{category}-{amount}"),
            amount,
            date: "2026-05-15".to_string(),
            merchant_name: "Transfer".to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("transfer".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    /// financial_overview.spending must equal spending_report.total_spent for
    /// the same transaction set — they share the same helper by construction.
    #[test]
    fn overview_spending_agrees_with_spending_report_for_same_transactions() {
        use crate::spending_report::compute_spending_report;
        let txns = vec![
            make_expense_txn(-365.0, "Groceries"),
            make_expense_txn(-200.0, "Dining"),
        ];
        let cashflow = Cashflow {
            income: 5_000.0,
            spending: 9_999.0, // Monarch aggregate — should be ignored
            prior_month_spending: 0.0,
        };
        let overview = compute_overview(&[], &cashflow, &txns, &zero_history());
        let report = compute_spending_report(&txns, &[], &cashflow);
        assert_eq!(
            overview.cashflow.spending, report.total_spent,
            "financial_overview.spending and spending_report.total_spent must agree"
        );
        assert_eq!(overview.cashflow.spending, 565.0);
    }

    /// A transfer transaction must not inflate financial_overview.spending.
    #[test]
    fn overview_spending_excludes_transfer_transactions() {
        let txns = vec![
            make_transfer_txn(-3_299.02, "Credit Card Payment"),
            make_expense_txn(-731.27, "Groceries"),
        ];
        let cashflow = Cashflow {
            income: 5_000.0,
            spending: 4_030.29, // Monarch aggregate includes the transfer — should be ignored
            prior_month_spending: 0.0,
        };
        let result = compute_overview(&[], &cashflow, &txns, &zero_history());
        assert!(
            (result.cashflow.spending - 731.27).abs() < 0.001,
            "transfer must not inflate spending; expected 731.27, got {}",
            result.cashflow.spending
        );
    }

    /// net is always income − true_spending regardless of the Monarch aggregate.
    #[test]
    fn overview_net_uses_true_spending_not_monarch_aggregate() {
        let txns = vec![make_expense_txn(-500.0, "Dining")];
        let cashflow = Cashflow {
            income: 3_000.0,
            spending: 9_000.0, // inflated Monarch aggregate
            prior_month_spending: 0.0,
        };
        let result = compute_overview(&[], &cashflow, &txns, &zero_history());
        // net = income(3000) - true_spending(500) = 2500, NOT 3000-9000=-6000
        assert_eq!(result.cashflow.net, 2_500.0);
        assert_eq!(result.cashflow.spending, 500.0);
    }
}
