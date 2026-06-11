//! Adversarial QA probes for issue #69 (`retirement_readiness`), gauntlet run #1.
//!
//! These are black-box probes against the PUBLIC surface of
//! `monarch_mcp::retirement_readiness`. They attack the SWR math, the
//! divide-by-zero / `None` guards, the withdrawal-rate boundary validation
//! (including the IEEE-754 special values NaN / ±inf that a JSON caller could
//! pass), and the JSON serialisation of the `Option` fields (the payload must
//! never emit `Infinity` / `NaN`, which are not valid JSON).
//!
//! Run with:
//!     cargo test --test adversarial_qa_issue69
//!
//! All probes are expected to PASS — they confirm the implementation is robust
//! against the attack surface. Any probe that FAILS is a confirmed bug and is
//! annotated `#[ignore]` with a `BUG:` comment so the green suite stays green
//! until the root agent fixes the underlying code.

use monarch_mcp::client::{Account, AccountSubtype, AccountType};
use monarch_mcp::retirement_readiness::{
    compute_retirement_readiness, invested_financial_accounts, validate_withdrawal_rate,
    WITHDRAWAL_RATE_MAX, WITHDRAWAL_RATE_MIN,
};

fn account(type_name: &str, subtype: Option<&str>, balance: f64) -> Account {
    Account {
        id: format!("{type_name}-{balance}"),
        display_name: format!("{type_name} acct"),
        current_balance: balance,
        balance_was_null: false,
        account_type: AccountType {
            name: type_name.to_string(),
        },
        subtype: subtype.map(|n| AccountSubtype {
            name: n.to_string(),
            display: n.to_string(),
        }),
        is_hidden: false,
    }
}

// ---------------------------------------------------------------------------
// VECTOR 2: withdrawal-rate validation against IEEE-754 special values.
// A JSON number can deserialize to f64::NAN / INFINITY in some paths; the
// validator must reject all of them (NaN-safe range check), never panic, never
// let them flow into the division.
// ---------------------------------------------------------------------------

#[test]
fn nan_withdrawal_rate_is_rejected() {
    // NaN compared against any range is always false; `contains` must return
    // false and the validator must Err — not Ok(NaN).
    assert!(
        validate_withdrawal_rate(f64::NAN).is_err(),
        "NaN withdrawal rate must be rejected, not accepted"
    );
}

#[test]
fn positive_infinity_withdrawal_rate_is_rejected() {
    assert!(
        validate_withdrawal_rate(f64::INFINITY).is_err(),
        "+inf withdrawal rate must be rejected"
    );
}

#[test]
fn negative_infinity_withdrawal_rate_is_rejected() {
    assert!(
        validate_withdrawal_rate(f64::NEG_INFINITY).is_err(),
        "-inf withdrawal rate must be rejected"
    );
}

#[test]
fn huge_withdrawal_rate_is_rejected() {
    assert!(validate_withdrawal_rate(1.0).is_err());
    assert!(validate_withdrawal_rate(100.0).is_err());
}

