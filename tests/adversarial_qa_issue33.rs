//! Adversarial QA (Gate 3, penetration test) — issue #33 live-fetch / pagination.
//!
//! These tests characterize the robustness of `get_transactions_with_count`'s
//! `totalCount` parse path and the behavior-preservation of the
//! `get_transactions` → `get_transactions_with_count` delegation.
//!
//! Run: `cargo test --test adversarial_qa_issue33`
//!
//! VERDICT (run #1): No BUG found. The probes below document that the
//! `as_u64().unwrap_or(0)` default on a missing/null/non-integer `totalCount`
//! is BENIGN, not a silent-truncation bug, because:
//!   - `get_transactions` (the production path) discards `totalCount` entirely,
//!     so a silent-zero count cannot affect `total_spent`.
//!   - The only consumer of the count is the live truncation-guard test, which
//!     asserts `results.len() == totalCount`. A silent-zero would make that
//!     assertion FAIL loudly whenever results are non-empty — the SAFE
//!     direction (it never produces a false "no cap" pass when results exist).
//!
//! These are characterization tests, NOT bug-proving tests. They are written
//! against the same wiremock pattern the in-crate small tier uses, but live in
//! tests/ because src/ may not be edited during adversarial QA.

use monarch_mcp::client::MonarchClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a client pointed at the mock server with a dummy token already set.
/// `MonarchClient` does not expose its `token` field publicly, so we drive
/// authentication through the env-var path (`resolve_token_from_env_or_disk`)
/// using a process-local temp config dir to avoid touching the real session.
/// Uses `temp_env::with_var` to scope the MONARCH_TOKEN env var only around
/// construction, preventing cross-test bleed. The token is copied into the
/// client's field during resolve_token_from_env_or_disk, so scoping just
/// the construction is sufficient.
fn live_client_for(base: &str) -> MonarchClient {
    temp_env::with_var("MONARCH_TOKEN", Some("adversarial-test-token"), || {
        let mut c = MonarchClient::new(Some(base.to_string()));
        c.resolve_token_from_env_or_disk();
        c
    })
}

fn one_result() -> serde_json::Value {
    json!([
        {
            "id": "txn-1",
            "amount": -42.00,
            "date": "2026-06-01",
            "needsReview": false,
            "notes": "",
            "category": {"id": "c1", "name": "Groceries",
                "group": {"type": "expense", "__typename": "CategoryGroup"},
                "__typename": "Category"},
            "merchant": {"name": "Store", "id": "m1", "__typename": "Merchant"},
            "account": {"id": "a1", "displayName": "Checking", "__typename": "Account"},
            "tags": [],
            "__typename": "Transaction"
        }
    ])
}

/// PROBE 1: `totalCount` absent from the response.
/// Characterizes: results still parse; totalCount silently defaults to 0.
/// This is the SAFE direction — production drops the count, and the live guard
/// would fail loudly (1 != 0) rather than pass falsely.
#[tokio::test]
async fn missing_total_count_defaults_to_zero_results_still_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "allTransactions": {
                    // NOTE: no "totalCount" key at all.
                    "results": one_result(),
                    "__typename": "TransactionList"
                }
            }
        })))
        .mount(&server)
        .await;

    let client = live_client_for(&server.uri());
    let (txns, total) = client
        .get_transactions_with_count("2026-06-01", "2026-06-30", 100)
        .await
        .expect("missing totalCount must NOT error — results still parse");

    assert_eq!(
        txns.len(),
        1,
        "results must still parse when totalCount absent"
    );
    assert_eq!(
        total, 0,
        "documented behavior: absent totalCount defaults to 0 (benign — \
         production discards it; live guard would fail loudly on len!=count)"
    );
}

/// PROBE 2: `totalCount` is JSON null.
#[tokio::test]
async fn null_total_count_defaults_to_zero() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "allTransactions": {
                    "totalCount": null,
                    "results": one_result(),
                    "__typename": "TransactionList"
                }
            }
        })))
        .mount(&server)
        .await;

    let client = live_client_for(&server.uri());
    let (txns, total) = client
        .get_transactions_with_count("2026-06-01", "2026-06-30", 100)
        .await
        .expect("null totalCount must NOT error");
    assert_eq!(txns.len(), 1);
    assert_eq!(total, 0, "null totalCount defaults to 0 (benign)");
}

/// PROBE 3: `totalCount` is a string ("149") rather than an integer.
/// `.as_u64()` returns None for a JSON string → defaults to 0. No panic.
#[tokio::test]
async fn string_total_count_does_not_panic_defaults_to_zero() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "allTransactions": {
                    "totalCount": "149",
                    "results": one_result(),
                    "__typename": "TransactionList"
                }
            }
        })))
        .mount(&server)
        .await;

    let client = live_client_for(&server.uri());
    let (txns, total) = client
        .get_transactions_with_count("2026-06-01", "2026-06-30", 100)
        .await
        .expect("string totalCount must NOT panic");
    assert_eq!(txns.len(), 1);
    assert_eq!(total, 0, "non-integer totalCount defaults to 0, no panic");
}

/// PROBE 4 (delegation equivalence): `get_transactions` returns exactly the
/// same Vec that `get_transactions_with_count` produces for the same response.
/// Proves the refactor that made `get_transactions` delegate is behavior-
/// preserving for the result vector.
#[tokio::test]
async fn get_transactions_returns_same_vec_as_with_count() {
    let body = json!({
        "data": {
            "allTransactions": {
                "totalCount": 1,
                "results": one_result(),
                "__typename": "TransactionList"
            }
        }
    });

    // Server A drives get_transactions_with_count.
    let server_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server_a)
        .await;
    let client_a = live_client_for(&server_a.uri());
    let (with_count_txns, count) = client_a
        .get_transactions_with_count("2026-06-01", "2026-06-30", 100)
        .await
        .unwrap();

    // Server B drives the delegating get_transactions.
    let server_b = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server_b)
        .await;
    let client_b = live_client_for(&server_b.uri());
    let plain_txns = client_b
        .get_transactions("2026-06-01", "2026-06-30", 100)
        .await
        .unwrap();

    assert_eq!(count, 1);
    assert_eq!(
        plain_txns.len(),
        with_count_txns.len(),
        "get_transactions must return the same number of txns as with_count"
    );
    // Field-by-field equivalence on the mapped values.
    for (a, b) in plain_txns.iter().zip(with_count_txns.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.amount, b.amount);
        assert_eq!(a.date, b.date);
        assert_eq!(a.merchant_name, b.merchant_name);
        assert_eq!(a.category.name, b.category.name);
        assert_eq!(a.category.group_type, b.category.group_type);
    }
}
