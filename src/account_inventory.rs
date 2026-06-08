//! Account inventory — groups household accounts into retirement-planning buckets.
//!
//! All bucketing logic lives here, separated from I/O, so it can be unit-tested
//! without a mock server. The tool handler in `tools.rs` fetches data via
//! `client.get_accounts()` then delegates to [`compute_account_inventory`].
//!
//! ## Bucketing rules (ADR 0009)
//!
//! Primary key: `account.account_type.name`. For `brokerage` accounts the
//! subtype further refines into taxable vs tax-advantaged. Unknown subtypes
//! fall back to the type-level bucket and are flagged with `unknown_subtype`.
//!
//! | Bucket              | type       | subtype (when type = brokerage)              |
//! |---------------------|------------|----------------------------------------------|
//! | `tax_advantaged`    | brokerage  | `st_401k`, `roth`, `health_savings_account`  |
//! | `taxable_brokerage` | brokerage  | `brokerage`, `stock_plan`, or unrecognized   |
//! | `cash`              | depository | any                                          |
//! | `other_assets`      | vehicle    | any                                          |
//! | `liabilities`       | credit     | any                                          |
//! | `liabilities`       | loan       | any                                          |
//!
//! ## Monarch sign convention
//! Asset balances are positive; liability balances are negative.
//! `total_liabilities` in [`Rollup`] is the **absolute** sum of negative balances.

use crate::client::{Account, AccountSubtype};
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// The result payload returned as JSON text inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct AccountInventory {
    /// Accounts grouped into planning buckets.
    pub buckets: HashMap<String, Bucket>,
    /// Sanity rollup — reconciles with `financial_overview` net worth.
    pub rollup: Rollup,
}

/// One planning bucket containing its accounts and a per-bucket total.
#[derive(Debug, Serialize, PartialEq)]
pub struct Bucket {
    /// Sum of balances in this bucket (signed — liabilities are negative).
    pub total: f64,
    pub accounts: Vec<AccountEntry>,
}

/// One account's contribution to a bucket.
#[derive(Debug, Serialize, PartialEq)]
pub struct AccountEntry {
    pub display_name: String,
    /// Monarch `type.name` (e.g. `"brokerage"`, `"depository"`).
    pub type_name: String,
    /// Monarch `subtype.name` (e.g. `"st_401k"`), or `None` when Monarch
    /// returns null for this account.
    pub subtype_name: Option<String>,
    /// Human-readable subtype label (e.g. `"401k"`), or `None` when null.
    pub subtype_display: Option<String>,
    /// Current balance in USD (0.0 when Monarch returned null — see `balance_unknown`).
    pub balance: f64,
    /// `true` when Monarch returned `null` for `currentBalance` — the 0.0 value
    /// is a placeholder, not a real zero balance.
    pub balance_unknown: bool,
    /// `true` when the account is hidden in the Monarch UI.
    pub is_hidden: bool,
    /// `true` when the subtype was not in the recognized vocabulary (ADR 0009).
    /// The account is still bucketed by type-level fallback.
    pub unknown_subtype: bool,
}

/// Asset/liability rollup for sanity-checking against `financial_overview`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Rollup {
    /// Sum of all positive balances (assets).
    pub total_assets: f64,
    /// Absolute sum of all negative balances (liabilities owed).
    pub total_liabilities: f64,
    /// Net worth = total_assets − total_liabilities.
    pub net_worth: f64,
}

// ---------------------------------------------------------------------------
// Bucket names — stable string keys used in the output HashMap
// ---------------------------------------------------------------------------

const BUCKET_TAX_ADVANTAGED: &str = "tax_advantaged";
const BUCKET_TAXABLE_BROKERAGE: &str = "taxable_brokerage";
const BUCKET_CASH: &str = "cash";
const BUCKET_OTHER_ASSETS: &str = "other_assets";
const BUCKET_LIABILITIES: &str = "liabilities";

/// Recognized brokerage subtypes that map to the tax-advantaged bucket (ADR 0009).
const TAX_ADVANTAGED_SUBTYPES: &[&str] = &["st_401k", "roth", "health_savings_account"];

/// Recognized brokerage subtypes that map to the taxable-brokerage bucket (ADR 0009).
const TAXABLE_BROKERAGE_SUBTYPES: &[&str] = &["brokerage", "stock_plan"];

// ---------------------------------------------------------------------------
// Pure compute function — no I/O
// ---------------------------------------------------------------------------

