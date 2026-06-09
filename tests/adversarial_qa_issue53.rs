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
#[ignore = "RED: proves the duplicate-name silent-misrouting bug; un-ignore when \
            resolve_category_names rejects ambiguous names (dev fix for #53)"]
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
