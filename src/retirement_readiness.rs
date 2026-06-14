//! Pure compute for the `retirement_readiness` tool.
//!
//! Combines the **invested portfolio** (Equities-class accounts from
//! `asset_allocation`) with the **annualized baseline spend** (from
//! `spending_report::compute_true_spending`) into a safe-withdrawal-rate
//! coverage check.
//!
//! ## Safe-Withdrawal-Rate model (ADR 0016)
//!
//! `sustainable_annual_withdrawal = invested_assets × withdrawal_rate`
//!
//! The default rate is 4 % (the "25× spending" rule). The rate is validated
//! to `[0.02, 0.10]` — below 2 % is economically equivalent to a savings
//! account; above 10 % is reckless and the tool rejects both extremes with a
//! clear [`MonarchError::InvalidInput`] so callers see "out of range", not NaN.
//!
//! ## Invested-assets definition (ADR 0016)
//!
//! Only **`AssetClass::Equities`** accounts count toward the investable
//! portfolio: brokerage, 401k, Roth IRA, HSA, stock plan. Real estate,
//! vehicles, cash, crypto, and liabilities are **excluded** from the SWR
//! base (see ADR 0016 for the reconciliation with ADR 0014's broader
//! `is_investable()` contract).
//!
//! The helper [`invested_financial_accounts`] encapsulates this rule. It
//! delegates to [`classify_asset_class`] from `asset_allocation.rs` — no
//! classification logic is duplicated here.
//!
//! ## Zero guards
//!
//! - `annual_baseline_spend == 0` → `coverage_ratio = None` (no ÷0 / NaN)
//! - `withdrawal_rate > 0` is enforced by the range clamp, so
//!   `target_portfolio` is always safe to compute.
//!
//! ## Sign convention
//!
//! Asset balances are **positive** (Monarch convention). Expense magnitudes
//! coming from `compute_true_spending` are also positive (the function
//! returns the magnitude of outflows, not the signed Monarch amount).

use crate::asset_allocation::{classify_asset_class, AssetClass};
use crate::client::Account;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// The retirement-readiness payload returned inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct RetirementReadiness {
    /// Total investable portfolio (Equities-class accounts only).
    pub invested_assets: f64,
    /// Annualized baseline spend derived from transaction history.
    pub annual_baseline_spend: f64,
    /// `invested_assets × withdrawal_rate`.
    pub sustainable_annual_withdrawal: f64,
    /// `sustainable_annual_withdrawal / annual_baseline_spend`.
    /// `None` when `annual_baseline_spend == 0` (avoids ÷0 / NaN).
    pub coverage_ratio: Option<f64>,
    /// `annual_baseline_spend / withdrawal_rate` (the "25× spend" target
    /// at the default 4 % rate).
    pub target_portfolio: Option<f64>,
    /// `invested_assets − target_portfolio`.
    /// Positive = surplus (ahead of target); negative = gap.
    /// `None` when `target_portfolio` is `None`.
    pub surplus_or_gap: Option<f64>,
    /// The withdrawal rate that was used (echoed so the output is
    /// self-interpreting — ADR 0016).
    pub withdrawal_rate_used: f64,
    /// Number of complete calendar months used to annualize the spend baseline.
    pub spend_window_months: u32,
    /// Human-readable note on what counts as "invested" for this calculation.
    pub invested_assets_note: String,
}

// ---------------------------------------------------------------------------
// Invested-assets helper (reuses the #67 classifier — no new logic)
// ---------------------------------------------------------------------------