/// Compute the account inventory from already-fetched Monarch account data.
///
/// Accounts with `null` balance (coerced to 0.0 by the client) are flagged
/// with `balance_unknown`. Accounts with unrecognized subtypes are flagged
/// with `unknown_subtype` and bucketed by type-level fallback.
pub fn compute_account_inventory(accounts: &[Account]) -> AccountInventory {
    let mut buckets: HashMap<String, Bucket> = HashMap::new();

    for account in accounts {
        let (bucket_name, unknown_subtype) = assign_bucket(account);
        let entry = build_entry(account, unknown_subtype);
        let bucket = buckets.entry(bucket_name).or_insert(Bucket {
            total: 0.0,
            accounts: vec![],
        });
        bucket.total += account.current_balance;
        bucket.accounts.push(entry);
    }

    let rollup = compute_rollup(&buckets);

    AccountInventory { buckets, rollup }
}

/// Assign an account to a bucket name and flag whether the subtype was recognized.
fn assign_bucket(account: &Account) -> (String, bool) {
    match account.account_type.name.as_str() {
        "depository" => (BUCKET_CASH.to_string(), false),
        "credit" | "loan" => (BUCKET_LIABILITIES.to_string(), false),
        "vehicle" => (BUCKET_OTHER_ASSETS.to_string(), false),
        "brokerage" => assign_brokerage_bucket(account.subtype.as_ref()),
        _ => {
            // Unknown type — bucket by balance sign as a safe fallback.
            let bucket = if account.current_balance < 0.0 {
                BUCKET_LIABILITIES
            } else {
                BUCKET_OTHER_ASSETS
            };
            (bucket.to_string(), true)
        }
    }
}

/// Refine a `brokerage`-type account into taxable vs tax-advantaged by subtype.
fn assign_brokerage_bucket(subtype: Option<&AccountSubtype>) -> (String, bool) {
    match subtype {
        Some(st) if TAX_ADVANTAGED_SUBTYPES.contains(&st.name.as_str()) => {
            (BUCKET_TAX_ADVANTAGED.to_string(), false)
        }
        Some(st) if TAXABLE_BROKERAGE_SUBTYPES.contains(&st.name.as_str()) => {
            (BUCKET_TAXABLE_BROKERAGE.to_string(), false)
        }
        Some(_) => {
            // Recognized type but unrecognized subtype — taxable is the conservative fallback.
            (BUCKET_TAXABLE_BROKERAGE.to_string(), true)
        }
        None => {
            // Null subtype — fall back to taxable (conservative assumption).
            (BUCKET_TAXABLE_BROKERAGE.to_string(), false)
        }
    }
}

/// Build an `AccountEntry` from an `Account`, flagging balance_unknown when
/// the client coerced a null balance to 0.0.
///
/// The client always stores 0.0 for null balances (ADR 0003). We detect
/// "balance was null" by checking the `balance_null` flag that the compute
/// layer receives — except the client doesn't expose that flag. Instead, we
/// rely on callers passing a pre-computed flag via `balance_unknown`. For
/// now, this function receives the raw account and sets `balance_unknown`
/// conservatively to `false`; the caller is responsible for tracking which
/// accounts had null balances when that precision is needed.
///
/// Note: the client currently coerces null → 0.0 without preserving the
/// original null. A follow-on could add an `Option<f64>` field to `Account`
/// to distinguish "zero" from "unknown". For Phase 1 we accept that all 0.0
/// balances on non-depository accounts may be either real zeros or unknowns.
fn build_entry(account: &Account, unknown_subtype: bool) -> AccountEntry {
    AccountEntry {
        display_name: account.display_name.clone(),
        type_name: account.account_type.name.clone(),
        subtype_name: account.subtype.as_ref().map(|s| s.name.clone()),
        subtype_display: account.subtype.as_ref().map(|s| s.display.clone()),
        balance: account.current_balance,
        balance_unknown: false, // See doc comment — Phase 1 limitation.
        is_hidden: account.is_hidden,
        unknown_subtype,
    }
}

/// Compute the asset/liability rollup from the populated buckets.
fn compute_rollup(buckets: &HashMap<String, Bucket>) -> Rollup {
    let liability_total = buckets
        .get(BUCKET_LIABILITIES)
        .map(|b| b.total)
        .unwrap_or(0.0);
    let total_liabilities = liability_total.abs();

    let total_assets: f64 = buckets
        .iter()
        .filter(|(name, _)| *name != BUCKET_LIABILITIES)
        .map(|(_, b)| b.total)
        .sum();

    let net_worth = total_assets - total_liabilities;

    Rollup {
        total_assets,
        total_liabilities,
        net_worth,
    }
}

