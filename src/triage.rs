//! Pure logic for `triage_uncategorized` and `apply_changeset`.
//!
//! No I/O here — all computation is against already-fetched data so it can be
//! unit-tested without a running mock server. The tool handlers in `tools.rs`
//! fetch data and delegate here.

use crate::client::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public output types
// ---------------------------------------------------------------------------

/// A single proposed change for one transaction.
/// Only emitted when a category suggestion exists — unknown merchants are omitted.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct ProposedChange {
    pub id: String,
    pub merchant: String,
    /// Suggested category name derived from household history.
    pub category: String,
}

/// The result of `triage_uncategorized` — a list of proposed changes for review.
#[derive(Debug, Serialize, PartialEq)]
pub struct TriageResult {
    pub proposed_changes: Vec<ProposedChange>,
}

/// A single change entry supplied by the caller in an `apply_changeset` request.
#[derive(Debug, Deserialize, Clone)]
pub struct ChangeEntry {
    pub id: Option<String>,
    /// Merchant name used for lookup when `id` is absent.
    #[allow(dead_code)]
    pub merchant: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    // Any other fields — we check for forbidden ones below
    pub amount: Option<f64>,
}

/// One applied or rejected change in the result.
#[derive(Debug, Serialize, PartialEq)]
pub struct AppliedChange {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// A change entry that was rejected because it contained a forbidden field.
#[derive(Debug, Serialize, PartialEq)]
pub struct RejectedChange {
    /// The `id` from the input entry (may be unknown if the entry lacked one).
    pub id: String,
    pub reason: String,
}

/// The result of `apply_changeset`.
#[derive(Debug, Serialize, PartialEq)]
pub struct ApplyResult {
    pub applied_changes: Vec<AppliedChange>,
    pub rejected_changes: Vec<RejectedChange>,
    /// Total transaction count after applying — must equal the count before.
    pub transaction_count: usize,
}

// ---------------------------------------------------------------------------
// Pure computation
// ---------------------------------------------------------------------------

/// Build a category-suggestion map from transaction history.
///
/// For each merchant in `history` that has a non-"Uncategorized" category,
/// record the most recently seen category. `uncategorized_txns` are excluded
/// from the history used for inference — they are the *subjects* of triage.
pub fn build_category_suggestion_map(history: &[Transaction]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for txn in history {
        let cat = &txn.category.name;
        if cat != "Uncategorized" {
            map.insert(txn.merchant_name.clone(), cat.clone());
        }
    }
    map
}

/// Propose a category for each uncategorized transaction using household history.
///
/// Only transactions whose merchant appears in `suggestion_map` are included.
/// Unknown merchants are omitted from the result entirely — no proposal means
/// no action is expected.
pub fn propose_changes(
    uncategorized: &[Transaction],
    suggestion_map: &HashMap<String, String>,
) -> TriageResult {
    let proposed_changes = uncategorized
        .iter()
        .filter_map(|txn| {
            suggestion_map.get(&txn.merchant_name).map(|category| ProposedChange {
                id: txn.id.clone(),
                merchant: txn.merchant_name.clone(),
                category: category.clone(),
            })
        })
        .collect();
    TriageResult { proposed_changes }
}

/// Validate a change entry.
///
/// Returns `Err(reason)` when the entry contains a forbidden field (e.g. `amount`).
pub fn validate_change_entry(entry: &ChangeEntry) -> Result<(), String> {
    if entry.amount.is_some() {
        return Err("amount_change_forbidden".to_string());
    }
    Ok(())
}

/// Apply an approved changeset, filtering out forbidden mutations.
///
/// Each entry is validated before being passed to the caller's apply function.
/// Entries with forbidden fields are collected in `rejected_changes`; allowed
/// entries are collected in `applied_changes`. The `total_transaction_count`
/// is threaded through to the result so callers can assert the id-set is
/// unchanged.
pub fn partition_changeset(
    entries: &[ChangeEntry],
    total_transaction_count: usize,
) -> ApplyResult {
    let mut applied_changes = Vec::new();
    let mut rejected_changes = Vec::new();

    for entry in entries {
        let id = entry.id.clone().unwrap_or_else(|| "unknown".to_string());
        match validate_change_entry(entry) {
            Ok(()) => {
                applied_changes.push(AppliedChange {
                    id,
                    category: entry.category.clone(),
                    tags: entry.tags.clone(),
                    notes: entry.notes.clone(),
                });
            }
            Err(reason) => {
                rejected_changes.push(RejectedChange { id, reason });
            }
        }
    }

    ApplyResult {
        applied_changes,
        rejected_changes,
        transaction_count: total_transaction_count,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Category, Transaction};

    fn make_txn(id: &str, merchant: &str, category: &str) -> Transaction {
        Transaction {
            id: id.to_string(),
            amount: 5.50,
            date: "2026-05-20".to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
            },
            tags: vec![],
            notes: String::new(),
        }
    }

