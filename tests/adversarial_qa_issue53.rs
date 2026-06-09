//! Adversarial QA finding (Gate 3, issue #53 — category name → UUID resolution).
//!
//! Proves the duplicate-category-name silent-misrouting bug in
//! `resolve_category_names`. The resolver builds a `HashMap<&str, &str>` from the
//! category catalog via `.collect()`. When the catalog contains two categories
//! that share a display name but have different UUIDs — a real Monarch shape, since
//! the same name can exist under different groups, or as a system + custom pair —
//! the HashMap silently keeps only ONE entry (last-write-wins by iteration order).
//!
//! Consequence: a user approves `"category": "Pets"` and the transaction is routed
//! to whichever "Pets" UUID the catalog ordering happened to put last, with NO
//! rejection and NO warning. This is silent data corruption: the change lands in a
//! category the user never selected. The safe behavior for an ambiguous name is to
//! reject it (like an unknown name) with a clear reason, never to guess.
//!
//! This does NOT widen the mutation surface (still category/tags/notes only) and is
//! not a money-movement bug — but it silently applies the WRONG category.

use monarch_mcp::client::CategoryWithId;
use monarch_mcp::triage::{resolve_category_names, AppliedChange};

fn cat(id: &str, name: &str) -> CategoryWithId {
    CategoryWithId {
        id: id.to_string(),
        name: name.to_string(),
    }
}

fn applied(id: &str, category: &str) -> AppliedChange {
    AppliedChange {
        id: id.to_string(),
        category: Some(category.to_string()),
        tags: None,
        notes: None,
    }
}

#[test]
fn ambiguous_category_name_is_rejected_not_silently_misrouted() {
    // Two real categories share the display name "Pets" with different UUIDs.
    let categories = vec![
        cat("uuid-pets-system", "Pets"),
        cat("uuid-pets-custom", "Pets"),
    ];
    let changes = vec![applied("txn-amb", "Pets")];

    let (resolved, rejections) = resolve_category_names(&categories, changes);

    // The change must NOT be silently applied to one arbitrary UUID — that would
    // recategorize the transaction to a category the user never approved.
    assert_eq!(
        resolved.len(),
        0,
        "ambiguous name must not be silently resolved to an arbitrary UUID; got resolved: {:?}",
        resolved
    );
    assert_eq!(
        rejections.len(),
        1,
        "ambiguous category name must be rejected, not guessed"
    );
    assert_eq!(
        rejections[0].id, "txn-amb",
        "real txn id must be preserved on ambiguity rejection"
    );
    let reason = rejections[0].reason.to_lowercase();
    assert!(
        reason.contains("ambiguous") || reason.contains("multiple"),
        "rejection reason must explain the ambiguity, got: {:?}",
        rejections[0].reason
    );
}

// ---------------------------------------------------------------------------
// Run #2 hardening sweep — probe the ambiguity fix for NEW reachable bugs.
// ---------------------------------------------------------------------------

fn applied_opt(id: &str, category: Option<&str>) -> AppliedChange {
    AppliedChange {
        id: id.to_string(),
        category: category.map(str::to_string),
        tags: None,
        notes: None,
    }
}

#[test]
fn three_distinct_uuids_for_one_name_is_rejected_as_ambiguous() {
    // A name shared by 3 distinct categories must still reject (count >= 2 path).
    let categories = vec![
        cat("uuid-1", "Travel"),
        cat("uuid-2", "Travel"),
        cat("uuid-3", "Travel"),
    ];
    let (resolved, rejections) = resolve_category_names(&categories, vec![applied("t3", "Travel")]);
    assert_eq!(resolved.len(), 0, "3-way ambiguous name must not resolve");
    assert_eq!(rejections.len(), 1);
    assert_eq!(rejections[0].id, "t3");
    assert!(
        rejections[0].reason.contains("3 matches"),
        "reason must report the real distinct-UUID count, got: {:?}",
        rejections[0].reason
    );
}

#[test]
fn same_name_same_uuid_repeated_resolves_not_rejected() {
    // Dedup must compare UUIDs, not names: the same category returned twice
    // (a real possibility if the catalog has dup rows) is NOT ambiguous.
    let categories = vec![cat("uuid-dup", "Books"), cat("uuid-dup", "Books")];
    let (resolved, rejections) =
        resolve_category_names(&categories, vec![applied("t-dup", "Books")]);
    assert_eq!(rejections.len(), 0, "same-UUID-twice must not be ambiguous");
    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].category.as_deref(),
        Some("uuid-dup"),
        "must resolve to the single distinct UUID"
    );
}

#[test]
fn three_way_mixed_batch_partitions_resolve_ambiguous_unknown_with_ids_preserved() {
    // The decisive partition test: in one call, a resolvable name, an ambiguous
    // name, an unknown name, and a tags-only change must each land in the right
    // bucket with txn ids preserved and no cross-contamination.
    let categories = vec![
        cat("uuid-coffee", "Coffee"), // unique → resolves
        cat("uuid-pets-a", "Pets"),   // ambiguous
        cat("uuid-pets-b", "Pets"),   // ambiguous
    ];
    let changes = vec![
        applied("t-good", "Coffee"),  // → resolved to uuid-coffee
        applied("t-amb", "Pets"),     // → rejected ambiguous
        applied("t-unknown", "Nope"), // → rejected unknown
        applied_opt("t-tags", None),  // → resolved (passthrough)
    ];
    let (resolved, rejections) = resolve_category_names(&categories, changes);

    // Two resolved: the unique-name change and the tags-only passthrough.
    assert_eq!(resolved.len(), 2, "resolved: {:?}", resolved);
    let good = resolved.iter().find(|c| c.id == "t-good").unwrap();
    assert_eq!(good.category.as_deref(), Some("uuid-coffee"));
    let tags = resolved.iter().find(|c| c.id == "t-tags").unwrap();
    assert_eq!(tags.category, None, "tags-only passthrough must keep None");

    // Two rejected: ambiguous and unknown, with distinct reasons and real ids.
    assert_eq!(rejections.len(), 2, "rejections: {:?}", rejections);
    let amb = rejections.iter().find(|r| r.id == "t-amb").unwrap();
    assert!(
        amb.reason.to_lowercase().contains("ambiguous"),
        "ambiguous reason, got: {:?}",
        amb.reason
    );
    let unk = rejections.iter().find(|r| r.id == "t-unknown").unwrap();
    assert!(
        unk.reason.to_lowercase().contains("unknown"),
        "unknown reason, got: {:?}",
        unk.reason
    );

    // No resolved UUID leaks into the ambiguous rejection reason (no data leak).
    assert!(
        !amb.reason.contains("uuid-pets-a") && !amb.reason.contains("uuid-pets-b"),
        "ambiguity reason must not leak the candidate UUIDs, got: {:?}",
        amb.reason
    );
}