/// Return accounts whose asset class is [`AssetClass::Equities`].
///
/// This is **narrower** than [`crate::asset_allocation::investable_accounts`]
/// (which also includes `RealEstate`). For SWR calculations, primary-residence
/// real estate is illiquid and excluded from the drawdown portfolio per
/// standard financial planning convention (ADR 0016).
pub fn invested_financial_accounts(accounts: &[Account]) -> Vec<&Account> {
    accounts
        .iter()
        .filter(|a| {
            let (class, _) = classify_asset_class(a);
            class == AssetClass::Equities
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Withdrawal-rate validation
// ---------------------------------------------------------------------------

/// Minimum allowed withdrawal rate (2 %).
pub const WITHDRAWAL_RATE_MIN: f64 = 0.02;
/// Maximum allowed withdrawal rate (10 %).
pub const WITHDRAWAL_RATE_MAX: f64 = 0.10;
/// Default withdrawal rate (4 % rule).
pub const WITHDRAWAL_RATE_DEFAULT: f64 = 0.04;

/// Human-readable note on what counts as "invested" for the SWR calculation.
///
/// Surfaced verbatim in every [`RetirementReadiness`] payload so the response
/// is self-interpreting (ADR 0016). Defined as a named const so tests can
/// assert the field value without repeating the string.
pub(crate) const INVESTED_ASSETS_NOTE: &str = "Invested assets = Equities-class accounts only \
    (brokerage, 401k, Roth, HSA, stock plan). Real estate, cash, crypto, \
    vehicles, and liabilities are excluded from the SWR base (ADR 0016).";

/// Validate that `rate` is in `[WITHDRAWAL_RATE_MIN, WITHDRAWAL_RATE_MAX]`.
///
/// Returns the rate unchanged on success, or an `Err(String)` with a clear
/// message on failure (suitable for wrapping in
/// [`MonarchError::InvalidInput`]).
pub fn validate_withdrawal_rate(rate: f64) -> Result<f64, String> {
    if !(WITHDRAWAL_RATE_MIN..=WITHDRAWAL_RATE_MAX).contains(&rate) {
        Err(format!(
            "withdrawal_rate {rate} is outside the supported range \
             [{WITHDRAWAL_RATE_MIN}, {WITHDRAWAL_RATE_MAX}]: \
             use a value between {:.0}% and {:.0}%",
            WITHDRAWAL_RATE_MIN * 100.0,
            WITHDRAWAL_RATE_MAX * 100.0,
        ))
    } else {
        Ok(rate)
    }
}

// ---------------------------------------------------------------------------
// Pure compute function — no I/O
// ---------------------------------------------------------------------------

/// Compute the retirement-readiness snapshot from pre-fetched, pre-validated inputs.
///
/// # Arguments
///
/// * `invested_assets` — total balance of Equities-class accounts (positive).
/// * `annual_baseline_spend` — annualised true-spending baseline (positive magnitude).
/// * `withdrawal_rate` — **already validated** fraction in `[0.02, 0.10]`.
/// * `spend_window_months` — number of complete months used to compute the baseline.
pub fn compute_retirement_readiness(
    invested_assets: f64,
    annual_baseline_spend: f64,
    withdrawal_rate: f64,
    spend_window_months: u32,
) -> RetirementReadiness {
    let sustainable_annual_withdrawal = invested_assets * withdrawal_rate;

    let coverage_ratio = if annual_baseline_spend == 0.0 {
        None
    } else {
        Some(sustainable_annual_withdrawal / annual_baseline_spend)
    };

    // withdrawal_rate is validated > 0 by validate_withdrawal_rate, so safe.
    let target_portfolio = if annual_baseline_spend == 0.0 {
        None
    } else {
        Some(annual_baseline_spend / withdrawal_rate)
    };

    let surplus_or_gap = target_portfolio.map(|t| invested_assets - t);

    RetirementReadiness {
        invested_assets,
        annual_baseline_spend,
        sustainable_annual_withdrawal,
        coverage_ratio,
        target_portfolio,
        surplus_or_gap,
        withdrawal_rate_used: withdrawal_rate,
        spend_window_months,
        invested_assets_note: INVESTED_ASSETS_NOTE.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first (module+tests committed before production code)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Account, AccountSubtype, AccountType};

    // -----------------------------------------------------------------------
    // Account fixture helpers (mirror asset_allocation.rs test helpers)
    // -----------------------------------------------------------------------

    fn make_account(type_name: &str, subtype_name: Option<&str>, balance: f64) -> Account {
        Account {
            id: format!("{type_name}-{balance}"),
            display_name: format!("{type_name} account"),
            current_balance: balance,
            balance_was_null: false,
            account_type: AccountType {
                name: type_name.to_string(),
            },
            subtype: subtype_name.map(|n| AccountSubtype {
                name: n.to_string(),
                display: n.to_string(),
            }),
            is_hidden: false,
        }
    }

    // -----------------------------------------------------------------------
    // validate_withdrawal_rate
    // -----------------------------------------------------------------------

    #[test]
    fn default_rate_is_valid() {
        assert!(validate_withdrawal_rate(WITHDRAWAL_RATE_DEFAULT).is_ok());
    }

    #[test]
    fn minimum_rate_is_valid() {
        assert!(validate_withdrawal_rate(WITHDRAWAL_RATE_MIN).is_ok());
    }

    #[test]
    fn maximum_rate_is_valid() {
        assert!(validate_withdrawal_rate(WITHDRAWAL_RATE_MAX).is_ok());
    }

    #[test]
    fn rate_below_minimum_returns_err() {
        let result = validate_withdrawal_rate(0.01);
        assert!(result.is_err(), "rate 0.01 is below min 0.02 — must error");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("outside the supported range"),
            "error message must mention range, got: {msg}"
        );
    }

    #[test]
    fn rate_above_maximum_returns_err() {
        let result = validate_withdrawal_rate(0.5);
        assert!(result.is_err(), "rate 0.5 is above max 0.10 — must error");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("outside the supported range"),
            "error message must mention range, got: {msg}"
        );
    }

    #[test]
    fn rate_zero_returns_err() {
        assert!(validate_withdrawal_rate(0.0).is_err());
    }

    #[test]
    fn rate_negative_returns_err() {
        assert!(validate_withdrawal_rate(-0.04).is_err());
    }

    // -----------------------------------------------------------------------
    // invested_financial_accounts helper
    // -----------------------------------------------------------------------

    #[test]
    fn invested_financial_accounts_returns_only_equities() {
        let accounts = vec![
            make_account("brokerage", Some("roth"), 800_000.0),
            make_account("real_estate", Some("house"), 400_000.0),
            make_account("depository", Some("checking"), 50_000.0),
            make_account("credit", Some("credit_card"), -10_000.0),
            make_account("vehicle", Some("car"), 15_000.0),
        ];
        let invested = invested_financial_accounts(&accounts);
        assert_eq!(
            invested.len(),
            1,
            "only the brokerage (equities) account is invested; \
             real_estate, cash, credit, vehicle excluded"
        );
        assert_eq!(invested[0].account_type.name, "brokerage");
    }

    #[test]
    fn primary_residence_excluded_from_invested() {
        // Issue #69 BDD scenario: brokerage 800k + property 400k → invested 800k
        let accounts = vec![
            make_account("brokerage", Some("brokerage"), 800_000.0),
            make_account("real_estate", Some("house"), 400_000.0),
        ];
        let invested = invested_financial_accounts(&accounts);
        assert_eq!(invested.len(), 1);
        let total: f64 = invested.iter().map(|a| a.current_balance).sum();
        assert!(
            (total - 800_000.0).abs() < 0.01,
            "invested total must be 800,000 (property excluded), got {total}"
        );
    }

    #[test]
    fn multiple_brokerage_accounts_all_included() {
        let accounts = vec![
            make_account("brokerage", Some("st_401k"), 300_000.0),
            make_account("brokerage", Some("roth"), 200_000.0),
            make_account("brokerage", Some("brokerage"), 300_000.0),
            make_account("depository", Some("checking"), 50_000.0),
        ];
        let invested = invested_financial_accounts(&accounts);
        assert_eq!(
            invested.len(),
            3,
            "all three brokerage accounts are invested"
        );
        let total: f64 = invested.iter().map(|a| a.current_balance).sum();
        assert!((total - 800_000.0).abs() < 0.01);
    }

    #[test]
    fn invested_financial_accounts_empty_when_no_equities() {
        let accounts = vec![
            make_account("depository", Some("checking"), 50_000.0),
            make_account("real_estate", Some("house"), 400_000.0),
        ];
        let invested = invested_financial_accounts(&accounts);
        assert!(
            invested.is_empty(),
            "no Equities accounts → empty invested list"
        );
    }

    // -----------------------------------------------------------------------
    // compute_retirement_readiness — core math
    // -----------------------------------------------------------------------

    #[test]
    fn coverage_ratio_one_at_breakeven() {
        // Invested 1,000,000 + spend 40,000 @ 4% → sustainable 40,000, coverage 1.0
        let rr = compute_retirement_readiness(1_000_000.0, 40_000.0, 0.04, 6);
        assert!(
            (rr.sustainable_annual_withdrawal - 40_000.0).abs() < 0.01,
            "sustainable withdrawal must be 40,000, got {}",
            rr.sustainable_annual_withdrawal
        );
        let ratio = rr.coverage_ratio.expect("coverage_ratio must be Some");
        assert!(
            (ratio - 1.0).abs() < 0.001,
            "coverage_ratio must be 1.0, got {ratio}"
        );
        let target = rr.target_portfolio.expect("target_portfolio must be Some");
        assert!(
            (target - 1_000_000.0).abs() < 0.01,
            "target_portfolio must be 1,000,000, got {target}"
        );
        let gap = rr.surplus_or_gap.expect("surplus_or_gap must be Some");
        assert!(
            gap.abs() < 0.01,
            "surplus_or_gap must be ~0 at breakeven, got {gap}"
        );
    }

    #[test]
    fn coverage_ratio_half_reports_gap() {
        // Invested 500,000 + spend 40,000 @ 4% → coverage 0.5, gap -500,000
        let rr = compute_retirement_readiness(500_000.0, 40_000.0, 0.04, 6);
        let ratio = rr.coverage_ratio.expect("coverage_ratio must be Some");
        assert!(
            (ratio - 0.5).abs() < 0.001,
            "coverage_ratio must be 0.5, got {ratio}"
        );
        let target = rr.target_portfolio.expect("target_portfolio must be Some");
        assert!(
            (target - 1_000_000.0).abs() < 0.01,
            "target_portfolio must be 1,000,000, got {target}"
        );
        let gap = rr.surplus_or_gap.expect("surplus_or_gap must be Some");
        assert!(
            (gap - (-500_000.0)).abs() < 0.01,
            "surplus_or_gap must be -500,000, got {gap}"
        );
    }

    #[test]
    fn custom_withdrawal_rate_is_honored_and_echoed() {
        // Invested 1,000,000 + spend 35,000 @ 3.5%
        let rr = compute_retirement_readiness(1_000_000.0, 35_000.0, 0.035, 6);
        assert!(
            (rr.withdrawal_rate_used - 0.035).abs() < 1e-9,
            "withdrawal_rate_used must echo 0.035, got {}",
            rr.withdrawal_rate_used
        );
        // target = 35,000 / 0.035 = 1,000,000
        let target = rr.target_portfolio.expect("target_portfolio must be Some");
        assert!(
            (target - 1_000_000.0).abs() < 0.01,
            "target_portfolio at 3.5% + 35k spend must be 1,000,000, got {target}"
        );
    }

    #[test]
    fn zero_spend_yields_none_coverage_ratio() {
        // Zero baseline spend → coverage_ratio must be None (not inf/NaN)
        let rr = compute_retirement_readiness(1_000_000.0, 0.0, 0.04, 6);
        assert!(
            rr.coverage_ratio.is_none(),
            "coverage_ratio must be None when annual_baseline_spend is 0"
        );
        assert!(
            rr.target_portfolio.is_none(),
            "target_portfolio must be None when annual_baseline_spend is 0"
        );
        assert!(
            rr.surplus_or_gap.is_none(),
            "surplus_or_gap must be None when annual_baseline_spend is 0"
        );
        // sustainable_annual_withdrawal is still computable
        assert!(
            (rr.sustainable_annual_withdrawal - 40_000.0).abs() < 0.01,
            "sustainable_annual_withdrawal must still be 40,000, got {}",
            rr.sustainable_annual_withdrawal
        );
    }

    #[test]
    fn zero_invested_assets_with_nonzero_spend_reports_zero_withdrawal_and_full_gap() {
        let rr = compute_retirement_readiness(0.0, 40_000.0, 0.04, 6);
        assert!(
            (rr.sustainable_annual_withdrawal - 0.0).abs() < 0.01,
            "sustainable_annual_withdrawal must be 0 with no assets"
        );
        let ratio = rr.coverage_ratio.expect("coverage_ratio must be Some");
        assert!(
            ratio.abs() < 0.001,
            "coverage_ratio must be 0.0 with no invested assets, got {ratio}"
        );
        let gap = rr.surplus_or_gap.expect("surplus_or_gap must be Some");
        assert!(
            (gap - (-1_000_000.0)).abs() < 0.01,
            "surplus_or_gap must be -1,000,000 (full gap), got {gap}"
        );
    }

    #[test]
    fn surplus_when_portfolio_exceeds_target() {
        // 2,000,000 invested + 40,000 spend @ 4% → target 1,000,000, surplus 1,000,000
        let rr = compute_retirement_readiness(2_000_000.0, 40_000.0, 0.04, 6);
        let gap = rr.surplus_or_gap.expect("surplus_or_gap must be Some");
        assert!(
            (gap - 1_000_000.0).abs() < 0.01,
            "surplus_or_gap must be +1,000,000 when 2x target, got {gap}"
        );
        let ratio = rr.coverage_ratio.expect("coverage_ratio must be Some");
        assert!(
            (ratio - 2.0).abs() < 0.001,
            "coverage_ratio must be 2.0 when 2x target, got {ratio}"
        );
    }

    #[test]
    fn spend_window_months_is_echoed_in_output() {
        let rr = compute_retirement_readiness(500_000.0, 36_000.0, 0.04, 12);
        assert_eq!(
            rr.spend_window_months, 12,
            "spend_window_months must be echoed as 12"
        );
    }

    #[test]
    fn invested_assets_note_is_non_empty() {
        let rr = compute_retirement_readiness(500_000.0, 36_000.0, 0.04, 6);
        assert!(
            !rr.invested_assets_note.is_empty(),
            "invested_assets_note must be non-empty"
        );
        assert!(
            rr.invested_assets_note.to_lowercase().contains("equities"),
            "note must mention equities"
        );
    }

    #[test]
    fn invested_assets_note_equals_const() {
        // Proves the const promotion is byte-identical: the serialised output
        // carries exactly INVESTED_ASSETS_NOTE, not a diverged copy.
        let rr = compute_retirement_readiness(500_000.0, 36_000.0, 0.04, 6);
        assert_eq!(
            rr.invested_assets_note, INVESTED_ASSETS_NOTE,
            "invested_assets_note must equal the INVESTED_ASSETS_NOTE const"
        );
    }

    // -----------------------------------------------------------------------
    // Triangulation: annualisation correctness via spend_window_months
    // -----------------------------------------------------------------------

    #[test]
    fn annualised_spend_from_6_month_window_matches_expected() {
        // If monthly_true_spend = 3_000, annualised = 3_000 * 12 = 36_000
        // invested 900_000 @ 4% = 36_000 withdrawal → coverage = 1.0
        let monthly_spend = 3_000.0_f64;
        let annual_spend = monthly_spend * 12.0;
        let rr = compute_retirement_readiness(900_000.0, annual_spend, 0.04, 6);
        let ratio = rr.coverage_ratio.expect("coverage_ratio must be Some");
        assert!(
            (ratio - 1.0).abs() < 0.001,
            "coverage_ratio must be 1.0 when portfolio exactly covers annualised spend"
        );
    }
}