    fn make_change(id: Option<&str>, category: Option<&str>, amount: Option<f64>) -> ChangeEntry {
        ChangeEntry {
            id: id.map(str::to_string),
            merchant: None,
            category: category.map(str::to_string),
            tags: None,
            notes: None,
            amount,
        }
    }

    // -----------------------------------------------------------------------
    // 9a RED: known merchant gets its historical category suggested
    // -----------------------------------------------------------------------

    #[test]
    fn known_merchant_gets_historical_category_suggestion() {
        let history = vec![make_txn("h1", "Blue Bottle", "Coffee")];
        let map = build_category_suggestion_map(&history);
        assert_eq!(map.get("Blue Bottle"), Some(&"Coffee".to_string()));
    }

    // -----------------------------------------------------------------------
    // 9b GREEN already passes → 9c TRIANGULATE: unknown merchant → omitted
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_merchant_produces_no_proposal_entry() {
        let history: Vec<Transaction> = vec![];
        let map = build_category_suggestion_map(&history);
        let uncategorized = vec![make_txn("t1", "Mystery Merchant", "Uncategorized")];
        let result = propose_changes(&uncategorized, &map);
        assert!(
            result.proposed_changes.is_empty(),
            "unknown merchant should produce no proposal, got: {:?}",
            result.proposed_changes
        );
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: uncategorized history entry is ignored for inference
    // -----------------------------------------------------------------------

    #[test]
    fn uncategorized_history_entry_is_not_used_for_inference() {
        // A previous transaction from the same merchant that was also uncategorized
        // should not propagate "Uncategorized" as a suggestion — it is excluded
        // from the suggestion map, so the merchant produces no proposal at all.
        let history = vec![make_txn("h2", "Mystery Merchant", "Uncategorized")];
        let map = build_category_suggestion_map(&history);
        let uncategorized = vec![make_txn("t2", "Mystery Merchant", "Uncategorized")];
        let result = propose_changes(&uncategorized, &map);
        assert!(
            result.proposed_changes.is_empty(),
            "Uncategorized history should produce no proposal, got: {:?}",
            result.proposed_changes
        );
    }

    // -----------------------------------------------------------------------
    // 9a RED: propose_changes builds correct ProposedChange for known merchant
    // -----------------------------------------------------------------------

    #[test]
    fn propose_changes_assigns_correct_category_to_known_merchant() {
        let history = vec![make_txn("h3", "Blue Bottle", "Coffee")];
        let map = build_category_suggestion_map(&history);
        let uncategorized = vec![make_txn("new-1", "Blue Bottle", "Uncategorized")];
        let result = propose_changes(&uncategorized, &map);
        assert_eq!(result.proposed_changes.len(), 1);
        let proposal = &result.proposed_changes[0];
        assert_eq!(proposal.merchant, "Blue Bottle");
        assert_eq!(proposal.category, "Coffee".to_string());
        assert_eq!(proposal.id, "new-1");
    }

    // -----------------------------------------------------------------------
    // 9a RED: validate_change_entry allows category/tags/notes
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_with_only_category_is_valid() {
        let entry = make_change(Some("t1"), Some("Coffee"), None);
        assert!(validate_change_entry(&entry).is_ok());
    }

    // -----------------------------------------------------------------------
    // 9b GREEN → 9c TRIANGULATE: amount field is forbidden
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_with_amount_is_rejected() {
        let entry = make_change(Some("t1"), None, Some(0.0));
        let err = validate_change_entry(&entry).unwrap_err();
        assert_eq!(err, "amount_change_forbidden");
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: amount=0 is still rejected (not just nonzero)
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_amount_zero_is_also_rejected() {
        let entry = make_change(Some("t1"), None, Some(0.0));
        assert!(validate_change_entry(&entry).is_err());
    }

    // -----------------------------------------------------------------------
    // 9a RED: partition_changeset separates valid from rejected entries
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_separates_valid_and_rejected_entries() {
        let entries = vec![
            make_change(Some("t1"), Some("Coffee"), None),
            make_change(Some("t2"), None, Some(0.0)),
        ];
        let result = partition_changeset(&entries, 10);
        assert_eq!(result.applied_changes.len(), 1);
        assert_eq!(result.applied_changes[0].id, "t1");
        assert_eq!(result.rejected_changes.len(), 1);
        assert_eq!(result.rejected_changes[0].id, "t2");
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: transaction_count is preserved regardless of changes
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_preserves_transaction_count() {
        let entries = vec![make_change(Some("t1"), Some("Coffee"), None)];
        let result = partition_changeset(&entries, 40);
        assert_eq!(result.transaction_count, 40);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: empty changeset is valid with zero applied
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_handles_empty_input() {
        let result = partition_changeset(&[], 5);
        assert!(result.applied_changes.is_empty());
        assert!(result.rejected_changes.is_empty());
        assert_eq!(result.transaction_count, 5);
    }
}
