//! Adversarial QA (Gate 3 penetration test) for issue #67 `asset_allocation`.
//!
//! Run #1 found a HIGH bug: `net_worth` diverged from `financial_overview` when
//! an asset class was net-negative (overdrawn checking / margin brokerage),
//! because `gross_assets`' `.max(0.0)` clamp leaked into `net_worth`. Commit
//! 2682310 fixed it by computing `net_worth` as the raw signed sum of all
//! account balances (matching `compute_overview` exactly).
//!
//! Run #2 (these tests) are the regression guard for that fix, plus a broad
//! cross-check of `asset_allocation.net_worth == compute_overview.net_worth`
//! across all-positive, overdrawn, margin, net-negative-household, and
//! hidden-account shapes, plus a byte-for-byte re-verification of the
//! `is_investable()` contract that #69 depends on. All pass; no `#[ignore]`.
//!
//! # How to run
//!
//! ```bash
//! cargo test --test adversarial_qa_issue67
//! ```

use monarch_mcp::asset_allocation::compute_asset_allocation;
use monarch_mcp::client::{Account, AccountSubtype, AccountType, Cashflow, NetWorthHistory};
use monarch_mcp::financial_overview::compute_overview;

fn acct(type_name: &str, subtype: Option<&str>, balance: f64) -> Account {
    acct_h(type_name, subtype, balance, false)
}

fn acct_h(type_name: &str, subtype: Option<&str>, balance: f64, hidden: bool) -> Account {
    Account {
        id: format!("{type_name}-{balance}-{hidden}"),
        display_name: format!("{type_name} account"),
        current_balance: balance,
        balance_was_null: false,
        account_type: AccountType {
            name: type_name.to_string(),
        },
        subtype: subtype.map(|n| AccountSubtype {
            name: n.to_string(),
            display: n.to_string(),
        }),
        is_hidden: hidden,
    }
}

/// `compute_overview`'s net_worth for the same account slice. The cashflow /
/// history inputs do not affect net_worth (it is purely the signed sum of
/// account balances), so zero values are fine for the cross-check.
fn overview_net_worth(accounts: &[Account]) -> f64 {
    let cashflow = Cashflow {
        income: 0.0,
        spending: 0.0,
        prior_month_spending: 0.0,
    };
    let history = NetWorthHistory {
        prior_month_net_worth: 0.0,
    };
    compute_overview(accounts, &cashflow, &[], &history).net_worth
}

// ---------------------------------------------------------------------------
// BUG 1 (run #1) regression guards — now FIXED, must PASS
// ---------------------------------------------------------------------------

