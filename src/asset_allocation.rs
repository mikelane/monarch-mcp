//! Asset allocation — splits net worth by asset class.
//!
//! All classification logic lives here, separated from I/O, so it can be
//! unit-tested without a mock server. The tool handler in `tools.rs` fetches
//! data via `client.get_accounts()` then delegates to
//! [`compute_asset_allocation`].
//!
//! ## Asset-class taxonomy (ADR 0014)
//!
//! Primary key: `account.account_type.name`. For `brokerage` accounts the
//! subtype further refines into equities sub-categories. Unrecognized types
//! or subtypes fall through to `other` and are flagged with `recognized: false`.
//!
//! | Asset class    | type       | subtype (when type = brokerage)              |
//! |----------------|------------|----------------------------------------------|
//! | `equities`     | brokerage  | any (Monarch does not break out equity/bond) |
//! | `cash`         | depository | any                                          |
//! | `real_estate`  | real_estate| any                                          |
//! | `crypto`       | crypto     | any                                          |
//! | `other_assets` | vehicle    | any                                          |
//! | `liabilities`  | credit     | any                                          |
//! | `liabilities`  | loan       | any                                          |
//! | `other`        | <unknown>  | — flagged `recognized: false`                |
//!
//! ## API limitation — equity vs. bond
//!
//! Monarch does not return individual holdings data via `GetAccounts`. All
//! brokerage accounts (401k, Roth, taxable) are classified as `equities`
//! at the account-type level. An account that is partially or fully in bonds
//! cannot be distinguished from one that is 100% equity. This is documented
//! in ADR 0014; clients should note this limitation.
//!
//! ## Monarch sign convention
//! Asset balances are positive; liability balances are negative.
//! `total_liabilities` is the **signed** sum of liability-class balances
//! (i.e. typically a negative number). `gross_assets` sums only positive-
//! balance classes. `net_worth` is the signed sum of all classes.

use crate::client::{Account, AccountSubtype};
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// The result payload returned as JSON text inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetAllocation {
    /// Per-class breakdown: class name → class summary.
    pub classes: HashMap<String, AssetClassSummary>,
    /// Sum of all positive-balance class totals (assets only, not liabilities).
    pub gross_assets: f64,
    /// Signed sum of all liability-class totals (typically negative).
    pub total_liabilities: f64,
    /// net_worth = gross_assets + total_liabilities (signed sum of all accounts).
    pub net_worth: f64,
    /// API note: Monarch does not break out equity vs. bond within an account.
    /// All brokerage accounts are classified as equities.
    pub note: String,
}

/// Summary for one asset class.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetClassSummary {
    /// Signed total of all accounts in this class.
    pub total: f64,
    /// Fraction of gross_assets represented by this class.
    /// `None` when gross_assets is zero (avoids divide-by-zero).
    pub percent_of_assets: Option<f64>,
    /// `false` when one or more accounts in this class had an unrecognized
    /// type or subtype (mirrors account_inventory's honesty about unknowns).
    pub recognized: bool,
}

// ---------------------------------------------------------------------------
// Asset class names — stable string keys used in the output HashMap
// ---------------------------------------------------------------------------

const CLASS_EQUITIES: &str = "equities";
const CLASS_CASH: &str = "cash";
const CLASS_REAL_ESTATE: &str = "real_estate";
const CLASS_CRYPTO: &str = "crypto";
const CLASS_OTHER_ASSETS: &str = "other_assets";
const CLASS_LIABILITIES: &str = "liabilities";
const CLASS_OTHER: &str = "other";

// ---------------------------------------------------------------------------
// Public reusable classification function — consumed by retirement_readiness (#69)
// ---------------------------------------------------------------------------

/// Asset class assigned to an account.
///
/// This enum is the single source of truth for the asset-class taxonomy.
/// `retirement_readiness` (#69) calls [`classify_asset_class`] to compute
/// the "investable portfolio" — the classes counted as investable are
/// documented in ADR 0014.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    /// Brokerage holdings (all tax treatments). Monarch does not break out
    /// equity vs. bond within an account, so this class covers both.
    Equities,
    /// Depository accounts (checking, savings, etc.).
    Cash,
    /// Real-estate accounts (Monarch `type="real_estate"`).
    RealEstate,
    /// Cryptocurrency accounts (Monarch `type="crypto"`).
    Crypto,
    /// Vehicles and other tangible personal property.
    OtherAssets,
    /// Credit cards, loans, and mortgages.
    Liabilities,
    /// Catch-all for unrecognized account types.
    Other,
}

