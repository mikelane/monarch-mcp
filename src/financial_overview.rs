//! Pure aggregation logic for the `financial_overview` tool.
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_overview`].

use crate::client::{Account, Cashflow, NetWorthHistory};
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
    history: &NetWorthHistory,
) -> OverviewResult {
    let net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
    let cash_net = cashflow.income - cashflow.spending;
    let net_worth_change = net_worth - history.prior_month_net_worth;

    OverviewResult {
        net_worth,
        cashflow: CashflowSummary {
            income: cashflow.income,
            spending: cashflow.spending,
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
    use crate::client::{AccountType, Cashflow, NetWorthHistory};

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
        let result = compute_overview(&accounts, &zero_cashflow(), &zero_history());
        assert_eq!(result.net_worth, 70_000.0);
    }

    // 9c TRIANGULATE: liabilities exceed assets → negative net worth
    #[test]
    fn net_worth_is_negative_when_liabilities_exceed_assets() {
        let accounts = vec![account(10_000.0), account(-30_000.0)];
        let result = compute_overview(&accounts, &zero_cashflow(), &zero_history());
        assert_eq!(result.net_worth, -20_000.0);
    }

    // 9c TRIANGULATE: empty account list → zero net worth
    #[test]
    fn net_worth_is_zero_with_no_accounts() {
        let result = compute_overview(&[], &zero_cashflow(), &zero_history());
        assert_eq!(result.net_worth, 0.0);
    }

    // 9a RED: cashflow net = income − spending
    #[test]
    fn cashflow_net_is_income_minus_spending() {
        let cashflow = Cashflow {
            income: 8_000.0,
            spending: 6_500.0,
            prior_month_spending: 0.0,
        };
        let result = compute_overview(&[], &cashflow, &zero_history());
        assert_eq!(result.cashflow.income, 8_000.0);
        assert_eq!(result.cashflow.spending, 6_500.0);
        assert_eq!(result.cashflow.net, 1_500.0);
    }

    // 9c TRIANGULATE: zero cashflow → net is zero
    #[test]
    fn cashflow_net_is_zero_when_no_activity() {
        let result = compute_overview(&[], &zero_cashflow(), &zero_history());
        assert_eq!(result.cashflow.net, 0.0);
    }

    // 9a RED: month-over-month change = current − prior
    #[test]
    fn net_worth_change_is_positive_when_worth_grew() {
        let accounts = vec![account(70_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 68_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &history);
        assert_eq!(result.net_worth_change, 2_000.0);
    }

    // 9c TRIANGULATE: negative change when worth shrank
    #[test]
    fn net_worth_change_is_negative_when_worth_shrank() {
        let accounts = vec![account(60_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 68_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &history);
        assert_eq!(result.net_worth_change, -8_000.0);
    }

    // 9c TRIANGULATE: zero change when no movement
    #[test]
    fn net_worth_change_is_zero_when_unchanged() {
        let accounts = vec![account(50_000.0)];
        let history = NetWorthHistory {
            prior_month_net_worth: 50_000.0,
        };
        let result = compute_overview(&accounts, &zero_cashflow(), &history);
        assert_eq!(result.net_worth_change, 0.0);
    }
}
