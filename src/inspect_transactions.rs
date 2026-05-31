//! Pure aggregation logic for the `inspect_transactions` tool.
//!
//! All arithmetic lives here, separated from I/O, so it can be unit-tested
//! without standing up a mock server. The tool handler in `tools.rs` fetches
//! data then delegates to [`compute_inspection`].
//!
//! # Compound value-add over a raw passthrough
//!
//! Beyond listing line items, this function splits totals into **inflow vs
//! outflow** so refunds and credits are visible at a glance alongside charges.
//! The caller receives:
//! - Every transaction's `id` — usable directly with `apply_changeset`.
//! - `total_count`, `net_amount`, `total_inflow`, `total_outflow`.
//!
//! # Two-step re-categorization pattern
//!
//! ```text
//! 1. inspect_transactions category="Pets" → returns [{id:"txn-abc", amount:-12000, ...}, ...]
//! 2. apply_changeset changes=[{id:"txn-abc", category:"Veterinary"}]
//! ```
//!
//! The `id` field in `inspect_transactions` output is the exact value required
//! by `apply_changeset`. No lookup step is needed.

use crate::client::Transaction;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Input filter — passed from the tool handler
// ---------------------------------------------------------------------------

/// Optional filters for narrowing the transaction list.
///
/// All fields are optional. `None` means "no filter on this dimension".
/// An empty or whitespace-only string is treated identically to `None` —
/// it imposes no constraint on that dimension. This invariant is enforced
/// at both the handler boundary (`fetch_and_compute_inspection`) and inside
/// `matches_filter`, so callers that pass `Some("")` get the same result as
/// callers that pass `None`.
///
/// If no `start_date`/`end_date` is provided the handler defaults to the
/// current calendar month.
#[derive(Debug, Default)]
pub struct InspectFilter {
    /// Substring match on category name (case-insensitive).
    /// `None` or an empty/whitespace-only value means "no constraint".
    pub category: Option<String>,
    /// Substring match on merchant name (case-insensitive).
    /// `None` or an empty/whitespace-only value means "no constraint".
    pub merchant: Option<String>,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// One line item in the inspection result — includes the transaction id so the
/// caller can pass it directly to `apply_changeset`.
#[derive(Debug, Serialize, PartialEq)]
pub struct InspectLineItem {
    /// Monarch transaction id — use this in `apply_changeset.changes[].id`.
    pub id: String,
    pub merchant: String,
    /// Positive = inflow (refund/credit), negative = outflow (charge).
    pub amount: f64,
    pub date: String,
    pub category: String,
    pub tags: Vec<String>,
    pub notes: String,
}

/// Summary computed over all matching line items.
#[derive(Debug, Serialize, PartialEq)]
pub struct InspectSummary {
    pub total_count: usize,
    /// Algebraic sum: positive amounts minus negative amounts.
    pub net_amount: f64,
    /// Sum of positive amounts (refunds, credits, income postings).
    pub total_inflow: f64,
    /// Sum of absolute values of negative amounts (charges, payments).
    pub total_outflow: f64,
}

/// Full result returned as JSON inside an MCP `CallToolResult`.
#[derive(Debug, Serialize, PartialEq)]
pub struct InspectionResult {
    pub summary: InspectSummary,
    pub transactions: Vec<InspectLineItem>,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Coerces an empty or whitespace-only `Some` value to `None`.
///
/// This ensures that `blank_to_none(Some(""))` and `blank_to_none(Some("  "))`
/// behave identically to `None` — no filter on that dimension — rather than
/// silently matching every transaction via `"".contains("")`.
pub fn blank_to_none(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Pure computation — no I/O
// ---------------------------------------------------------------------------

/// Filter `transactions` by `filter` and compute the compound summary.
///
/// Filtering is applied in-process (client-side) so the tool works with the
/// same `get_transactions` call used by other tools — no new GraphQL operation
/// is needed. The GraphQL layer already handles date-range narrowing via
/// `startDate`/`endDate`; category and merchant are narrowed here.
pub fn compute_inspection(
    transactions: &[Transaction],
    filter: &InspectFilter,
) -> InspectionResult {
    let matching: Vec<&Transaction> = transactions
        .iter()
        .filter(|t| matches_filter(t, filter))
        .collect();

    let line_items: Vec<InspectLineItem> = matching
        .iter()
        .map(|t| InspectLineItem {
            id: t.id.clone(),
            merchant: t.merchant_name.clone(),
            amount: t.amount,
            date: t.date.clone(),
            category: t.category.name.clone(),
            tags: t.tags.clone(),
            notes: t.notes.clone(),
        })
        .collect();

    let summary = compute_summary(&line_items);

    InspectionResult {
        summary,
        transactions: line_items,
    }
}

fn matches_filter(txn: &Transaction, filter: &InspectFilter) -> bool {
    // An empty or whitespace-only needle is treated as "no constraint" —
    // `"".contains("")` is always true, which would silently match every
    // transaction and present a whole-account dump as a category inspection.
    if let Some(category_needle) = &filter.category {
        let needle = category_needle.trim();
        if !needle.is_empty()
            && !txn
                .category
                .name
                .to_lowercase()
                .contains(&needle.to_lowercase())
        {
            return false;
        }
    }
    if let Some(merchant_needle) = &filter.merchant {
        let needle = merchant_needle.trim();
        if !needle.is_empty()
            && !txn
                .merchant_name
                .to_lowercase()
                .contains(&needle.to_lowercase())
        {
            return false;
        }
    }
    true
}

fn compute_summary(items: &[InspectLineItem]) -> InspectSummary {
    let total_count = items.len();
    let mut total_inflow = 0.0_f64;
    let mut total_outflow = 0.0_f64;

    for item in items {
        if item.amount > 0.0 {
            total_inflow += item.amount;
        } else {
            total_outflow += item.amount.abs();
        }
    }

    let net_amount = total_inflow - total_outflow;

    InspectSummary {
        total_count,
        net_amount,
        total_inflow,
        total_outflow,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::blank_to_none;

    // -----------------------------------------------------------------------
    // 9a RED: blank_to_none helper coerces empty/whitespace to None
    // -----------------------------------------------------------------------

    #[test]
    fn blank_to_none_coerces_empty_string_to_none() {
        assert_eq!(blank_to_none(Some(String::new())), None);
    }

    #[test]
    fn blank_to_none_coerces_whitespace_only_to_none() {
        assert_eq!(blank_to_none(Some("  ".to_string())), None);
    }

    #[test]
    fn blank_to_none_preserves_non_blank_value() {
        assert_eq!(
            blank_to_none(Some("Pets".to_string())),
            Some("Pets".to_string())
        );
    }

    use super::*;
    use crate::client::{Category, Transaction};

    fn make_txn(id: &str, merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: id.to_string(),
            amount,
            date: date.to_string(),
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

    fn make_txn_with_tags(
        id: &str,
        merchant: &str,
        amount: f64,
        category: &str,
        date: &str,
        tags: Vec<&str>,
        notes: &str,
    ) -> Transaction {
        Transaction {
            id: id.to_string(),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("expense".into()),
            },
            tags: tags.into_iter().map(str::to_string).collect(),
            notes: notes.to_string(),
            needs_review: false,
        }
    }

    fn no_filter() -> InspectFilter {
        InspectFilter::default()
    }

    // -----------------------------------------------------------------------
    // 9a RED: result includes transaction ids
    // -----------------------------------------------------------------------

    #[test]
    fn result_includes_transaction_ids() {
        let txns = vec![
            make_txn("txn-001", "Petco", -12000.0, "Pets", "2026-05-10"),
            make_txn("txn-002", "CVS Pharmacy", -3300.0, "Medical", "2026-05-12"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        let ids: Vec<&str> = result.transactions.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"txn-001"), "expected txn-001 in ids: {ids:?}");
        assert!(ids.contains(&"txn-002"), "expected txn-002 in ids: {ids:?}");
    }

    // -----------------------------------------------------------------------
    // 9b GREEN + 9c TRIANGULATE: correct count and net amount
    // -----------------------------------------------------------------------

    #[test]
    fn summary_count_matches_transaction_count() {
        let txns = vec![
            make_txn("t1", "Petco", -12000.0, "Pets", "2026-05-10"),
            make_txn("t2", "Banfield", -500.0, "Pets", "2026-05-11"),
            make_txn("t3", "Petco Refund", 200.0, "Pets", "2026-05-12"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        assert_eq!(result.summary.total_count, 3);
    }

    #[test]
    fn net_amount_is_algebraic_sum_of_all_amounts() {
        let txns = vec![
            make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "Refund", 30.0, "Pets", "2026-05-11"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        // net = 30 - 100 = -70
        assert!(
            (result.summary.net_amount - (-70.0)).abs() < 0.01,
            "expected net=-70.0, got {}",
            result.summary.net_amount
        );
    }

    // -----------------------------------------------------------------------
    // 9a RED: inflow vs outflow split
    // -----------------------------------------------------------------------

    #[test]
    fn inflow_outflow_split_separates_charges_from_refunds() {
        let txns = vec![
            make_txn("t1", "Petco", -12000.0, "Pets", "2026-05-10"),
            make_txn("t2", "Petco Refund", 500.0, "Pets", "2026-05-15"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        assert!(
            (result.summary.total_inflow - 500.0).abs() < 0.01,
            "expected inflow=500, got {}",
            result.summary.total_inflow
        );
        assert!(
            (result.summary.total_outflow - 12000.0).abs() < 0.01,
            "expected outflow=12000, got {}",
            result.summary.total_outflow
        );
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: all charges and no refunds — inflow is zero
    // -----------------------------------------------------------------------

    #[test]
    fn only_outflows_leaves_inflow_zero() {
        let txns = vec![
            make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "Vet Clinic", -200.0, "Pets", "2026-05-11"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        assert_eq!(result.summary.total_inflow, 0.0);
        assert!((result.summary.total_outflow - 300.0).abs() < 0.01);
    }

    #[test]
    fn only_inflows_leaves_outflow_zero() {
        let txns = vec![make_txn(
            "t1",
            "Direct Deposit",
            5000.0,
            "Income",
            "2026-05-01",
        )];
        let result = compute_inspection(&txns, &no_filter());
        assert_eq!(result.summary.total_outflow, 0.0);
        assert!((result.summary.total_inflow - 5000.0).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // 9a RED: category filter
    // -----------------------------------------------------------------------

    #[test]
    fn category_filter_returns_only_matching_transactions() {
        let txns = vec![
            make_txn("t1", "Petco", -12000.0, "Pets", "2026-05-10"),
            make_txn("t2", "CVS", -3300.0, "Medical", "2026-05-12"),
            make_txn("t3", "Banfield", -500.0, "Pets", "2026-05-14"),
        ];
        let filter = InspectFilter {
            category: Some("Pets".to_string()),
            merchant: None,
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 2);
        let categories: Vec<&str> = result
            .transactions
            .iter()
            .map(|t| t.category.as_str())
            .collect();
        assert!(
            categories.iter().all(|c| *c == "Pets"),
            "expected only Pets, got: {categories:?}"
        );
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: category filter is case-insensitive
    // -----------------------------------------------------------------------

    #[test]
    fn category_filter_is_case_insensitive() {
        let txns = vec![
            make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "CVS", -50.0, "Medical", "2026-05-11"),
        ];
        let filter = InspectFilter {
            category: Some("pets".to_string()),
            merchant: None,
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 1);
        assert_eq!(result.transactions[0].id, "t1");
    }

    // -----------------------------------------------------------------------
    // 9a RED: merchant filter
    // -----------------------------------------------------------------------

    #[test]
    fn merchant_filter_returns_only_matching_transactions() {
        let txns = vec![
            make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "Petco", -200.0, "Pets", "2026-05-11"),
            make_txn("t3", "Banfield Vet", -500.0, "Pets", "2026-05-12"),
        ];
        let filter = InspectFilter {
            category: None,
            merchant: Some("Petco".to_string()),
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 2);
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: merchant filter is case-insensitive substring match
    // -----------------------------------------------------------------------

    #[test]
    fn merchant_filter_is_case_insensitive_substring() {
        let txns = vec![
            make_txn("t1", "Petco Store #42", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "CVS Pharmacy", -50.0, "Medical", "2026-05-11"),
        ];
        let filter = InspectFilter {
            category: None,
            merchant: Some("petco".to_string()),
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 1);
        assert_eq!(result.transactions[0].id, "t1");
    }

    // -----------------------------------------------------------------------
    // 9a RED: combined category + merchant filter
    // -----------------------------------------------------------------------

    #[test]
    fn combined_category_and_merchant_filter_both_must_match() {
        let txns = vec![
            make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10"),
            make_txn("t2", "Petco", -50.0, "General", "2026-05-11"),
            make_txn("t3", "Banfield", -200.0, "Pets", "2026-05-12"),
        ];
        let filter = InspectFilter {
            category: Some("Pets".to_string()),
            merchant: Some("Petco".to_string()),
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 1);
        assert_eq!(result.transactions[0].id, "t1");
    }

    // -----------------------------------------------------------------------
    // 9a RED: empty result when nothing matches
    // -----------------------------------------------------------------------

    #[test]
    fn no_matches_returns_empty_result_with_zero_summary() {
        let txns = vec![make_txn("t1", "Petco", -100.0, "Pets", "2026-05-10")];
        let filter = InspectFilter {
            category: Some("Medical".to_string()),
            merchant: None,
        };
        let result = compute_inspection(&txns, &filter);
        assert_eq!(result.summary.total_count, 0);
        assert_eq!(result.summary.net_amount, 0.0);
        assert_eq!(result.summary.total_inflow, 0.0);
        assert_eq!(result.summary.total_outflow, 0.0);
        assert!(result.transactions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 9a RED: line items carry all fields including tags and notes
    // -----------------------------------------------------------------------

    #[test]
    fn line_items_carry_tags_and_notes() {
        let txns = vec![make_txn_with_tags(
            "t1",
            "Petco",
            -100.0,
            "Pets",
            "2026-05-10",
            vec!["urgent", "reimbursable"],
            "Heartworm meds",
        )];
        let result = compute_inspection(&txns, &no_filter());
        let item = &result.transactions[0];
        assert_eq!(item.tags, vec!["urgent", "reimbursable"]);
        assert_eq!(item.notes, "Heartworm meds");
    }

    // -----------------------------------------------------------------------
    // 9a RED: empty transaction list
    // -----------------------------------------------------------------------

    #[test]
    fn empty_transaction_list_returns_zero_summary() {
        let result = compute_inspection(&[], &no_filter());
        assert_eq!(result.summary.total_count, 0);
        assert_eq!(result.summary.net_amount, 0.0);
        assert_eq!(result.summary.total_inflow, 0.0);
        assert_eq!(result.summary.total_outflow, 0.0);
        assert!(result.transactions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 9c TRIANGULATE: inflow/outflow with zero-amount transactions
    // -----------------------------------------------------------------------

    #[test]
    fn zero_amount_transaction_contributes_to_neither_inflow_nor_outflow() {
        let txns = vec![
            make_txn("t1", "Adjustment", 0.0, "General", "2026-05-10"),
            make_txn("t2", "Petco", -100.0, "Pets", "2026-05-11"),
        ];
        let result = compute_inspection(&txns, &no_filter());
        assert_eq!(result.summary.total_inflow, 0.0);
        assert!((result.summary.total_outflow - 100.0).abs() < 0.01);
    }
}