impl AssetClass {
    /// Returns the stable string key used in the output HashMap.
    pub fn as_str(self) -> &'static str {
        match self {
            AssetClass::Equities => CLASS_EQUITIES,
            AssetClass::Cash => CLASS_CASH,
            AssetClass::RealEstate => CLASS_REAL_ESTATE,
            AssetClass::Crypto => CLASS_CRYPTO,
            AssetClass::OtherAssets => CLASS_OTHER_ASSETS,
            AssetClass::Liabilities => CLASS_LIABILITIES,
            AssetClass::Other => CLASS_OTHER,
        }
    }

    /// Returns `true` when this class counts as "investable" for
    /// retirement-readiness calculations (ADR 0014).
    ///
    /// Investable classes: equities and real estate.
    /// Non-investable: cash (liquid buffer), crypto (speculative/separate),
    /// other_assets (illiquid), liabilities (debt), other (unknown).
    #[allow(dead_code)]
    // Used by retirement_readiness (#69); staged here as the single source of truth
    // for the investable-class contract.
    pub fn is_investable(self) -> bool {
        matches!(self, AssetClass::Equities | AssetClass::RealEstate)
    }
}

/// Classify an account into an [`AssetClass`].
///
/// Returns `(class, recognized)` where `recognized` is `false` when the
/// account's type or subtype was not in the known vocabulary (ADR 0009/0014).
///
/// This is the **single source of truth** for the asset-class taxonomy.
/// Both `compute_asset_allocation` and `retirement_readiness` (#69) call
/// this function.
pub fn classify_asset_class(account: &Account) -> (AssetClass, bool) {
    match account.account_type.name.as_str() {
        "depository" => (AssetClass::Cash, true),
        "brokerage" => classify_brokerage_asset_class(account.subtype.as_ref()),
        "credit" | "loan" => (AssetClass::Liabilities, true),
        "vehicle" => (AssetClass::OtherAssets, true),
        "real_estate" => (AssetClass::RealEstate, true),
        "crypto" => (AssetClass::Crypto, true),
        _ => {
            // Unknown type — fall through to `other` and flag as unrecognized.
            (AssetClass::Other, false)
        }
    }
}

/// Classify a `brokerage`-type account into an asset class by subtype.
///
/// Monarch does not distinguish equity-vs-bond holdings within an account,
/// so ALL brokerage subtypes map to `Equities`. The subtype is validated
/// against the known vocabulary (ADR 0009) to detect future unknowns, but
/// even unrecognized subtypes map to Equities (the conservative choice for
/// investment accounts is still "invested", not "unknown").
fn classify_brokerage_asset_class(subtype: Option<&AccountSubtype>) -> (AssetClass, bool) {
    const KNOWN_BROKERAGE_SUBTYPES: &[&str] = &[
        "brokerage",
        "stock_plan",
        "health_savings_account",
        "st_401k",
        "roth",
    ];

    match subtype {
        Some(st) if KNOWN_BROKERAGE_SUBTYPES.contains(&st.name.as_str()) => {
            (AssetClass::Equities, true)
        }
        Some(_) => {
            // Recognized type but unrecognized subtype — still equities,
            // but flagged so callers know the vocabulary may need updating.
            (AssetClass::Equities, false)
        }
        None => {
            // Null subtype — fall back to equities (reasonable for any brokerage).
            (AssetClass::Equities, true)
        }
    }
}

