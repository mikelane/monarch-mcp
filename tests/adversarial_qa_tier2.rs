//! Adversarial QA (Gate 3) — penetration tests for the three Tier-2 tools.
//!
//! These tests drive the PUBLIC client ops through a wiremock server, feeding
//! the REAL Monarch response shapes documented in ADR 0003 (including the
//! nullable fields the ADR explicitly calls out). They exist to prove
//! reachable bugs the happy-path BDD + unit tests miss.
//!
//! Authentication is established purely through public API: write a session.json
//! to a temp path, construct the client with `with_session_path`, then call
//! `resolve_token_from_env_or_disk()`.

use std::path::PathBuf;

use monarch_mcp::client::MonarchClient;
use monarch_mcp::net_worth_trend::compute_trend;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build an authenticated client pointed at `base`, using only public API.
fn authed_client(base: &str) -> (MonarchClient, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let session_path: PathBuf = tmp.path().join("monarch-mcp").join("session.json");
    std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
    std::fs::write(&session_path, r#"{"token":"test-token"}"#).unwrap();

    let mut client = MonarchClient::with_session_path(Some(base.to_string()), session_path);
    client.resolve_token_from_env_or_disk();
    assert!(
        client.token().is_some(),
        "client should be authenticated for test"
    );
    (client, tmp)
}

// ===========================================================================
// BUG 1 — null `amountDiff` in a real recurring response panics/errors the
// whole scan. ADR 0003 line 130 documents `amountDiff: "<float|null>"`, and
// the field semantics note (line 156) say a NEW recurring stream has no prior
// occurrence to diff against — Monarch sends null. `RecurringTransactionItemRaw`
// uses a bare `f64`, so serde fails to deserialize the array.
// ===========================================================================

#[tokio::test]
async fn recurring_scan_null_amount_diff_does_not_break_the_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "recurringTransactionItems": [
                    {
                        "stream": {
                            "id": "stream-new",
                            "frequency": "monthly",
                            "amount": -19.99,
                            "isApproximate": false,
                            "merchant": {"id": "m-1", "name": "NewSubscription", "logoUrl": null, "__typename": "RecurringTransactionStream"},
                            "__typename": "RecurringTransactionStream"
                        },
                        "date": "2026-05-20",
                        "isPast": false,
                        "transactionId": null,
                        "amount": -19.99,
                        // Real Monarch: a brand-new recurring stream has no prior
                        // occurrence to diff against, so amountDiff is null.
                        "amountDiff": null,
                        "category": {"id": "c-1", "name": "Entertainment", "__typename": "Category"},
                        "account": {"id": "a-1", "displayName": "Checking", "logoUrl": null, "__typename": "Account"},
                        "__typename": "RecurringTransactionItem"
                    }
                ]
            }
        })))
        .mount(&server)
        .await;

    let (client, _tmp) = authed_client(&server.uri());
    let result = client
        .get_recurring_for_scan("2026-05-01", "2026-05-31")
        .await;
    assert!(
        result.is_ok(),
        "null amountDiff must not break the scan — real Monarch sends null for new streams; got: {:?}",
        result.err()
    );
    let items = result.unwrap();
    assert_eq!(
        items.len(),
        1,
        "the single recurring item should still be returned"
    );
    // A null diff means "unknown / no change measurable" — must NOT be treated
    // as a creeping charge.
    assert!(
        items[0].amount_diff.abs() < f64::EPSILON,
        "null amountDiff should map to 0.0 (no measurable drift), got {}",
        items[0].amount_diff
    );
}

// ===========================================================================
// BUG 2 — null `merchant` (or null merchant.name) in a recurring item panics
// the whole call. ADR 0003 marks merchant sub-fields nullable and real Monarch
// streams without a resolved merchant send `merchant: null`. `RecurringStreamRaw`
// requires a non-Optional `RecurringMerchantRaw` with a required `name`.
// ===========================================================================

#[tokio::test]
async fn recurring_scan_null_merchant_does_not_break_the_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "recurringTransactionItems": [
                    {
                        "stream": {
                            "id": "stream-x",
                            "frequency": "monthly",
                            "amount": -9.99,
                            "isApproximate": false,
                            // Unresolved merchant — Monarch sends null here.
                            "merchant": null,
                            "__typename": "RecurringTransactionStream"
                        },
                        "date": "2026-05-20",
                        "isPast": false,
                        "transactionId": null,
                        "amount": -13.99,
                        "amountDiff": 4.0,
                        "category": null,
                        "account": null,
                        "__typename": "RecurringTransactionItem"
                    }
                ]
            }
        })))
        .mount(&server)
        .await;

    let (client, _tmp) = authed_client(&server.uri());
    let result = client
        .get_recurring_for_scan("2026-05-01", "2026-05-31")
        .await;
    assert!(
        result.is_ok(),
        "null merchant must not break the scan; got: {:?}",
        result.err()
    );
}

// ===========================================================================
// BUG 3 — same null-amountDiff hazard on the cashflow_forecast path
// (`get_recurring` shares `RecurringTransactionItemRaw`). A single new stream
// with null amountDiff makes the whole forecast fail, so the household gets an
// internal error instead of a month-end projection.
// ===========================================================================