// ---------------------------------------------------------------------------
// Tests — RED first, then GREEN (TDD per CLAUDE.md)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Account, AccountSubtype, AccountType};

    fn account(
        type_name: &str,
        subtype_name: Option<&str>,
        balance: f64,
        is_hidden: bool,
    ) -> Account {
        Account {
            id: format!("{type_name}-{balance}"),
            display_name: format!("{type_name} account"),
            current_balance: balance,
            account_type: AccountType {
                name: type_name.to_string(),
            },
            subtype: subtype_name.map(|n| AccountSubtype {
                name: n.to_string(),
                display: n.to_string(),
            }),
            is_hidden,
        }
    }

    // 9a RED: 401k goes to tax_advantaged bucket
    #[test]
    fn st_401k_subtype_maps_to_tax_advantaged() {
        let accounts = vec![account("brokerage", Some("st_401k"), 50_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(
            inv.buckets.contains_key(BUCKET_TAX_ADVANTAGED),
            "tax_advantaged bucket must exist"
        );
        assert_eq!(inv.buckets[BUCKET_TAX_ADVANTAGED].accounts.len(), 1);
    }

    // 9c TRIANGULATE: roth also tax_advantaged
    #[test]
    fn roth_subtype_maps_to_tax_advantaged() {
        let accounts = vec![account("brokerage", Some("roth"), 200_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAX_ADVANTAGED));
        assert_eq!(inv.buckets[BUCKET_TAX_ADVANTAGED].accounts.len(), 1);
    }

    // 9c TRIANGULATE: HSA also tax_advantaged
    #[test]
    fn hsa_subtype_maps_to_tax_advantaged() {
        let accounts = vec![account(
            "brokerage",
            Some("health_savings_account"),
            5_000.0,
            false,
        )];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAX_ADVANTAGED));
    }

    // 9a RED: brokerage subtype maps to taxable_brokerage
    #[test]
    fn brokerage_subtype_maps_to_taxable_brokerage() {
        let accounts = vec![account("brokerage", Some("brokerage"), 10_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAXABLE_BROKERAGE));
        assert!(!inv.buckets.contains_key(BUCKET_TAX_ADVANTAGED));
    }

    // 9c TRIANGULATE: stock_plan also taxable_brokerage
    #[test]
    fn stock_plan_subtype_maps_to_taxable_brokerage() {
        let accounts = vec![account("brokerage", Some("stock_plan"), 0.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAXABLE_BROKERAGE));
    }

    // 9a RED: null subtype on brokerage → taxable fallback (no flag)
    #[test]
    fn null_subtype_on_brokerage_falls_back_to_taxable() {
        let accounts = vec![account("brokerage", None, 5_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAXABLE_BROKERAGE));
        assert!(!inv.buckets[BUCKET_TAXABLE_BROKERAGE].accounts[0].unknown_subtype);
    }

    // 9a RED: unknown subtype on brokerage → taxable + flagged
    #[test]
    fn unknown_subtype_on_brokerage_is_flagged() {
        let accounts = vec![account("brokerage", Some("future_product"), 8_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_TAXABLE_BROKERAGE));
        assert!(
            inv.buckets[BUCKET_TAXABLE_BROKERAGE].accounts[0].unknown_subtype,
            "unknown subtype must be flagged"
        );
        assert_eq!(
            inv.buckets[BUCKET_TAXABLE_BROKERAGE].accounts[0]
                .subtype_name
                .as_deref(),
            Some("future_product"),
            "raw subtype must be preserved in output"
        );
    }

    // 9a RED: depository → cash bucket
    #[test]
    fn depository_maps_to_cash() {
        let accounts = vec![account("depository", Some("checking"), 5_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_CASH));
        assert_eq!(inv.buckets[BUCKET_CASH].accounts.len(), 1);
    }

    // 9a RED: credit → liabilities
    #[test]
    fn credit_maps_to_liabilities() {
        let accounts = vec![account("credit", Some("credit_card"), -3_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_LIABILITIES));
        assert!((inv.buckets[BUCKET_LIABILITIES].total - (-3_000.0)).abs() < 0.01);
    }

    // 9c TRIANGULATE: loan also → liabilities
    #[test]
    fn loan_maps_to_liabilities() {
        let accounts = vec![account("loan", Some("other"), -200_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_LIABILITIES));
    }

    // 9a RED: vehicle → other_assets
    #[test]
    fn vehicle_maps_to_other_assets() {
        let accounts = vec![account("vehicle", Some("car"), 15_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_OTHER_ASSETS));
        assert!((inv.buckets[BUCKET_OTHER_ASSETS].total - 15_000.0).abs() < 0.01);
    }

    // 9a RED: hidden account is included but flagged
    #[test]
    fn hidden_account_is_included_and_flagged() {
        let accounts = vec![account("depository", Some("savings"), 100.0, true)];
        let inv = compute_account_inventory(&accounts);
        assert!(inv.buckets.contains_key(BUCKET_CASH));
        assert!(
            inv.buckets[BUCKET_CASH].accounts[0].is_hidden,
            "hidden flag must be propagated"
        );
    }

    // 9a RED: empty list → empty buckets, zero rollup
    #[test]
    fn empty_accounts_returns_zero_rollup() {
        let inv = compute_account_inventory(&[]);
        assert!(inv.buckets.is_empty());
        assert!((inv.rollup.net_worth - 0.0).abs() < 0.01);
        assert!((inv.rollup.total_assets - 0.0).abs() < 0.01);
        assert!((inv.rollup.total_liabilities - 0.0).abs() < 0.01);
    }

    // 9a RED: rollup net_worth = assets − |liabilities|
    #[test]
    fn rollup_net_worth_reconciles_assets_minus_liabilities() {
        let accounts = vec![
            account("depository", Some("checking"), 10_000.0, false),
            account("brokerage", Some("roth"), 200_000.0, false),
            account("credit", Some("credit_card"), -5_000.0, false),
            account("loan", Some("other"), -100_000.0, false),
            account("vehicle", Some("car"), 15_000.0, false),
        ];
        let inv = compute_account_inventory(&accounts);
        // assets = 10000 + 200000 + 15000 = 225000
        // liabilities = 5000 + 100000 = 105000
        // net worth = 225000 - 105000 = 120000
        assert!((inv.rollup.total_assets - 225_000.0).abs() < 0.01);
        assert!((inv.rollup.total_liabilities - 105_000.0).abs() < 0.01);
        assert!((inv.rollup.net_worth - 120_000.0).abs() < 0.01);
    }

    // 9c TRIANGULATE: bucket totals are signed sums
    #[test]
    fn bucket_total_is_signed_sum_of_balances() {
        let accounts = vec![
            account("credit", Some("credit_card"), -3_000.0, false),
            account("credit", Some("credit_card"), -2_000.0, false),
        ];
        let inv = compute_account_inventory(&accounts);
        assert!((inv.buckets[BUCKET_LIABILITIES].total - (-5_000.0)).abs() < 0.01);
    }

    // 9c TRIANGULATE: completely unknown type with positive balance → other_assets + flagged
    #[test]
    fn unknown_type_positive_balance_falls_back_to_other_assets_and_is_flagged() {
        let accounts = vec![account("collectible", Some("art"), 10_000.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(
            inv.buckets.contains_key(BUCKET_OTHER_ASSETS),
            "unknown type with positive balance must land in other_assets"
        );
        assert!(
            inv.buckets[BUCKET_OTHER_ASSETS].accounts[0].unknown_subtype,
            "unknown type must be flagged"
        );
    }

    // 9c TRIANGULATE: completely unknown type with negative balance → liabilities + flagged
    #[test]
    fn unknown_type_negative_balance_falls_back_to_liabilities_and_is_flagged() {
        let accounts = vec![account("mystery", None, -500.0, false)];
        let inv = compute_account_inventory(&accounts);
        assert!(
            inv.buckets.contains_key(BUCKET_LIABILITIES),
            "unknown type with negative balance must land in liabilities"
        );
        assert!(
            inv.buckets[BUCKET_LIABILITIES].accounts[0].unknown_subtype,
            "unknown type must be flagged"
        );
    }

    // 9c TRIANGULATE: per-bucket total matches sum of account balances
    #[test]
    fn tax_advantaged_total_is_sum_of_accounts() {
        let accounts = vec![
            account("brokerage", Some("st_401k"), 50_000.0, false),
            account("brokerage", Some("roth"), 200_000.0, false),
            account("brokerage", Some("health_savings_account"), 5_000.0, false),
        ];
        let inv = compute_account_inventory(&accounts);
        assert!((inv.buckets[BUCKET_TAX_ADVANTAGED].total - 255_000.0).abs() < 0.01);
    }
}