/// Return only the accounts whose asset class is investable (ADR 0014).
///
/// Investable = equities + real_estate. Used by `retirement_readiness` (#69)
/// to compute the investable portfolio total.
#[allow(dead_code)]
// Used by retirement_readiness (#69); staged here as the single source of truth
// for the investable-class contract.
pub fn investable_accounts(accounts: &[Account]) -> Vec<&Account> {
    accounts
        .iter()
        .filter(|a| {
            let (class, _) = classify_asset_class(a);
            class.is_investable()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pure compute function — no I/O
// ---------------------------------------------------------------------------

/// Compute the asset allocation from already-fetched Monarch account data.
///
/// Hidden accounts are included (same treatment as `account_inventory`).
/// Accounts with `null` balance (coerced to 0.0 by the client) contribute
/// their 0.0 balance; any downstream consumer should apply the same caution
/// as `account_inventory` does with `balance_unknown`.
pub fn compute_asset_allocation(accounts: &[Account]) -> AssetAllocation {
    // Accumulate per-class totals and whether all subtypes were recognized.
    let mut class_totals: HashMap<String, f64> = HashMap::new();
    let mut class_recognized: HashMap<String, bool> = HashMap::new();

    for account in accounts {
        let (class, recognized) = classify_asset_class(account);
        let key = class.as_str().to_string();
        *class_totals.entry(key.clone()).or_insert(0.0) += account.current_balance;
        // Mark class as unrecognized if any account in it is unrecognized.
        let entry = class_recognized.entry(key).or_insert(true);
        if !recognized {
            *entry = false;
        }
    }

    // gross_assets = sum of positive-balance class totals (assets, not liabilities)
    let gross_assets: f64 = class_totals
        .iter()
        .filter(|(k, _)| k.as_str() != CLASS_LIABILITIES)
        .map(|(_, v)| v.max(0.0))
        .sum();

    let total_liabilities = *class_totals.get(CLASS_LIABILITIES).unwrap_or(&0.0);

    // net_worth = raw signed sum of ALL account balances (matching financial_overview's computation).
    // This ensures the ADR 0014 trust cross-check holds even when an asset class (e.g., overdrawn
    // checking) is net-negative. The gross_assets sum is clamped for the percentage denominator
    // (which should only include positive balances), but net_worth must be the true signed sum.
    let net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();

    let classes: HashMap<String, AssetClassSummary> = class_totals
        .into_iter()
        .map(|(key, total)| {
            let recognized = *class_recognized.get(&key).unwrap_or(&true);
            let percent_of_assets = if key == CLASS_LIABILITIES {
                // Liabilities are excluded from the percentage base.
                None
            } else if gross_assets > 0.0 {
                Some((total / gross_assets) * 100.0)
            } else {
                None
            };
            (
                key,
                AssetClassSummary {
                    total,
                    percent_of_assets,
                    recognized,
                },
            )
        })
        .collect();

    AssetAllocation {
        classes,
        gross_assets,
        total_liabilities,
        net_worth,
        note: "Monarch does not expose per-holding data; all brokerage accounts \
               (401k, Roth, taxable) are classified as equities. Equity/bond \
               breakdown within an account is not available from the API."
            .to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests — RED first, then GREEN (TDD per CLAUDE.md)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Account, AccountSubtype, AccountType};

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

    fn make_hidden_account(type_name: &str, subtype_name: Option<&str>, balance: f64) -> Account {
        Account {
            is_hidden: true,
            ..make_account(type_name, subtype_name, balance)
        }
    }

    // ---------------------------------------------------------------------------
    // RED: classify_asset_class tests
    // ---------------------------------------------------------------------------

    #[test]
    fn brokerage_st_401k_classifies_as_equities() {
        let acct = make_account("brokerage", Some("st_401k"), 50_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized, "st_401k is a known brokerage subtype");
    }

    #[test]
    fn brokerage_roth_classifies_as_equities() {
        let acct = make_account("brokerage", Some("roth"), 100_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized);
    }

    #[test]
    fn brokerage_taxable_classifies_as_equities() {
        let acct = make_account("brokerage", Some("brokerage"), 25_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized);
    }

    #[test]
    fn brokerage_hsa_classifies_as_equities() {
        let acct = make_account("brokerage", Some("health_savings_account"), 5_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized);
    }

    #[test]
    fn brokerage_stock_plan_classifies_as_equities() {
        let acct = make_account("brokerage", Some("stock_plan"), 0.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized);
    }

    #[test]
    fn brokerage_unknown_subtype_classifies_as_equities_unrecognized() {
        let acct = make_account("brokerage", Some("future_etf"), 8_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(!recognized, "unknown subtype must be flagged unrecognized");
    }

    #[test]
    fn brokerage_null_subtype_classifies_as_equities() {
        let acct = make_account("brokerage", None, 15_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Equities);
        assert!(recognized, "null subtype on brokerage is not flagged");
    }

    #[test]
    fn depository_classifies_as_cash() {
        let acct = make_account("depository", Some("checking"), 5_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Cash);
        assert!(recognized);
    }

    #[test]
    fn credit_classifies_as_liabilities() {
        let acct = make_account("credit", Some("credit_card"), -3_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Liabilities);
        assert!(recognized);
    }

    #[test]
    fn loan_classifies_as_liabilities() {
        let acct = make_account("loan", Some("other"), -200_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Liabilities);
        assert!(recognized);
    }

    #[test]
    fn vehicle_classifies_as_other_assets() {
        let acct = make_account("vehicle", Some("car"), 15_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::OtherAssets);
        assert!(recognized);
    }

    #[test]
    fn real_estate_type_classifies_as_real_estate() {
        let acct = make_account("real_estate", Some("house"), 350_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::RealEstate);
        assert!(recognized);
    }

    #[test]
    fn crypto_type_classifies_as_crypto() {
        let acct = make_account("crypto", Some("bitcoin"), 10_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Crypto);
        assert!(recognized);
    }

    #[test]
    fn unknown_type_classifies_as_other_unrecognized() {
        let acct = make_account("collectible", Some("art"), 5_000.0);
        let (class, recognized) = classify_asset_class(&acct);
        assert_eq!(class, AssetClass::Other);
        assert!(!recognized, "unknown type must be flagged unrecognized");
    }

    // ---------------------------------------------------------------------------
    // RED: investable_accounts helper tests
    // ---------------------------------------------------------------------------

    #[test]
    fn investable_accounts_returns_only_equities_and_real_estate() {
        let accounts = vec![
            make_account("brokerage", Some("roth"), 100_000.0),
            make_account("depository", Some("checking"), 5_000.0),
            make_account("real_estate", Some("house"), 350_000.0),
            make_account("credit", Some("credit_card"), -3_000.0),
            make_account("vehicle", Some("car"), 15_000.0),
        ];
        let investable = investable_accounts(&accounts);
        assert_eq!(
            investable.len(),
            2,
            "only equities and real_estate are investable"
        );
        let names: Vec<&str> = investable
            .iter()
            .map(|a| a.account_type.name.as_str())
            .collect();
        assert!(names.contains(&"brokerage"));
        assert!(names.contains(&"real_estate"));
    }

    #[test]
    fn investable_accounts_empty_when_no_investable_accounts() {
        let accounts = vec![
            make_account("depository", Some("checking"), 5_000.0),
            make_account("credit", Some("credit_card"), -3_000.0),
        ];
        let investable = investable_accounts(&accounts);
        assert!(investable.is_empty());
    }

    #[test]
    fn investable_accounts_excludes_unrecognized_account() {
        // #69 reads the investable portfolio via investable_accounts(). An account
        // with an unrecognized type must classify as `other` AND be excluded from
        // the investable set — otherwise unknown holdings would silently leak into
        // the retirement-readiness portfolio. This joins the classification path to
        // the is_investable() contract end-to-end, not just per-enum-variant.
        let unknown = make_account("collectible", Some("art"), 5_000.0);
        let (class, recognized) = classify_asset_class(&unknown);
        assert_eq!(
            class,
            AssetClass::Other,
            "unknown type must classify as other"
        );
        assert!(!recognized, "unknown type must be flagged unrecognized");

        let accounts = vec![make_account("brokerage", Some("roth"), 100_000.0), unknown];
        let investable = investable_accounts(&accounts);
        assert_eq!(
            investable.len(),
            1,
            "only the brokerage account is investable; the unrecognized account must not leak in"
        );
        assert_eq!(
            investable[0].account_type.name, "brokerage",
            "the single investable account must be the brokerage, not the unrecognized one"
        );
    }

    // ---------------------------------------------------------------------------
    // RED: compute_asset_allocation tests — basic classification
    // ---------------------------------------------------------------------------

    #[test]
    fn brokerage_maps_to_equities_class() {
        let accounts = vec![make_account("brokerage", Some("roth"), 78_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!(
            alloc.classes.contains_key(CLASS_EQUITIES),
            "equities class must exist"
        );
        assert!((alloc.classes[CLASS_EQUITIES].total - 78_000.0).abs() < 0.01);
    }

    #[test]
    fn depository_maps_to_cash_class() {
        let accounts = vec![make_account("depository", Some("checking"), 12_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!(alloc.classes.contains_key(CLASS_CASH));
        assert!((alloc.classes[CLASS_CASH].total - 12_000.0).abs() < 0.01);
    }

    #[test]
    fn real_estate_maps_to_real_estate_class() {
        let accounts = vec![make_account("real_estate", Some("house"), 350_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!(alloc.classes.contains_key(CLASS_REAL_ESTATE));
        assert!((alloc.classes[CLASS_REAL_ESTATE].total - 350_000.0).abs() < 0.01);
    }

    // ---------------------------------------------------------------------------
    // RED: percentages computed against gross_assets
    // ---------------------------------------------------------------------------

    #[test]
    fn percentages_computed_against_gross_assets_not_net_worth() {
        // brokerage=78000, checking=12000, property=10000 → gross=100000
        let accounts = vec![
            make_account("brokerage", Some("brokerage"), 78_000.0),
            make_account("depository", Some("checking"), 12_000.0),
            make_account("real_estate", Some("house"), 10_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        assert!((alloc.gross_assets - 100_000.0).abs() < 0.01);
        let eq_pct = alloc.classes[CLASS_EQUITIES].percent_of_assets.unwrap();
        let cash_pct = alloc.classes[CLASS_CASH].percent_of_assets.unwrap();
        let re_pct = alloc.classes[CLASS_REAL_ESTATE].percent_of_assets.unwrap();
        assert!(
            (eq_pct - 78.0).abs() < 0.01,
            "equities pct should be 78%, got {eq_pct}"
        );
        assert!(
            (cash_pct - 12.0).abs() < 0.01,
            "cash pct should be 12%, got {cash_pct}"
        );
        assert!(
            (re_pct - 10.0).abs() < 0.01,
            "real_estate pct should be 10%, got {re_pct}"
        );
    }

    #[test]
    fn asset_class_percentages_sum_to_one_hundred() {
        // The sum-to-100% invariant guarded only by the gated live test must also
        // hold hermetically. Uneven splits (not clean round numbers) catch any
        // off-by-denominator mutation: 33000+17000+50000 = 100000 gross.
        let accounts = vec![
            make_account("brokerage", Some("roth"), 33_000.0),
            make_account("depository", Some("checking"), 17_000.0),
            make_account("real_estate", Some("house"), 50_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        let pct_sum: f64 = alloc
            .classes
            .values()
            .filter_map(|s| s.percent_of_assets)
            .sum();
        assert!(
            (pct_sum - 100.0).abs() < 0.01,
            "per-class percent_of_assets must sum to ~100% of gross_assets, got {pct_sum}"
        );
    }

    #[test]
    fn liability_percentages_excluded_so_sum_stays_one_hundred() {
        // With a liability present, the liabilities class contributes None to the
        // percent sum, so the asset classes must STILL sum to exactly 100% — the
        // liability magnitude must not dilute or inflate the denominator.
        let accounts = vec![
            make_account("brokerage", Some("roth"), 60_000.0),
            make_account("depository", Some("checking"), 40_000.0),
            make_account("credit", Some("credit_card"), -25_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        assert!(
            (alloc.gross_assets - 100_000.0).abs() < 0.01,
            "gross_assets must exclude the liability, got {}",
            alloc.gross_assets
        );
        assert!(
            alloc.classes[CLASS_LIABILITIES].percent_of_assets.is_none(),
            "liabilities must contribute None to the percent sum"
        );
        let pct_sum: f64 = alloc
            .classes
            .values()
            .filter_map(|s| s.percent_of_assets)
            .sum();
        assert!(
            (pct_sum - 100.0).abs() < 0.01,
            "asset-class percentages must sum to 100% even with a liability present, got {pct_sum}"
        );
    }

    // ---------------------------------------------------------------------------
    // RED: liabilities are excluded from percentage base
    // ---------------------------------------------------------------------------

    #[test]
    fn liabilities_excluded_from_percentage_base() {
        let accounts = vec![
            make_account("depository", Some("checking"), 100_000.0),
            make_account("credit", Some("credit_card"), -5_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        // gross_assets = 100000 (only positive classes)
        assert!((alloc.gross_assets - 100_000.0).abs() < 0.01);
        assert!((alloc.total_liabilities - (-5_000.0)).abs() < 0.01);
        assert!((alloc.net_worth - 95_000.0).abs() < 0.01);
        // Liabilities get None percent_of_assets
        assert!(
            alloc.classes[CLASS_LIABILITIES].percent_of_assets.is_none(),
            "liabilities must not have a percent_of_assets"
        );
        // Cash class has 100% of assets
        let cash_pct = alloc.classes[CLASS_CASH].percent_of_assets.unwrap();
        assert!((cash_pct - 100.0).abs() < 0.01);
    }

    // ---------------------------------------------------------------------------
    // RED: net_worth is the signed sum of all classes
    // ---------------------------------------------------------------------------

    #[test]
    fn net_worth_is_signed_sum_of_all_account_balances() {
        let accounts = vec![
            make_account("brokerage", Some("roth"), 78_000.0),
            make_account("depository", Some("checking"), 12_000.0),
            make_account("real_estate", Some("house"), 10_000.0),
            make_account("credit", Some("credit_card"), -5_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        let expected_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
        assert!(
            (alloc.net_worth - expected_net_worth).abs() < 0.01,
            "net_worth ({}) must equal signed sum of all balances ({})",
            alloc.net_worth,
            expected_net_worth
        );
    }

    // ---------------------------------------------------------------------------
    // RED: unrecognized subtype → other class, recognized=false
    // ---------------------------------------------------------------------------

    #[test]
    fn unrecognized_type_maps_to_other_class_flagged() {
        let accounts = vec![make_account("collectible", Some("art"), 4_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!(
            alloc.classes.contains_key(CLASS_OTHER),
            "other class must exist for unrecognized type"
        );
        assert!(
            !alloc.classes[CLASS_OTHER].recognized,
            "other class must be flagged unrecognized"
        );
        assert!((alloc.classes[CLASS_OTHER].total - 4_000.0).abs() < 0.01);
    }

    #[test]
    fn unrecognized_brokerage_subtype_stays_equities_but_flagged() {
        let accounts = vec![make_account("brokerage", Some("future_etf"), 8_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        // Still classified as equities, but recognized=false
        assert!(alloc.classes.contains_key(CLASS_EQUITIES));
        assert!(
            !alloc.classes[CLASS_EQUITIES].recognized,
            "unknown brokerage subtype must flag equities class as unrecognized"
        );
    }

    // ---------------------------------------------------------------------------
    // RED: hidden account is included but balance contributes normally
    // ---------------------------------------------------------------------------

    #[test]
    fn hidden_account_is_included_in_totals() {
        let accounts = vec![
            make_account("depository", Some("checking"), 5_000.0),
            make_hidden_account("brokerage", Some("roth"), 100_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        // Both balances contribute — hidden does not mean excluded
        assert!((alloc.gross_assets - 105_000.0).abs() < 0.01);
        assert!((alloc.classes[CLASS_EQUITIES].total - 100_000.0).abs() < 0.01);
    }

    // ---------------------------------------------------------------------------
    // RED: empty accounts → no divide-by-zero, zero rollup
    // ---------------------------------------------------------------------------

    #[test]
    fn empty_accounts_returns_zero_rollup_no_panic() {
        let alloc = compute_asset_allocation(&[]);
        assert!(alloc.classes.is_empty());
        assert!((alloc.gross_assets - 0.0).abs() < f64::EPSILON);
        assert!((alloc.total_liabilities - 0.0).abs() < f64::EPSILON);
        assert!((alloc.net_worth - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_gross_assets_yields_none_percentages() {
        // Only liabilities — gross_assets = 0
        let accounts = vec![make_account("credit", Some("credit_card"), -5_000.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!((alloc.gross_assets - 0.0).abs() < f64::EPSILON);
        // No asset classes exist, so no percentages to check. Liabilities get None.
        assert!(alloc.classes[CLASS_LIABILITIES].percent_of_assets.is_none());
    }

    #[test]
    fn asset_class_with_zero_gross_assets_yields_none_percent_not_nan() {
        // An ASSET class is present (cash) but gross_assets is exactly 0 (the single
        // depository account has a 0.0 balance). The percentage must be None, never
        // 0.0/0.0 = NaN. This pins the divide-by-zero guard on the asset-class branch
        // specifically — the liabilities-only case above never exercises an asset
        // class at gross_assets == 0.
        let accounts = vec![make_account("depository", Some("checking"), 0.0)];
        let alloc = compute_asset_allocation(&accounts);
        assert!(
            (alloc.gross_assets - 0.0).abs() < f64::EPSILON,
            "gross_assets must be 0 for a single zero-balance account, got {}",
            alloc.gross_assets
        );
        let cash = &alloc.classes[CLASS_CASH];
        assert!(
            (cash.total - 0.0).abs() < f64::EPSILON,
            "cash total must be 0.0, got {}",
            cash.total
        );
        assert_eq!(
            cash.percent_of_assets, None,
            "percent_of_assets must be None (not NaN) when gross_assets is 0; got {:?}",
            cash.percent_of_assets
        );
    }

    // ---------------------------------------------------------------------------
    // RED: gross_assets excludes liabilities even when balance is positive
    //      (e.g. overpaid credit card counts per-account toward assets)
    // ---------------------------------------------------------------------------

    #[test]
    fn multiple_brokerage_accounts_accumulate_in_equities_class() {
        let accounts = vec![
            make_account("brokerage", Some("st_401k"), 50_000.0),
            make_account("brokerage", Some("roth"), 200_000.0),
            make_account("brokerage", Some("health_savings_account"), 5_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);
        assert!((alloc.classes[CLASS_EQUITIES].total - 255_000.0).abs() < 0.01);
        assert!((alloc.gross_assets - 255_000.0).abs() < 0.01);
        let pct = alloc.classes[CLASS_EQUITIES].percent_of_assets.unwrap();
        assert!((pct - 100.0).abs() < 0.01, "single class = 100% of assets");
    }

    // ---------------------------------------------------------------------------
    // RED: AssetClass::is_investable contract
    // ---------------------------------------------------------------------------

    #[test]
    fn equities_and_real_estate_are_investable() {
        assert!(AssetClass::Equities.is_investable());
        assert!(AssetClass::RealEstate.is_investable());
    }

    #[test]
    fn cash_crypto_other_liabilities_are_not_investable() {
        assert!(!AssetClass::Cash.is_investable());
        assert!(!AssetClass::Crypto.is_investable());
        assert!(!AssetClass::OtherAssets.is_investable());
        assert!(!AssetClass::Liabilities.is_investable());
        assert!(!AssetClass::Other.is_investable());
    }

    // ---------------------------------------------------------------------------
    // RED: net_worth regression — overdrawn asset class
    // ---------------------------------------------------------------------------

    #[test]
    fn net_worth_includes_negative_asset_class_balances() {
        // Overdrawn checking (negative cash class balance) must not vanish from net_worth.
        let accounts = vec![
            make_account("brokerage", Some("roth"), 100_000.0),
            make_account("depository", Some("checking"), -2_000.0),
        ];
        let alloc = compute_asset_allocation(&accounts);

        // Verify that gross_assets is still clamped to positive balances only
        // (used as the percentage denominator).
        assert!(
            (alloc.gross_assets - 100_000.0).abs() < 0.01,
            "gross_assets should exclude negative asset class (100000), got {}",
            alloc.gross_assets
        );

        // But net_worth must be the signed sum of all balances, including negatives.
        let expected_net_worth: f64 = accounts.iter().map(|a| a.current_balance).sum();
        assert!(
            (alloc.net_worth - expected_net_worth).abs() < 0.01,
            "net_worth must be signed sum of all balances ({}), got {}",
            expected_net_worth,
            alloc.net_worth
        );
    }
}