/// Overdrawn checking: net-negative cash class must NOT be dropped from net_worth.
/// EXPECTED net_worth = 100000 + (-2000) = 98000.
#[test]
fn net_worth_includes_overdrawn_checking() {
    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        acct("depository", Some("checking"), -2_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    assert!(
        (alloc.net_worth - 98_000.0).abs() < 0.01,
        "net_worth must be 98000 (overdraft included), got {}",
        alloc.net_worth
    );
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
    // gross_assets still clamps the negative cash class out of the percentage base.
    assert!(
        (alloc.gross_assets - 100_000.0).abs() < 0.01,
        "gross_assets must remain the clamped positive-asset sum (100000), got {}",
        alloc.gross_assets
    );
}

/// Margin brokerage: net-negative equities class must NOT be dropped from net_worth.
/// EXPECTED net_worth = 50000 + (-10000) = 40000.
#[test]
fn net_worth_includes_negative_brokerage_margin() {
    let accounts = vec![
        acct("depository", Some("checking"), 50_000.0),
        acct("brokerage", Some("brokerage"), -10_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    assert!(
        (alloc.net_worth - 40_000.0).abs() < 0.01,
        "net_worth must be 40000 (margin included), got {}",
        alloc.net_worth
    );
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
    // The negative equities class clamps to 0 in gross_assets.
    assert!(
        (alloc.gross_assets - 50_000.0).abs() < 0.01,
        "gross_assets must be 50000 (negative equities clamped out), got {}",
        alloc.gross_assets
    );
}

// ---------------------------------------------------------------------------
// (2) The ADR 0014 trust cross-check, exhaustively: asset_allocation.net_worth
//     == compute_overview.net_worth across every shape that matters.
// ---------------------------------------------------------------------------

/// (a) all-positive ordinary household — no regression for normal cases.
#[test]
fn cross_check_all_positive_household() {
    let accounts = vec![
        acct("brokerage", Some("roth"), 78_000.0),
        acct("depository", Some("checking"), 12_000.0),
        acct("real_estate", Some("house"), 10_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    assert!((alloc.net_worth - 100_000.0).abs() < 0.01);
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
    // Percentages still sum to ~100% of gross_assets.
    let pct_sum: f64 = alloc
        .classes
        .values()
        .filter_map(|s| s.percent_of_assets)
        .sum();
    assert!((pct_sum - 100.0).abs() < 0.1, "pct sum {pct_sum}");
}

/// (b) overdrawn checking — explicit cross-check pairing.
#[test]
fn cross_check_overdrawn_checking() {
    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        acct("depository", Some("checking"), -2_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
}

/// (c) margin brokerage — explicit cross-check pairing.
#[test]
fn cross_check_margin_brokerage() {
    let accounts = vec![
        acct("depository", Some("checking"), 50_000.0),
        acct("brokerage", Some("brokerage"), -10_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
}

/// (d) net-negative household — liabilities exceed assets. net_worth must be
///     negative and equal compute_overview. gross_assets is the positive-asset
///     sum (unaffected by the negative net worth).
#[test]
fn cross_check_net_negative_household() {
    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        acct("depository", Some("checking"), 20_000.0),
        acct("credit", Some("credit_card"), -3_000.0),
        acct("loan", Some("other"), -200_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    // 100000 + 20000 - 3000 - 200000 = -83000
    assert!(
        (alloc.net_worth - (-83_000.0)).abs() < 0.01,
        "net_worth must be -83000, got {}",
        alloc.net_worth
    );
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
    // gross_assets = 120000 (only positive asset classes), liabilities excluded.
    assert!(
        (alloc.gross_assets - 120_000.0).abs() < 0.01,
        "gross_assets must be 120000, got {}",
        alloc.gross_assets
    );
    assert!((alloc.total_liabilities - (-203_000.0)).abs() < 0.01);
    // Percentages computed against gross_assets, not net_worth.
    let pct_sum: f64 = alloc
        .classes
        .values()
        .filter_map(|s| s.percent_of_assets)
        .sum();
    assert!((pct_sum - 100.0).abs() < 0.1, "pct sum {pct_sum}");
}

/// (e) with a hidden account — BOTH compute paths must include it identically.
#[test]
fn cross_check_hidden_account_included_by_both() {
    let accounts = vec![
        acct_h("brokerage", Some("roth"), 50_000.0, true), // hidden
        acct("depository", Some("checking"), 10_000.0),
    ];
    let alloc = compute_asset_allocation(&accounts);
    // Hidden IRA contributes; net_worth = 60000.
    assert!((alloc.net_worth - 60_000.0).abs() < 0.01);
    assert!(
        (alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01,
        "asset_allocation and financial_overview must treat the hidden account identically"
    );
    assert!((alloc.gross_assets - 60_000.0).abs() < 0.01);
    assert!((alloc.classes["equities"].total - 50_000.0).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// (2 cont.) gross_assets percentage base / divide-by-zero — unaffected by fix.
// ---------------------------------------------------------------------------

/// gross_assets == 0 (liabilities only) → no inf/NaN; all percentages None.
#[test]
fn zero_gross_assets_no_nan_or_inf() {
    let accounts = vec![acct("credit", Some("credit_card"), -5_000.0)];
    let alloc = compute_asset_allocation(&accounts);
    assert!((alloc.gross_assets - 0.0).abs() < f64::EPSILON);
    assert!(alloc.net_worth.is_finite());
    assert!((alloc.net_worth - (-5_000.0)).abs() < 0.01);
    assert!((alloc.net_worth - overview_net_worth(&accounts)).abs() < 0.01);
    for s in alloc.classes.values() {
        if let Some(p) = s.percent_of_assets {
            assert!(p.is_finite(), "no class may have a non-finite percentage");
        }
    }
    assert!(alloc.classes["liabilities"].percent_of_assets.is_none());
}

/// Empty account list — no panic, zero rollup, no NaN.
#[test]
fn empty_accounts_zero_and_finite() {
    let alloc = compute_asset_allocation(&[]);
    assert!(alloc.classes.is_empty());
    assert!((alloc.gross_assets - 0.0).abs() < f64::EPSILON);
    assert!((alloc.net_worth - 0.0).abs() < f64::EPSILON);
    assert!(alloc.net_worth.is_finite());
}

// ---------------------------------------------------------------------------
// (3) is_investable contract for #69 — UNCHANGED. Re-verify byte-for-byte.
// ---------------------------------------------------------------------------

/// Investable set is exactly {equities, real_estate}; everything else is not.
#[test]
fn investable_contract_unchanged_for_issue_69() {
    use monarch_mcp::asset_allocation::AssetClass;
    assert!(AssetClass::Equities.is_investable());
    assert!(AssetClass::RealEstate.is_investable());
    assert!(!AssetClass::Cash.is_investable());
    assert!(!AssetClass::Crypto.is_investable());
    assert!(!AssetClass::OtherAssets.is_investable());
    assert!(!AssetClass::Liabilities.is_investable());
    assert!(!AssetClass::Other.is_investable());
}

/// classify_asset_class still maps each known type correctly, and an
/// unrecognized type → Other + recognized:false + NOT investable (must not
/// corrupt #69's portfolio).
#[test]
fn classifier_and_investable_boundary_unchanged() {
    use monarch_mcp::asset_allocation::{classify_asset_class, investable_accounts};

    let cases = [
        ("brokerage", Some("st_401k"), true, true), // equities, investable
        ("brokerage", Some("roth"), true, true),
        ("brokerage", Some("brokerage"), true, true),
        ("brokerage", None, true, true), // null subtype → equities
        ("real_estate", Some("house"), true, true), // real_estate, investable
        ("depository", Some("checking"), false, true), // cash, NOT investable
        ("vehicle", Some("car"), false, true), // other_assets, NOT investable
        ("crypto", Some("bitcoin"), false, true), // crypto, NOT investable
        ("credit", Some("credit_card"), false, true), // liabilities, NOT investable
        ("loan", Some("other"), false, true), // liabilities, NOT investable
    ];
    for (ty, st, expect_investable, expect_recognized) in cases {
        let a = acct(ty, st, 1.0);
        let (class, recognized) = classify_asset_class(&a);
        assert_eq!(
            class.is_investable(),
            expect_investable,
            "{ty}/{st:?} investable expectation"
        );
        assert_eq!(recognized, expect_recognized, "{ty}/{st:?} recognized");
    }

    // Unrecognized type → Other, not recognized, NOT investable.
    let unknown = acct("collectible", Some("art"), 4_000.0);
    let (class, recognized) = classify_asset_class(&unknown);
    assert!(
        !class.is_investable(),
        "unknown type must never be investable"
    );
    assert!(!recognized);

    // Unrecognized brokerage subtype → still Equities (investable) but flagged.
    let unknown_brokerage = acct("brokerage", Some("future_etf"), 8_000.0);
    let (class, recognized) = classify_asset_class(&unknown_brokerage);
    assert!(
        class.is_investable(),
        "brokerage stays investable even if subtype unknown"
    );
    assert!(!recognized, "unknown brokerage subtype must be flagged");

    // investable_accounts returns exactly the investable set.
    let accounts = vec![
        acct("brokerage", Some("roth"), 100_000.0),
        acct("depository", Some("checking"), 5_000.0),
        acct("real_estate", Some("house"), 350_000.0),
        acct("credit", Some("credit_card"), -3_000.0),
        acct("vehicle", Some("car"), 15_000.0),
        acct("crypto", Some("bitcoin"), 10_000.0),
        acct("collectible", Some("art"), 4_000.0),
    ];
    let inv = investable_accounts(&accounts);
    assert_eq!(
        inv.len(),
        2,
        "exactly brokerage + real_estate are investable"
    );
    let types: Vec<&str> = inv.iter().map(|a| a.account_type.name.as_str()).collect();
    assert!(types.contains(&"brokerage"));
    assert!(types.contains(&"real_estate"));
}
