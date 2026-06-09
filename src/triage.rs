//! Pure logic for `triage_uncategorized` and `apply_changeset`.
//!
//! No I/O here — all computation is against already-fetched data so it can be
//! unit-tested without a running mock server. The tool handlers in `tools.rs`
//! fetch data and delegate here.

use crate::client::{CategoryWithId, Transaction};
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
///
/// Only `category`, `tags`, and `notes` are mutable. Any other field (amount,
/// account, merchant, date, …) is unknown and rejected by `deny_unknown_fields`.
/// `id` identifies the transaction to update.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ChangeEntry {
    pub id: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
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
    /// Number of changeset entries processed (applied + rejected).
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
            suggestion_map
                .get(&txn.merchant_name)
                .map(|category| ProposedChange {
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
/// With `deny_unknown_fields` on `ChangeEntry`, serde already rejects any field
/// outside `{id, category, tags, notes}` at parse time. This function is the
/// post-parse gate for any remaining business-rule violations.
pub fn validate_change_entry(_entry: &ChangeEntry) -> Result<(), String> {
    Ok(())
}

/// Parse a list of raw JSON change entries, preserving the real transaction id
/// in a `RejectedChange` when serde fails (e.g. unknown fields, wrong types).
///
/// This replaces the previous `unwrap_or(ChangeEntry { all None })` pattern in
/// `tools.rs` that silently swallowed malformed or forbidden entries.
pub fn parse_raw_changes(raw: Vec<serde_json::Value>) -> Vec<ParsedEntry> {
    raw.into_iter()
        .map(|v| {
            // Extract the id independently so we can name the entry in rejections
            // even when the full parse fails.
            let id = v.get("id").and_then(|id| id.as_str()).map(str::to_string);

            match serde_json::from_value::<ChangeEntry>(v) {
                Ok(entry) => ParsedEntry::Ok(entry),
                Err(reason) => ParsedEntry::Rejected(RejectedChange {
                    id: id.unwrap_or_else(|| "unknown".to_string()),
                    reason: reason.to_string(),
                }),
            }
        })
        .collect()
}

/// Outcome of parsing one raw change entry.
pub enum ParsedEntry {
    Ok(ChangeEntry),
    Rejected(RejectedChange),
}

/// Resolve category names to Monarch category UUIDs.
///
/// Takes the caller-supplied `AppliedChange` list (which carries category *names*
/// at the MCP boundary) and the full category list from `get_categories()`.
///
/// For each change that carries a category name:
/// - If the name resolves to exactly one category UUID, the name is replaced with the UUID.
/// - If the name is ambiguous (maps to multiple distinct UUIDs), the change is rejected
///   with a clear reason — it is **never** sent to the Monarch API (silent misrouting
///   would apply the category to an arbitrary UUID the user never selected).
/// - If the name is unknown, the change is moved to the rejections list with a
///   clear reason — it is **never** sent to the Monarch API.
///
/// Changes with no category (tags/notes only) pass through unchanged.
///
/// Returns `(resolved_changes, new_rejections)`.
pub fn resolve_category_names(
    categories: &[CategoryWithId],
    applied: Vec<AppliedChange>,
) -> (Vec<AppliedChange>, Vec<RejectedChange>) {
    // Build a map from category name to all distinct UUIDs that share that name.
    let mut name_to_ids: HashMap<&str, Vec<&str>> = HashMap::new();
    for cat in categories {
        name_to_ids
            .entry(cat.name.as_str())
            .or_default()
            .push(cat.id.as_str());
    }

    // Deduplicate the UUID lists so we can detect truly distinct IDs.
    for ids in name_to_ids.values_mut() {
        ids.sort();
        ids.dedup();
    }

    let mut resolved = Vec::new();
    let mut rejections = Vec::new();

    for mut change in applied {
        match &change.category {
            None => resolved.push(change),
            Some(name) => match name_to_ids.get(name.as_str()) {
                Some(ids) if ids.len() == 1 => {
                    // Unambiguous: exactly one distinct UUID for this name.
                    change.category = Some(ids[0].to_string());
                    resolved.push(change);
                }
                Some(ids) if ids.len() > 1 => {
                    // Ambiguous: multiple distinct UUIDs map to the same name.
                    // Reject to avoid silent misrouting.
                    rejections.push(RejectedChange {
                        id: change.id,
                        reason: format!("ambiguous category {:?}: {} matches", name, ids.len()),
                    });
                }
                _ => {
                    // Unknown name (no entry in name_to_ids).
                    rejections.push(RejectedChange {
                        id: change.id,
                        reason: format!("unknown category {:?}", name),
                    });
                }
            },
        }
    }

    (resolved, rejections)
}

/// Apply an approved changeset, filtering out forbidden mutations.
///
/// Each `ParsedEntry` is either already-rejected (parse failed) or valid.
/// Valid entries pass through `validate_change_entry` for any remaining
/// business-rule checks. The returned `transaction_count` is the number of
/// entries processed (applied + rejected).
pub fn partition_changeset(entries: &[ParsedEntry]) -> ApplyResult {
    let mut applied_changes = Vec::new();
    let mut rejected_changes = Vec::new();

    for entry in entries {
        match entry {
            ParsedEntry::Rejected(r) => {
                rejected_changes.push(RejectedChange {
                    id: r.id.clone(),
                    reason: r.reason.clone(),
                });
            }
            ParsedEntry::Ok(entry) => {
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
        }
    }

    ApplyResult {
        transaction_count: applied_changes.len() + rejected_changes.len(),
        applied_changes,
        rejected_changes,
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
                group_type: Some("expense".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    fn make_change(id: Option<&str>, category: Option<&str>) -> ChangeEntry {
        ChangeEntry {
            id: id.map(str::to_string),
            category: category.map(str::to_string),
            tags: None,
            notes: None,
        }
    }

    fn wrap_ok(entries: Vec<ChangeEntry>) -> Vec<ParsedEntry> {
        entries.into_iter().map(ParsedEntry::Ok).collect()
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
        let entry = make_change(Some("t1"), Some("Coffee"));
        assert!(validate_change_entry(&entry).is_ok());
    }

    // -----------------------------------------------------------------------
    // 9b GREEN → 9c TRIANGULATE: amount field is forbidden (via serde rejection)
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_with_amount_is_rejected() {
        let raw = serde_json::json!({"id": "t1", "amount": 0.0});
        let entries = parse_raw_changes(vec![raw]);
        let result = partition_changeset(&entries);
        assert_eq!(result.rejected_changes.len(), 1, "amount must be rejected");
        assert_eq!(result.rejected_changes[0].id, "t1");
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: nonzero amount is also rejected (triangulate zero vs nonzero)
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_with_nonzero_amount_is_rejected() {
        let raw = serde_json::json!({"id": "t-nonzero", "amount": 99.99});
        let entries = parse_raw_changes(vec![raw]);
        let result = partition_changeset(&entries);
        assert_eq!(
            result.applied_changes.len(),
            0,
            "nonzero amount must be rejected"
        );
        assert_eq!(
            result.rejected_changes.len(),
            1,
            "one rejection expected for nonzero amount"
        );
        assert_eq!(
            result.rejected_changes[0].id, "t-nonzero",
            "real txn id must be preserved"
        );
    }

    // -----------------------------------------------------------------------
    // 9a RED: partition_changeset separates valid from rejected entries
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_separates_valid_and_rejected_entries() {
        // t1: valid (category only); t2: forbidden (amount via parse rejection)
        let valid = ParsedEntry::Ok(make_change(Some("t1"), Some("Coffee")));
        let rejected = ParsedEntry::Rejected(RejectedChange {
            id: "t2".to_string(),
            reason: "amount_change_forbidden".to_string(),
        });
        let result = partition_changeset(&[valid, rejected]);
        assert_eq!(result.applied_changes.len(), 1);
        assert_eq!(result.applied_changes[0].id, "t1");
        assert_eq!(result.rejected_changes.len(), 1);
        assert_eq!(result.rejected_changes[0].id, "t2");
        // transaction_count is derived as applied + rejected; with both buckets
        // non-empty it must be their sum (catches a mutant that counts only one).
        assert_eq!(result.transaction_count, 2);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: transaction_count equals entries processed
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_reports_entries_processed_count() {
        let entries = wrap_ok(vec![make_change(Some("t1"), Some("Coffee"))]);
        let result = partition_changeset(&entries);
        assert_eq!(result.transaction_count, 1);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: empty changeset is valid with zero applied
    // -----------------------------------------------------------------------

    #[test]
    fn partition_changeset_handles_empty_input() {
        let result = partition_changeset(&[]);
        assert!(result.applied_changes.is_empty());
        assert!(result.rejected_changes.is_empty());
        assert_eq!(result.transaction_count, 0);
    }

    // -----------------------------------------------------------------------
    // BUG 2 RED: forbidden fields account/merchant/date must be rejected
    // -----------------------------------------------------------------------

    #[test]
    fn change_entry_with_account_field_is_rejected() {
        // account reassignment is forbidden — only category/tags/notes are allowed
        let raw = serde_json::json!({
            "id": "t-acct", "category": "Coffee", "account": "other-account-id"
        });
        let result = parse_and_partition_single(raw);
        assert_eq!(
            result.applied_changes.len(),
            0,
            "account field must be rejected, not applied"
        );
        assert_eq!(
            result.rejected_changes.len(),
            1,
            "must have one rejected entry"
        );
        assert_eq!(
            result.rejected_changes[0].id, "t-acct",
            "real id must be preserved in rejection"
        );
    }

    #[test]
    fn change_entry_with_date_field_is_rejected() {
        let raw = serde_json::json!({
            "id": "t-date", "category": "Dining", "date": "2099-01-01"
        });
        let result = parse_and_partition_single(raw);
        assert_eq!(
            result.applied_changes.len(),
            0,
            "date field must be rejected"
        );
        assert_eq!(
            result.rejected_changes[0].id, "t-date",
            "real id must be preserved"
        );
    }

    // BUG 2b RED: amount as string must be rejected with real id, not swallowed
    #[test]
    fn amount_as_string_is_rejected_not_silently_swallowed() {
        // serde would fail to parse "100.00" as f64, unwrap_or used to drop to all-None
        // entry with id "unknown" and push it to applied — this must instead be a rejection
        let raw = serde_json::json!({
            "id": "t-stramt", "amount": "100.00"
        });
        let result = parse_and_partition_single(raw);
        assert_eq!(
            result.applied_changes.len(),
            0,
            "amount-as-string must not appear in applied_changes"
        );
        assert!(
            !result.rejected_changes.is_empty(),
            "amount-as-string entry must appear in rejected_changes"
        );
        assert_eq!(
            result.rejected_changes[0].id, "t-stramt",
            "real transaction id must be preserved, not 'unknown'"
        );
    }

    #[test]
    fn unknown_id_sentinel_never_appears_in_applied_changes() {
        // "unknown" as an id signals that parse_entries lost the real id — it must never
        // appear in applied_changes since that would trigger update_transaction("unknown", …)
        let raw = serde_json::json!({
            "id": "t-real", "amount": "bad-value"
        });
        let result = parse_and_partition_single(raw);
        let has_unknown_applied = result.applied_changes.iter().any(|a| a.id == "unknown");
        assert!(
            !has_unknown_applied,
            "id 'unknown' must never appear in applied_changes; got: {:?}",
            result.applied_changes
        );
    }

    // BUG 2c RED: malformed-but-legitimate entry must NOT be silently dropped
    #[test]
    fn legitimate_entry_with_malformed_tags_preserves_id_in_rejection() {
        // tags must be an array; a string value fails serde. The entry must be
        // rejected with the real id, not silently become a no-op with id "unknown".
        let raw = serde_json::json!({
            "id": "t-tags", "category": "Dining", "tags": "not-an-array"
        });
        let result = parse_and_partition_single(raw);
        // Either: parsed OK (tags ignored/coerced) with real id applied, OR
        // rejected with real id. What must NOT happen: id "unknown" in applied.
        let unknown_applied = result.applied_changes.iter().any(|a| a.id == "unknown");
        assert!(
            !unknown_applied,
            "malformed entry must not produce id 'unknown' in applied_changes; got: {:?}",
            result.applied_changes
        );
        // The real id must be traceable (either applied with correct id or rejected with real id)
        let real_id_present = result.applied_changes.iter().any(|a| a.id == "t-tags")
            || result.rejected_changes.iter().any(|r| r.id == "t-tags");
        assert!(
            real_id_present,
            "real id 't-tags' must appear somewhere in result; applied={:?} rejected={:?}",
            result.applied_changes, result.rejected_changes
        );
    }

    /// Helper: parse a single raw JSON value through the full parse+partition path
    /// (mirrors the tools.rs apply_approved_changeset logic).
    fn parse_and_partition_single(raw: serde_json::Value) -> ApplyResult {
        let entries = parse_raw_changes(vec![raw]);
        partition_changeset(&entries)
    }

    // -----------------------------------------------------------------------
    // 9a RED: resolve_category_names — known name resolves to its id
    // -----------------------------------------------------------------------

    fn make_cat(id: &str, name: &str) -> CategoryWithId {
        CategoryWithId {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn make_applied(id: &str, category: Option<&str>) -> AppliedChange {
        AppliedChange {
            id: id.to_string(),
            category: category.map(str::to_string),
            tags: None,
            notes: None,
        }
    }

    #[test]
    fn known_category_name_resolves_to_id() {
        let categories = vec![make_cat("cat-uuid-1", "Pets")];
        let applied = vec![make_applied("txn-1", Some("Pets"))];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(rejections.len(), 0, "no rejections expected for known name");
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].category.as_deref(),
            Some("cat-uuid-1"),
            "category should be resolved to UUID"
        );
    }

    #[test]
    fn unknown_category_name_produces_rejection_with_txn_id_preserved() {
        let categories = vec![make_cat("cat-uuid-1", "Pets")];
        let applied = vec![make_applied("txn-99", Some("NoSuchCategory"))];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(
            resolved.len(),
            0,
            "unknown category must not be in resolved"
        );
        assert_eq!(rejections.len(), 1, "one rejection expected");
        assert_eq!(rejections[0].id, "txn-99", "real txn id must be preserved");
        assert!(
            rejections[0].reason.contains("NoSuchCategory"),
            "rejection reason must name the unknown category, got: {:?}",
            rejections[0].reason
        );
    }

    #[test]
    fn change_with_no_category_passes_through_untouched() {
        let categories = vec![make_cat("cat-uuid-1", "Pets")];
        // tags-only change — no category field
        let applied = vec![AppliedChange {
            id: "txn-tags".to_string(),
            category: None,
            tags: Some(vec!["tag-a".to_string()]),
            notes: None,
        }];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(rejections.len(), 0, "no rejection for tags-only change");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].category, None, "category must remain None");
        assert_eq!(
            resolved[0].tags,
            Some(vec!["tag-a".to_string()]),
            "tags must pass through unchanged"
        );
    }

    #[test]
    fn change_with_notes_and_no_category_passes_through_untouched() {
        let categories = vec![make_cat("cat-uuid-1", "Pets")];
        // notes-only change — no category field
        let applied = vec![AppliedChange {
            id: "txn-notes".to_string(),
            category: None,
            tags: None,
            notes: Some("important note".to_string()),
        }];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(rejections.len(), 0, "no rejection for notes-only change");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].category, None, "category must remain None");
        assert_eq!(
            resolved[0].notes,
            Some("important note".to_string()),
            "notes must pass through unchanged"
        );
    }

    #[test]
    fn resolution_map_built_correctly_from_multiple_categories() {
        let categories = vec![
            make_cat("id-a", "Coffee"),
            make_cat("id-b", "Pets"),
            make_cat("id-c", "Dining"),
        ];
        let applied = vec![
            make_applied("t1", Some("Coffee")),
            make_applied("t2", Some("Dining")),
        ];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(rejections.len(), 0);
        assert_eq!(resolved.len(), 2);
        let coffee_resolved = resolved.iter().find(|c| c.id == "t1").unwrap();
        let dining_resolved = resolved.iter().find(|c| c.id == "t2").unwrap();
        assert_eq!(coffee_resolved.category.as_deref(), Some("id-a"));
        assert_eq!(dining_resolved.category.as_deref(), Some("id-c"));
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: mixed batch — known and unknown in same call
    // -----------------------------------------------------------------------

    #[test]
    fn mixed_batch_separates_resolved_and_rejected() {
        let categories = vec![make_cat("id-a", "Coffee"), make_cat("id-b", "Pets")];
        let applied = vec![
            make_applied("t-known", Some("Coffee")),
            make_applied("t-unknown", Some("MadeUp")),
            make_applied("t-no-cat", None), // tags/notes only
        ];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        // Only the unknown name becomes a rejection
        assert_eq!(rejections.len(), 1, "one rejection for unknown name");
        assert_eq!(rejections[0].id, "t-unknown");
        // The known name and the no-category change are both resolved
        assert_eq!(resolved.len(), 2);
        let known = resolved.iter().find(|c| c.id == "t-known").unwrap();
        assert_eq!(known.category.as_deref(), Some("id-a"));
        let no_cat = resolved.iter().find(|c| c.id == "t-no-cat").unwrap();
        assert_eq!(no_cat.category, None);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: empty categories list → all named changes rejected
    // -----------------------------------------------------------------------

    #[test]
    fn empty_category_list_rejects_all_named_changes() {
        let categories: Vec<CategoryWithId> = vec![];
        let applied = vec![make_applied("t1", Some("Pets"))];
        let (resolved, rejections) = resolve_category_names(&categories, applied);
        assert_eq!(
            resolved.len(),
            0,
            "no resolvable changes with empty catalog"
        );
        assert_eq!(rejections.len(), 1, "one rejection for unknown category");
        assert_eq!(rejections[0].id, "t1");
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: ambiguous category name must be rejected, not silently
    // resolved to an arbitrary UUID (issue #53)
    // -----------------------------------------------------------------------

    #[test]
    fn ambiguous_category_name_is_rejected() {
        // Two distinct UUIDs share the same display name "Pets".
        // This is a real Monarch shape (same name under different groups, or
        // system + custom pair). The resolver must reject the ambiguous name,
        // not silently apply it to one arbitrary UUID.
        let categories = vec![
            make_cat("uuid-pets-system", "Pets"),
            make_cat("uuid-pets-custom", "Pets"),
        ];
        let applied = vec![make_applied("txn-1", Some("Pets"))];
        let (resolved, rejections) = resolve_category_names(&categories, applied);

        // The change must be rejected, not silently routed to an arbitrary UUID.
        assert_eq!(resolved.len(), 0, "ambiguous name must not be resolved");
        assert_eq!(rejections.len(), 1, "one rejection for ambiguous name");
        assert_eq!(rejections[0].id, "txn-1", "real txn id must be preserved");
        assert!(
            rejections[0].reason.to_lowercase().contains("ambiguous")
                || rejections[0].reason.to_lowercase().contains("multiple"),
            "rejection reason must explain the ambiguity; got: {:?}",
            rejections[0].reason
        );
        // Pin the exact distinct-UUID count to catch mutations that alter the math
        assert!(
            rejections[0].reason.contains("2 matches"),
            "rejection reason must pin the distinct UUID count; got: {:?}",
            rejections[0].reason
        );
    }

    #[test]
    fn same_category_name_with_same_uuid_twice_is_not_ambiguous() {
        // If the same name + same UUID appears twice in the catalog
        // (e.g., duplicate or lazy evaluation), deduplicating by UUID
        // means it resolves as unambiguous (only one distinct UUID).
        let categories = vec![
            make_cat("uuid-pets-1", "Pets"),
            make_cat("uuid-pets-1", "Pets"), // same UUID, same name
        ];
        let applied = vec![make_applied("txn-1", Some("Pets"))];
        let (resolved, rejections) = resolve_category_names(&categories, applied);

        // Should resolve normally, not be rejected as ambiguous.
        assert_eq!(
            resolved.len(),
            1,
            "same UUID deduplication must allow resolve"
        );
        assert_eq!(resolved[0].category.as_deref(), Some("uuid-pets-1"));
        assert_eq!(rejections.len(), 0, "no rejection for non-ambiguous case");
    }
}