#[test]
fn out_of_range_error_message_names_both_boundaries() {
    // The error String becomes the body of MonarchError::InvalidInput surfaced
    // to the caller. It must be self-explaining: name the offending value AND
    // both ends of the supported range, so a JSON caller sees "0.5 is outside
    // [0.02, 0.10]" rather than a bare "invalid". Pin all three.
    let msg = validate_withdrawal_rate(0.5).expect_err("0.5 is out of range");
    assert!(
        msg.contains("outside the supported range"),
        "message must explain the failure mode, got: {msg}"
    );
    assert!(
        msg.contains("0.02"),
        "message must name the lower bound 0.02, got: {msg}"
    );
    assert!(
        msg.contains("0.1"),
        "message must name the upper bound 0.10, got: {msg}"
    );
    assert!(
        msg.contains("0.5"),
        "message must echo the offending value 0.5, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// VECTOR 2: boundary exactness. The range is documented as INCLUSIVE
// [0.02, 0.10]. Probe just-inside and just-outside.
// ---------------------------------------------------------------------------

#[test]
fn boundaries_are_inclusive() {
    assert!(
        validate_withdrawal_rate(WITHDRAWAL_RATE_MIN).is_ok(),
        "0.02 must be valid (inclusive lower bound)"
    );
    assert!(
        validate_withdrawal_rate(WITHDRAWAL_RATE_MAX).is_ok(),
        "0.10 must be valid (inclusive upper bound)"
    );
}

#[test]
fn just_outside_boundaries_are_rejected() {
    assert!(
        validate_withdrawal_rate(0.0199).is_err(),
        "0.0199 is below the 0.02 floor"
    );
    assert!(
        validate_withdrawal_rate(0.1001).is_err(),
        "0.1001 is above the 0.10 ceiling"
    );
}

// ---------------------------------------------------------------------------
// VECTOR 1: divide-by-zero / None guards round-tripped through SERDE JSON.
// The MCP handler serialises the struct to JSON. JSON has no Infinity/NaN
// literal; serde_json refuses to serialize a non-finite f64 and would error.
// Confirm a zero-spend payload serialises cleanly and omits/null-s the ratio
// (never "Infinity").
// ---------------------------------------------------------------------------

#[test]
fn zero_spend_payload_serialises_without_infinity_or_nan() {
    let rr = compute_retirement_readiness(1_000_000.0, 0.0, 0.04, 6);
    let json = serde_json::to_string(&rr).expect("zero-spend payload must serialise");
    assert!(
        !json.contains("Infinity") && !json.contains("inf") && !json.contains("NaN"),
        "payload must not contain Infinity/NaN, got: {json}"
    );
    // coverage_ratio / target_portfolio / surplus_or_gap are Option<f64> == None
    // → serde emits `null`.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["coverage_ratio"].is_null(), "coverage_ratio must be null");
    assert!(
        v["target_portfolio"].is_null(),
        "target_portfolio must be null"
    );
    assert!(v["surplus_or_gap"].is_null(), "surplus_or_gap must be null");
}

#[test]
fn zero_invested_assets_serialises_coverage_zero_not_null() {
    // invested 0 + spend nonzero → coverage 0.0 (Some), NOT None.
    let rr = compute_retirement_readiness(0.0, 40_000.0, 0.04, 6);
    let v = serde_json::to_value(&rr).unwrap();
    assert_eq!(
        v["coverage_ratio"].as_f64(),
        Some(0.0),
        "zero-asset coverage must serialise as 0.0, not null"
    );
    // The zero-INVESTED case is JSON-distinguishable from the zero-SPEND case:
    // here the target and gap are STILL present (the spender has a real target
    // they simply haven't funded), whereas zero-spend nulls all three. Pin the
    // exact serialised values so a regression that conflates the two guards is
    // caught at the wire format, not just in Rust Option-land.
    assert_eq!(
        v["target_portfolio"].as_f64(),
        Some(1_000_000.0),
        "zero-asset target_portfolio must be 1,000,000 (40k / 0.04), not null"
    );
    assert_eq!(
        v["surplus_or_gap"].as_f64(),
        Some(-1_000_000.0),
        "zero-asset surplus_or_gap must be -1,000,000 (full gap), not null"
    );
}

// ---------------------------------------------------------------------------
// VECTOR 1: canonical SWR cases (the contract numbers from issue #69).
// ---------------------------------------------------------------------------

#[test]
fn canonical_breakeven_case() {
    let rr = compute_retirement_readiness(1_000_000.0, 40_000.0, 0.04, 6);
    assert!((rr.sustainable_annual_withdrawal - 40_000.0).abs() < 1e-6);
    assert!((rr.coverage_ratio.unwrap() - 1.0).abs() < 1e-9);
    assert!((rr.target_portfolio.unwrap() - 1_000_000.0).abs() < 1e-6);
    assert!(rr.surplus_or_gap.unwrap().abs() < 1e-6);
    assert!(
        (rr.withdrawal_rate_used - 0.04).abs() < 1e-9,
        "withdrawal_rate_used must echo the 0.04 input, got {}",
        rr.withdrawal_rate_used
    );
}

#[test]
fn canonical_half_coverage_case() {
    let rr = compute_retirement_readiness(500_000.0, 40_000.0, 0.04, 6);
    assert!((rr.coverage_ratio.unwrap() - 0.5).abs() < 1e-9);
    assert!((rr.target_portfolio.unwrap() - 1_000_000.0).abs() < 1e-6);
    assert!((rr.surplus_or_gap.unwrap() + 500_000.0).abs() < 1e-6);
    assert!(
        (rr.withdrawal_rate_used - 0.04).abs() < 1e-9,
        "withdrawal_rate_used must echo the 0.04 input, got {}",
        rr.withdrawal_rate_used
    );
}

// ---------------------------------------------------------------------------
// VECTOR 3: invested-asset reconciliation (the #69-vs-#67 contract).
// A brokerage 800k + a primary-residence property 400k + a vehicle + cash +
// crypto + a credit liability → invested_assets must be EXACTLY 800k.
// ---------------------------------------------------------------------------

#[test]
fn reconciliation_only_equities_count() {
    let accounts = vec![
        account("brokerage", Some("brokerage"), 800_000.0),
        account("real_estate", Some("house"), 400_000.0),
        account("vehicle", Some("car"), 25_000.0),
        account("depository", Some("checking"), 50_000.0),
        account("crypto", Some("crypto"), 30_000.0),
        account("credit", Some("credit_card"), -10_000.0),
    ];
    let invested_accounts = invested_financial_accounts(&accounts);
    // Pin the COUNT, not just the sum: exactly one of the six accounts is
    // Equities-class. The sum guard alone can be satisfied by the wrong set of
    // accounts; the count guard is the honest reconciliation invariant and
    // kills mutations that admit a non-equities account into the SWR base.
    assert_eq!(
        invested_accounts.len(),
        1,
        "exactly one account (the brokerage) is Equities-class; \
         real_estate, vehicle, depository, crypto, credit are all excluded"
    );
    assert_eq!(
        invested_accounts[0].account_type.name, "brokerage",
        "the single invested account must be the brokerage"
    );
    let invested: f64 = invested_accounts.iter().map(|a| a.current_balance).sum();
    assert!(
        (invested - 800_000.0).abs() < 1e-6,
        "invested must be exactly 800,000 (only the brokerage), got {invested}"
    );
}

// ---------------------------------------------------------------------------
// VECTOR 3: negative equities balance (margin/short). A negative brokerage
// balance is still Equities-class and is SUMMED signed (not floored). Two
// brokerages 800k and -100k → 700k. This documents the actual behaviour: the
// sum is signed, so a margin-debit brokerage reduces the invested base. This
// is the mathematically-honest behaviour and matches asset_allocation's signed
// class total (the reconciliation contract).
// ---------------------------------------------------------------------------

#[test]
fn negative_brokerage_balance_reduces_invested_signed() {
    let accounts = vec![
        account("brokerage", Some("brokerage"), 800_000.0),
        account("brokerage", Some("margin"), -100_000.0),
    ];
    let invested: f64 = invested_financial_accounts(&accounts)
        .iter()
        .map(|a| a.current_balance)
        .sum();
    assert!(
        (invested - 700_000.0).abs() < 1e-6,
        "signed sum of equities must be 700,000, got {invested}"
    );
}