#[tokio::test]
async fn cashflow_forecast_null_amount_diff_does_not_break_recurring_fetch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "recurringTransactionItems": [
                    {
                        "stream": {
                            "id": "stream-rent",
                            "frequency": "monthly",
                            "amount": -1500.0,
                            "isApproximate": false,
                            "merchant": {"id": "m-1", "name": "Landlord", "logoUrl": null, "__typename": "RecurringTransactionStream"},
                            "__typename": "RecurringTransactionStream"
                        },
                        "date": "2026-05-15",
                        "isPast": false,
                        "transactionId": null,
                        "amount": -1500.0,
                        "amountDiff": null,
                        "category": {"id": "c-1", "name": "Rent", "__typename": "Category"},
                        "account": {"id": "a-1", "displayName": "Checking", "logoUrl": null, "__typename": "Account"},
                        "__typename": "RecurringTransactionItem"
                    }
                ]
            }
        })))
        .mount(&server)
        .await;

    let (client, _tmp) = authed_client(&server.uri());
    let result = client.get_recurring("2026-05-01", "2026-05-31").await;
    assert!(
        result.is_ok(),
        "null amountDiff must not break the forecast's recurring fetch; got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().len(), 1);
}

// ===========================================================================
// BUG 4 — net_worth_trend biggest_mover sign/selection: when an asset GROWS and
// a liability of equal magnitude also grows (debt increases), the "biggest mover"
// is selected by |change| with ties broken by HashMap iteration order, which is
// nondeterministic. Construct two types whose |change| is exactly equal and assert
// the mover is stable/deterministic across runs. (Reachable: equal-magnitude
// moves are plausible.) This documents nondeterminism, not just a value error.
// ===========================================================================

#[test]
fn biggest_mover_is_deterministic_on_tie() {
    // depository +5000, brokerage -5000 (both |5000|). Tie.
    let mk = |month: &str, ty: &str, bal: f64| monarch_mcp::net_worth_trend::AccountTypeSnapshot {
        account_type: ty.to_string(),
        month: month.to_string(),
        balance: bal,
    };
    let snaps = vec![
        mk("2026-04", "depository", 10_000.0),
        mk("2026-04", "brokerage", 40_000.0),
        mk("2026-05", "depository", 15_000.0), // +5000
        mk("2026-05", "brokerage", 35_000.0),  // -5000
    ];

    let first = compute_trend(&snaps).biggest_mover.unwrap().account_type;
    // Recompute many times; HashMap iteration order can vary between calls.
    for _ in 0..50 {
        let again = compute_trend(&snaps).biggest_mover.unwrap().account_type;
        assert_eq!(
            first, again,
            "biggest_mover on a |change| tie must be deterministic, got {first} then {again}"
        );
    }
}

// ===========================================================================
// BUG 5 — net_worth_trend: a month missing one account type. The per-type delta
// uses balance_for(earliest, t) which SUMS matching rows; when a type is absent
// in the earliest month it silently sums to 0.0, so its "change" is reported as
// (latest - 0) = full latest balance — a fabricated swing that did not happen.
// Reachable: a new brokerage account opened mid-period appears only in later months.
// ===========================================================================

// D-NWT fix: use each account type's first-seen month within the window as its
// baseline. A type absent in the overall earliest month still only contributes
// movement from its own first appearance, not a fabricated full-balance swing.
// See PLANNING.md "D-NWT" deferred bug and net_worth_trend.rs implementation comment.
#[test]
fn type_absent_in_earliest_month_does_not_fabricate_full_balance_swing() {
    let mk = |month: &str, ty: &str, bal: f64| monarch_mcp::net_worth_trend::AccountTypeSnapshot {
        account_type: ty.to_string(),
        month: month.to_string(),
        balance: bal,
    };
    // brokerage only exists in the latest month (account opened mid-period).
    let snaps = vec![
        mk("2026-04", "depository", 10_000.0),
        mk("2026-05", "depository", 10_200.0),
        mk("2026-05", "brokerage", 50_000.0),
    ];
    let result = compute_trend(&snaps);
    let brokerage = result.by_account_type.get("brokerage").unwrap();
    // Correct semantics: brokerage first appears in 2026-05; its baseline is its
    // own first-seen balance (50_000). change = latest - first_seen = 50_000 - 50_000 = 0.
    // The fabricated-swing bug reports 50_000 - 0 = 50_000 instead.
    assert!(
        brokerage.change.abs() < f64::EPSILON,
        "brokerage absent in earliest month should report change=0 (first-seen baseline), got change={}",
        brokerage.change
    );
    // biggest_mover must be depository (+200), not brokerage (+0 after fix)
    let mover = result
        .biggest_mover
        .expect("must have a biggest mover with 2 months");
    assert_eq!(
        mover.account_type, "depository",
        "biggest_mover must be depository (+200), not brokerage (no real movement), got {}",
        mover.account_type
    );
}
