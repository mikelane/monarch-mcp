//! Adversarial QA — issue #66 (budget_review pace taxonomy).
//!
//! Gate 3 penetration-test failing tests. These document a CONFIRMED bug in
//! `compute_budget_review`'s pace classification and are `#[ignore]`d so they
//! do not break the green suite. Remove `#[ignore]` (and fix `src/budget_review.rs`)
//! to turn them green.
//!
//! Run all adversarial probes:
//!   cargo test --test adversarial_qa_issue66 -- --ignored
//!
//! Run one:
//!   cargo test --test adversarial_qa_issue66 \
//!     rounded_percent_understates_over_pace_realistic_dollars -- --ignored
//!
//! ---------------------------------------------------------------------------
//! BUG (confirmed, reachable with whole-dollar budgets/spends):
//!
//! `classify_pace` compares the *rounded* `percent_spent` (an `i64`) against the
//! tolerance band, but ADR 0013 defines the band on the *raw* ratio
//! `spent / budget * 100`. Rounding shifts a value across the band edge, so a
//! category that is genuinely over-pace per the ADR is reported `on_track`
//! (and vice-versa). This defeats the tool's entire purpose: surfacing
//! categories "tracking hot" mid-month.
//!
//! Canonical case: budget $250, spent $151, day 15 of a 30-day month.
//!   pace        = 15/30 * 100            = 50.0%
//!   upper edge  = 50 + 10 (tolerance)    = 60.0%
//!   raw %spent  = 151/250 * 100          = 60.4%   → 60.4 > 60.0 → `over` (ADR)
//!   round %spent= round(60.4)            = 60      → 60 !> 60.0 → `on_track` (code)
//! ---------------------------------------------------------------------------

use monarch_mcp::budget_review::{compute_budget_review, PaceStatus};
use monarch_mcp::client::{Budget, Category, Transaction};

fn expense_budget(name: &str, amount: f64) -> Budget {
    Budget {
        category: Category {
            name: name.to_string(),
            group_type: Some("expense".into()),
        },
        amount,
    }
}

fn expense_txn(name: &str, amount: f64, category: &str) -> Transaction {
    Transaction {
        id: format!("{name}-{amount}"),
        amount,
        date: "2026-06-15".to_string(),
        merchant_name: name.to_string(),
        category: Category {
            name: category.to_string(),
            group_type: Some("expense".into()),
        },
        tags: vec![],
        notes: String::new(),
        needs_review: false,
    }
}

/// BUG: a category that is over-pace per ADR 0013 (raw 60.4% > 60.0% upper edge)
/// is misclassified `on_track` because the code rounds 60.4% down to 60 before
/// the band comparison. Reachable with ordinary whole-dollar amounts.
#[test]
fn rounded_percent_understates_over_pace_realistic_dollars() {
    let budgets = vec![expense_budget("Restaurants", -250.0)];
    let txns = vec![expense_txn("Diner", -151.0, "Restaurants")];
    // Day 15 of 30: pace 50%, upper tolerance edge 60%. 151/250 = 60.4% > 60%.
    let result = compute_budget_review(&budgets, &txns, 15, 30);
    let cat = result.by_category.get("Restaurants").unwrap();

    // ADR 0013 row 3: percent_spent (60.4) > pace+TOLERANCE (60) AND spent < budget → `over`.
    assert_eq!(
        cat.pace_status,
        PaceStatus::Over,
        "151/250 = 60.4% on day 15/30 is over-pace per ADR 0013 (60.4 > 60.0), \
         but the code rounds to 60 and reports {:?}",
        cat.pace_status
    );
}

/// BUG (mirror image): a category that is *on-track* per ADR (raw 38.4% within the
/// [38.39%, 58.39%] band on day 15/31) is misclassified `under` because the code
/// rounds 38.4% down to 38, which falls below the lower edge 38.39%.
#[test]
fn rounded_percent_overstates_under_pace_31_day_month() {
    let budgets = vec![expense_budget("Groceries", -250.0)];
    let txns = vec![expense_txn("Market", -96.0, "Groceries")];
    // Day 15 of 31: pace = 48.387%, lower edge = 38.387%. 96/250 = 38.4% > 38.387% → on_track.
    let result = compute_budget_review(&budgets, &txns, 15, 31);
    let cat = result.by_category.get("Groceries").unwrap();

    assert_eq!(
        cat.pace_status,
        PaceStatus::OnTrack,
        "96/250 = 38.4% on day 15/31 is within the [38.39%, 58.39%] band per ADR 0013, \
         but the code rounds to 38 and reports {:?}",
        cat.pace_status
    );
}

/// FIXED: a category at 99.6% spent (spent < budget, so NOT over_budget) rounds
/// its displayed `percent_spent` up to 100, while the pace classification uses
/// the raw ratio. 99.6% > 60% upper edge on day 15/30 → Over. The fix correctly
/// separates display rounding from pace classification, producing the expected
/// result: `percent_spent: 100` (rounded) with `pace_status: Over` (raw ratio).
#[test]
fn percent_spent_100_with_over_pace_is_correct() {
    let budgets = vec![expense_budget("Phone", -100.0)];
    let txns = vec![expense_txn("Carrier", -99.6, "Phone")];
    // Day 15 of 30. spent 99.6 < budget 100 → NOT over_budget.
    // 99.6% > 60% upper edge → Over (using raw ratio, not rounded).
    let result = compute_budget_review(&budgets, &txns, 15, 30);
    let cat = result.by_category.get("Phone").unwrap();

    // Displayed percent rounds up to 100.
    assert_eq!(cat.percent_spent, Some(100), "{:?}", cat);
    // But pace classification uses raw ratio: 99.6% > 60% → Over (not OverBudget).
    assert_eq!(
        cat.pace_status,
        PaceStatus::Over,
        "raw ratio 99.6% > 60% upper edge on day 15/30 → Over. \
         Display rounding is independent of pace classification."
    );
}
