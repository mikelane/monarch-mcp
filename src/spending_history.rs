//! Pure aggregation logic for the `spending_history` tool.
//!
//! Computes per-month true spending across a multi-month range, broken down
//! by category and split into fixed vs. discretionary buckets. All arithmetic
//! lives here, separated from I/O, so it can be unit-tested without a mock
//! server. The tool handler in `tools.rs` fetches data and delegates here.
//!
//! ## Fixed vs. discretionary taxonomy (ADR 0011)
//!
//! FIXED categories represent non-negotiable recurring obligations:
//! mortgage/rent, insurance, utilities, auto/loan payments, and recurring
//! medical/dental. Everything else is DISCRETIONARY. The taxonomy is a
//! documented constant — see [`FIXED_CATEGORY_PATTERNS`].

use crate::client::Transaction;
use crate::spending_report::transaction_spend_magnitude;
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Fixed-category taxonomy (ADR 0011)
// ---------------------------------------------------------------------------

/// Whole-word token patterns that identify FIXED spending categories.
///
/// A category is FIXED when its name contains any of these patterns as a
/// complete word (case-insensitive). Tokenization splits on any non-alphanumeric
/// character (spaces, `&`, `/`, `-`, etc.) so that "Concert Rentals" does NOT
/// match `"rent"` (only the token "Rentals" is present, not the whole word
/// "rent"), but "Rent" and "Rental Income" with a bare "Rent" token do match.
///
/// Multi-word patterns (sequences of tokens) are matched by sliding over the
/// token list looking for the full sequence in order.
///
/// Taxonomy rationale (ADR 0011):
/// - `"mortgage"` / `"rent"` — housing costs, non-negotiable monthly obligations
/// - `"insurance"` — health, auto, home, life; fixed premium obligations
/// - `"utilities"` / `"utility"` — electricity, gas, water, internet; relatively stable bills
/// - `"loan"` — debt service payments on a fixed schedule
/// - `"medical"` / `"dental"` — recurring healthcare premiums and copay plans
pub const FIXED_CATEGORY_PATTERNS: &[&str] = &[
    "mortgage",
    "rent",
    "insurance",
    "utilities",
    "utility",
    "loan",
    "medical",
    "dental",
];

/// Tokenize a category name into lowercase words split on non-alphanumeric characters.
fn category_tokens(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Returns `true` when a category name contains a fixed-spending pattern as a
/// complete word token (whole-word match, case-insensitive).
///
/// Splits both the category name and each pattern on non-alphanumeric characters
/// and checks that the pattern's token sequence appears as a contiguous subsequence
/// of the category's tokens. This prevents false positives like "Concert Rentals"
/// matching `"rent"` or "Accidental Purchases" matching `"dental"`.
pub fn is_fixed_category(category_name: &str) -> bool {
    let name_tokens = category_tokens(category_name);
    FIXED_CATEGORY_PATTERNS.iter().any(|pattern| {
        let pattern_tokens = category_tokens(pattern);
        if pattern_tokens.is_empty() {
            return false;
        }
        // Slide over name_tokens looking for the full pattern token sequence
        name_tokens
            .windows(pattern_tokens.len())
            .any(|window| window == pattern_tokens.as_slice())
    })
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Fixed vs. discretionary spending split for one month.
#[derive(Debug, Serialize, PartialEq)]
pub struct FixedDiscretionarySplit {
    /// Sum of true spending in fixed-obligation categories.
    pub fixed: f64,
    /// Sum of true spending in all other (discretionary) categories.
    pub discretionary: f64,
}

/// A large one-off transaction that dwarfs its category's typical monthly spend.
#[derive(Debug, Serialize, PartialEq)]
pub struct SpendingOutlier {
    /// Category containing the outlier.
    pub category: String,
    /// Merchant name for the transaction.
    pub merchant: String,
    /// Positive magnitude of the transaction.
    pub amount: f64,
    /// Date of the transaction (ISO-8601 YYYY-MM-DD).
    pub date: String,
}

/// Per-month spending aggregate — one entry per complete calendar month.
#[derive(Debug, Serialize, PartialEq)]
pub struct MonthlySpend {
    /// Calendar month in `YYYY-MM` format.
    pub month: String,
    /// Total true spending for the month (expense magnitudes, income/transfer excluded).
    pub total_true_spending: f64,
    /// True spending broken down by category name.
    pub by_category: HashMap<String, f64>,
    /// Fixed vs. discretionary split.
    pub split: FixedDiscretionarySplit,
    /// Large one-off transactions that are outliers in their category.
    /// Empty when no single transaction exceeds the outlier threshold.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outliers: Vec<SpendingOutlier>,
}

/// The full spending history payload returned by the `spending_history` tool.
#[derive(Debug, Serialize, PartialEq)]
pub struct SpendingHistory {
    /// One entry per complete calendar month, oldest-first.
    pub months: Vec<MonthlySpend>,
    /// ISO-8601 start date of the range queried.
    pub range_start: String,
    /// ISO-8601 end date of the range queried (inclusive, last day of month).
    pub range_end: String,
}

// ---------------------------------------------------------------------------
// Range resolution helpers
// ---------------------------------------------------------------------------

/// Convert a Unix epoch day count to `(year, month, day)`.
///
/// Uses the same Howard Hinnant algorithm as `tools.rs` — kept in sync so
/// month boundaries are computed identically in both crates.
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

/// Convert an ISO `YYYY-MM-DD` date string to Unix epoch days.
///
/// Returns `None` for unparseable inputs; callers fall back gracefully.
#[cfg(test)]
fn parse_iso_to_epoch_day(s: &str) -> Option<i64> {
    let mut parts = s.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month as u32) as i64;
    if day < 1 || day > max_day {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let m = month as u32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Determine the YYYY-MM bucket for a transaction date string.
///
/// Returns `None` when the date is not a valid ISO-8601 date.
fn month_bucket(date: &str) -> Option<String> {
    if date.len() < 7 {
        return None;
    }
    // Fast path: just take the YYYY-MM prefix if it looks valid.
    let prefix = &date[..7];
    let dash = prefix.as_bytes().get(4).copied();
    if dash != Some(b'-') {
        return None;
    }
    Some(prefix.to_string())
}

/// Subtract `n` months from `(year, month)`, returning the new `(year, month)`.
fn subtract_months(year: i64, month: u32, n: u32) -> (i64, u32) {
    let mut y = year;
    let mut m = month;
    for _ in 0..n {
        if m == 1 {
            y -= 1;
            m = 12;
        } else {
            m -= 1;
        }
    }
    (y, m)
}

/// Compute the ISO date range covered by `months` complete months ending
/// just before the month containing `today_epoch_day`.
///
/// "Complete months" = exclude the current partial month.
/// `months` is clamped to a minimum of 1 so that zero (or any underflowing
/// value) never causes a u32 subtraction underflow inside the loop.
/// Returns `(start_date, end_date)` as ISO-8601 strings.
pub fn range_for_months_count(today_day: i64, months: u32) -> (String, String) {
    let months = months.max(1);
    let (today_year, today_month, _) = epoch_days_to_ymd(today_day);

    // Prior month is the most recent complete month
    let (prior_year, prior_month) = subtract_months(today_year, today_month, 1);
    // Start month = N-1 months before the prior month (so we get N complete months total)
    let (start_year, start_month) = subtract_months(prior_year, prior_month, months - 1);

    let start = format!("{start_year:04}-{start_month:02}-01");
    let end_last = days_in_month(prior_year, prior_month);
    let end = format!("{prior_year:04}-{prior_month:02}-{end_last:02}");
    (start, end)
}

// ---------------------------------------------------------------------------
// Outlier detection
// ---------------------------------------------------------------------------

/// A transaction whose magnitude is at least this many times the category's
/// average-per-transaction spend is surfaced as an outlier.
///
/// Rationale (ADR 0011): a factor of 3× above the per-transaction average
/// is a pragmatic threshold that catches genuine one-off spikes (e.g. an
/// annual insurance premium, a large medical bill) while avoiding false
/// positives on normal month-to-month variation. It is applied per-category
/// per-month, not across months, so seasonal categories are not penalised.
const OUTLIER_FACTOR: f64 = 3.0;

/// Surface large one-off transactions that dominate their category's spend.
///
/// For each category with more than one expense transaction, any transaction
/// whose magnitude is ≥ [`OUTLIER_FACTOR`] × the per-transaction average is
/// reported as an outlier. Single-transaction categories are skipped — when
/// there is only one data point there is no norm to dwarfs against.
fn find_outliers(transactions: &[&Transaction]) -> Vec<SpendingOutlier> {
    // Group expense transactions by category.
    let mut by_category: HashMap<&str, Vec<&Transaction>> = HashMap::new();
    for txn in transactions {
        let magnitude = transaction_spend_magnitude(txn);
        if magnitude > 0.0 {
            by_category
                .entry(txn.category.name.as_str())
                .or_default()
                .push(txn);
        }
    }

    let mut outliers = Vec::new();
    for (category, txns) in &by_category {
        if txns.len() < 2 {
            continue; // single transaction — no norm to compare against
        }

        for i in 0..txns.len() {
            let magnitude = transaction_spend_magnitude(txns[i]);
            // Average of all OTHER transactions in this category (exclude candidate)
            // so the outlier does not inflate its own threshold and escape detection.
            let others_total: f64 = txns
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, t)| transaction_spend_magnitude(t))
                .sum();
            let others_avg = others_total / (txns.len() - 1) as f64;
            if others_avg <= 0.0 {
                continue; // all peers are zero — no meaningful baseline
            }
            if magnitude >= others_avg * OUTLIER_FACTOR {
                outliers.push(SpendingOutlier {
                    category: category.to_string(),
                    merchant: txns[i].merchant_name.clone(),
                    amount: magnitude,
                    date: txns[i].date.clone(),
                });
            }
        }
    }

    // Sort for deterministic output: by category, then by date, then by amount desc
    outliers.sort_by(|a, b| {
        a.category.cmp(&b.category).then(a.date.cmp(&b.date)).then(
            b.amount
                .partial_cmp(&a.amount)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    outliers
}

// ---------------------------------------------------------------------------
// Core computation
// ---------------------------------------------------------------------------

/// Aggregate transactions into per-month spending summaries.
///
/// Transactions are bucketed by their `YYYY-MM` date prefix. Only complete
/// months within the `[range_start, range_end]` window are included — any
/// transaction outside that window is silently dropped (the caller is
/// responsible for fetching the correct date range from Monarch).
///
/// Within each month:
/// - True spending is computed via [`transaction_spend_magnitude`] (reused
///   from `spending_report`), which excludes income/transfer groups and
///   treats positive amounts in expense categories as zero-spend refunds.
/// - Categories are split into fixed vs. discretionary via
///   [`is_fixed_category`].
/// - Outliers are surfaced via [`find_outliers`].
///
/// Months are returned oldest-first.
pub fn compute_spending_history(
    transactions: &[Transaction],
    range_start: &str,
    range_end: &str,
) -> SpendingHistory {
    // Collect all YYYY-MM buckets in the range (sorted, oldest-first).
    let month_labels = enumerate_months_in_range(range_start, range_end);

    // Group transactions by their YYYY-MM bucket.
    let mut bucket_map: HashMap<String, Vec<&Transaction>> = HashMap::new();
    for txn in transactions {
        if let Some(bucket) = month_bucket(&txn.date) {
            if month_labels.contains(&bucket) {
                bucket_map.entry(bucket).or_default().push(txn);
            }
        }
    }

    let months = month_labels
        .into_iter()
        .map(|label| {
            let txns = bucket_map.get(&label).map(|v| v.as_slice()).unwrap_or(&[]);
            build_monthly_spend(label, txns)
        })
        .collect();

    SpendingHistory {
        months,
        range_start: range_start.to_string(),
        range_end: range_end.to_string(),
    }
}

/// Build a [`MonthlySpend`] for a single bucket of transactions.
fn build_monthly_spend(month: String, transactions: &[&Transaction]) -> MonthlySpend {
    let mut by_category: HashMap<String, f64> = HashMap::new();
    let mut fixed_total = 0.0_f64;
    let mut discretionary_total = 0.0_f64;

    for txn in transactions {
        let magnitude = transaction_spend_magnitude(txn);
        if magnitude == 0.0 && !matches!(txn.category.group_type.as_deref(), Some("expense") | None)
        {
            continue; // skip income/transfer entirely
        }
        if magnitude == 0.0 {
            continue; // refund or zero — no spend contribution
        }

        *by_category.entry(txn.category.name.clone()).or_insert(0.0) += magnitude;
        if is_fixed_category(&txn.category.name) {
            fixed_total += magnitude;
        } else {
            discretionary_total += magnitude;
        }
    }

    let total_true_spending: f64 = by_category.values().sum();
    let outliers = find_outliers(transactions);

    MonthlySpend {
        month,
        total_true_spending,
        by_category,
        split: FixedDiscretionarySplit {
            fixed: fixed_total,
            discretionary: discretionary_total,
        },
        outliers,
    }
}

/// Enumerate all `YYYY-MM` labels between `range_start` and `range_end`
/// (inclusive), oldest-first.
///
/// Both inputs must be ISO-8601 dates (YYYY-MM-DD). Months are generated by
/// walking forward one month at a time from `start_month` to `end_month`.
fn enumerate_months_in_range(range_start: &str, range_end: &str) -> Vec<String> {
    let start_prefix = if range_start.len() >= 7 {
        &range_start[..7]
    } else {
        return vec![];
    };
    let end_prefix = if range_end.len() >= 7 {
        &range_end[..7]
    } else {
        return vec![];
    };

    let parse_ym = |s: &str| -> Option<(i64, u32)> {
        let mut parts = s.splitn(2, '-');
        let y: i64 = parts.next()?.parse().ok()?;
        let m: u32 = parts.next()?.parse().ok()?;
        Some((y, m))
    };

    let (mut y, mut m) = match parse_ym(start_prefix) {
        Some(v) => v,
        None => return vec![],
    };
    let (end_y, end_m) = match parse_ym(end_prefix) {
        Some(v) => v,
        None => return vec![],
    };

    let mut labels = Vec::new();
    loop {
        if y > end_y || (y == end_y && m > end_m) {
            break;
        }
        labels.push(format!("{y:04}-{m:02}"));
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    labels
}

// ---------------------------------------------------------------------------
// Tests — TDD: RED first, then GREEN
//
// Fixtures use the REAL Monarch sign convention:
//   - expense outflows: NEGATIVE amounts
//   - income: POSITIVE amounts
//   - refunds in expense categories: POSITIVE amounts
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Category, Transaction};

    fn make_expense_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
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

    fn make_income_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("income".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    fn make_transfer_txn(merchant: &str, amount: f64, category: &str, date: &str) -> Transaction {
        Transaction {
            id: format!("{merchant}-{amount}-{date}"),
            amount,
            date: date.to_string(),
            merchant_name: merchant.to_string(),
            category: Category {
                name: category.to_string(),
                group_type: Some("transfer".into()),
            },
            tags: vec![],
            notes: String::new(),
            needs_review: false,
        }
    }

    // -----------------------------------------------------------------------
    // enumerate_months_in_range
    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_months_single_month() {
        let labels = enumerate_months_in_range("2026-03-01", "2026-03-31");
        assert_eq!(labels, vec!["2026-03"]);
    }

    #[test]
    fn enumerate_months_three_months() {
        let labels = enumerate_months_in_range("2026-01-01", "2026-03-31");
        assert_eq!(labels, vec!["2026-01", "2026-02", "2026-03"]);
    }

    #[test]
    fn enumerate_months_crosses_year_boundary() {
        let labels = enumerate_months_in_range("2025-11-01", "2026-02-28");
        assert_eq!(labels, vec!["2025-11", "2025-12", "2026-01", "2026-02"]);
    }

    // -----------------------------------------------------------------------
    // range_for_months_count
    // -----------------------------------------------------------------------

    #[test]
    fn range_for_months_count_default_6_excludes_current_month() {
        // today = 2026-05-15; 6 complete months = Nov 2025 through Apr 2026
        let today = parse_iso_to_epoch_day("2026-05-15").unwrap();
        let (start, end) = range_for_months_count(today, 6);
        assert_eq!(start, "2025-11-01");
        assert_eq!(end, "2026-04-30");
    }

    #[test]
    fn range_for_months_count_1_returns_only_prior_month() {
        // today = 2026-05-15; 1 complete month = Apr 2026
        let today = parse_iso_to_epoch_day("2026-05-15").unwrap();
        let (start, end) = range_for_months_count(today, 1);
        assert_eq!(start, "2026-04-01");
        assert_eq!(end, "2026-04-30");
    }

    #[test]
    fn range_for_months_count_crosses_year_when_today_is_january() {
        // today = 2026-01-10; 3 complete months = Oct, Nov, Dec 2025
        let today = parse_iso_to_epoch_day("2026-01-10").unwrap();
        let (start, end) = range_for_months_count(today, 3);
        assert_eq!(start, "2025-10-01");
        assert_eq!(end, "2025-12-31");
    }

    // -----------------------------------------------------------------------
    // compute_spending_history — month bucketing
    // -----------------------------------------------------------------------

    #[test]
    fn empty_transactions_produces_zero_spend_per_month() {
        let history = compute_spending_history(&[], "2026-03-01", "2026-04-30");
        assert_eq!(history.months.len(), 2);
        for m in &history.months {
            assert_eq!(m.total_true_spending, 0.0);
            assert!(m.by_category.is_empty());
        }
    }

    #[test]
    fn transactions_bucketed_into_correct_months() {
        let txns = vec![
            make_expense_txn("Grocer", -365.0, "Groceries", "2026-03-10"),
            make_expense_txn("Dining", -200.0, "Dining", "2026-04-15"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-04-30");
        assert_eq!(history.months.len(), 2);
        let march = history
            .months
            .iter()
            .find(|m| m.month == "2026-03")
            .unwrap();
        let april = history
            .months
            .iter()
            .find(|m| m.month == "2026-04")
            .unwrap();
        assert_eq!(march.total_true_spending, 365.0);
        assert_eq!(april.total_true_spending, 200.0);
    }

    #[test]
    fn income_transactions_excluded_from_all_months() {
        let txns = vec![
            make_income_txn("Employer", 5000.0, "Paychecks", "2026-03-01"),
            make_expense_txn("Grocer", -365.0, "Groceries", "2026-03-10"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert_eq!(history.months.len(), 1);
        assert_eq!(history.months[0].total_true_spending, 365.0);
        assert!(!history.months[0].by_category.contains_key("Paychecks"));
    }

    #[test]
    fn transfer_transactions_excluded_from_all_months() {
        let txns = vec![
            make_transfer_txn("Chase CC", -3000.0, "Credit Card Payment", "2026-03-05"),
            make_expense_txn("Grocer", -365.0, "Groceries", "2026-03-10"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert_eq!(history.months[0].total_true_spending, 365.0);
    }

    #[test]
    fn refund_in_expense_category_contributes_zero_to_month_spend() {
        let txns = vec![
            make_expense_txn("Insurer", 120.0, "Medical", "2026-03-15"), // refund
            make_expense_txn("Clinic", -80.0, "Medical", "2026-03-20"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        // refund (positive) → magnitude 0; charge (-80) → magnitude 80
        assert_eq!(history.months[0].total_true_spending, 80.0);
        assert_eq!(*history.months[0].by_category.get("Medical").unwrap(), 80.0);
    }

    // -----------------------------------------------------------------------
    // Fixed vs. discretionary split
    // -----------------------------------------------------------------------

    #[test]
    fn mortgage_payment_classified_as_fixed() {
        let txns = vec![make_expense_txn(
            "Lender",
            -2500.0,
            "Mortgage",
            "2026-03-01",
        )];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let split = &history.months[0].split;
        assert_eq!(split.fixed, 2500.0);
        assert_eq!(split.discretionary, 0.0);
    }

    #[test]
    fn dining_classified_as_discretionary() {
        let txns = vec![make_expense_txn(
            "Restaurant",
            -85.0,
            "Dining",
            "2026-03-15",
        )];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let split = &history.months[0].split;
        assert_eq!(split.fixed, 0.0);
        assert_eq!(split.discretionary, 85.0);
    }

    #[test]
    fn mixed_fixed_and_discretionary_split_correctly() {
        let txns = vec![
            make_expense_txn("Lender", -2500.0, "Mortgage", "2026-03-01"),
            make_expense_txn("Utils Co", -150.0, "Utilities", "2026-03-10"),
            make_expense_txn("Restaurant", -85.0, "Dining", "2026-03-15"),
            make_expense_txn("Amazon", -60.0, "Shopping", "2026-03-20"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let split = &history.months[0].split;
        assert_eq!(split.fixed, 2650.0); // mortgage + utilities
        assert_eq!(split.discretionary, 145.0); // dining + shopping
    }

    #[test]
    fn insurance_category_classified_as_fixed() {
        let txns = vec![make_expense_txn(
            "State Farm",
            -180.0,
            "Auto Insurance",
            "2026-03-01",
        )];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert_eq!(history.months[0].split.fixed, 180.0);
        assert_eq!(history.months[0].split.discretionary, 0.0);
    }

    #[test]
    fn loan_repayment_classified_as_fixed() {
        let txns = vec![make_expense_txn("Bank", -1280.0, "Auto Loan", "2026-03-01")];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert_eq!(history.months[0].split.fixed, 1280.0);
    }

    // -----------------------------------------------------------------------
    // by_category breakdown
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_transactions_in_same_category_are_summed() {
        let txns = vec![
            make_expense_txn("Whole Foods", -365.0, "Groceries", "2026-03-10"),
            make_expense_txn("Trader Joes", -220.0, "Groceries", "2026-03-22"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let cat = history.months[0].by_category.get("Groceries").unwrap();
        assert_eq!(*cat, 585.0);
    }

    #[test]
    fn by_category_sums_match_total_true_spending() {
        let txns = vec![
            make_expense_txn("Whole Foods", -365.0, "Groceries", "2026-03-10"),
            make_expense_txn("Restaurant", -85.0, "Dining", "2026-03-15"),
            make_expense_txn("Lender", -2500.0, "Mortgage", "2026-03-01"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let m = &history.months[0];
        let cat_sum: f64 = m.by_category.values().sum();
        assert!(
            (cat_sum - m.total_true_spending).abs() < 0.001,
            "by_category sum {cat_sum} != total_true_spending {}",
            m.total_true_spending
        );
    }

    // -----------------------------------------------------------------------
    // Outlier detection
    // -----------------------------------------------------------------------

    #[test]
    fn large_one_off_transaction_surfaced_as_outlier() {
        // Normal: 3 × ~$50 dining charges. Outlier: 1 × $300 dinner.
        let txns = vec![
            make_expense_txn("Casual Diner", -50.0, "Dining", "2026-03-05"),
            make_expense_txn("Fast Food", -45.0, "Dining", "2026-03-12"),
            make_expense_txn("Fancy Dinner", -300.0, "Dining", "2026-03-20"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let outliers = &history.months[0].outliers;
        assert!(
            outliers.iter().any(|o| o.merchant == "Fancy Dinner"),
            "Expected Fancy Dinner as outlier, got: {outliers:?}"
        );
    }

    #[test]
    fn single_transaction_category_not_flagged_as_outlier() {
        // Only one transaction — no norm to compare against.
        let txns = vec![make_expense_txn("Dentist", -1200.0, "Dental", "2026-03-15")];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert!(
            history.months[0].outliers.is_empty(),
            "Single-transaction category must not be flagged as outlier"
        );
    }

    #[test]
    fn similar_sized_transactions_not_flagged_as_outliers() {
        let txns = vec![
            make_expense_txn("Grocer A", -365.0, "Groceries", "2026-03-01"),
            make_expense_txn("Grocer B", -380.0, "Groceries", "2026-03-15"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        assert!(
            history.months[0].outliers.is_empty(),
            "Similar-sized transactions must not produce outliers"
        );
    }

    // -----------------------------------------------------------------------
    // Multi-month range / months ordering
    // -----------------------------------------------------------------------

    #[test]
    fn six_months_produces_six_monthly_entries_oldest_first() {
        let txns = vec![
            make_expense_txn("G1", -100.0, "Groceries", "2025-11-15"),
            make_expense_txn("G2", -110.0, "Groceries", "2025-12-15"),
            make_expense_txn("G3", -120.0, "Groceries", "2026-01-15"),
            make_expense_txn("G4", -130.0, "Groceries", "2026-02-15"),
            make_expense_txn("G5", -140.0, "Groceries", "2026-03-15"),
            make_expense_txn("G6", -150.0, "Groceries", "2026-04-15"),
        ];
        // today = 2026-05-15, 6 complete months = Nov 2025 – Apr 2026
        let today = parse_iso_to_epoch_day("2026-05-15").unwrap();
        let (start, end) = range_for_months_count(today, 6);
        let history = compute_spending_history(&txns, &start, &end);
        assert_eq!(history.months.len(), 6);
        assert_eq!(history.months[0].month, "2025-11");
        assert_eq!(history.months[5].month, "2026-04");
        // Each month should have the right spend
        assert_eq!(history.months[0].total_true_spending, 100.0);
        assert_eq!(history.months[5].total_true_spending, 150.0);
    }

    #[test]
    fn transactions_outside_range_are_excluded() {
        let txns = vec![
            make_expense_txn("Old", -500.0, "Groceries", "2026-01-15"), // before range
            make_expense_txn("InRange", -200.0, "Groceries", "2026-03-15"),
            make_expense_txn("Current", -300.0, "Groceries", "2026-05-15"), // after range
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-04-30");
        let total: f64 = history.months.iter().map(|m| m.total_true_spending).sum();
        assert_eq!(
            total, 200.0,
            "Only the in-range transaction should be counted"
        );
    }

    // -----------------------------------------------------------------------
    // is_fixed_category taxonomy
    // -----------------------------------------------------------------------

    #[test]
    fn is_fixed_category_matches_known_patterns() {
        assert!(is_fixed_category("Mortgage"));
        assert!(is_fixed_category("Home Mortgage"));
        assert!(is_fixed_category("Rent"));
        assert!(is_fixed_category("Auto Insurance"));
        assert!(is_fixed_category("Utilities"));
        assert!(is_fixed_category("Electric Utility"));
        assert!(is_fixed_category("Auto Loan"));
        assert!(is_fixed_category("Loan Repayment"));
        assert!(is_fixed_category("Medical Bills"));
        assert!(is_fixed_category("Dental Care"));
    }

    #[test]
    fn is_fixed_category_rejects_discretionary_categories() {
        assert!(!is_fixed_category("Dining"));
        assert!(!is_fixed_category("Shopping"));
        assert!(!is_fixed_category("Entertainment"));
        assert!(!is_fixed_category("Travel"));
        assert!(!is_fixed_category("Groceries"));
        assert!(!is_fixed_category("Subscriptions"));
    }

    #[test]
    fn is_fixed_category_is_case_insensitive() {
        assert!(is_fixed_category("MORTGAGE"));
        assert!(is_fixed_category("auto insurance"));
        assert!(is_fixed_category("UTILITIES"));
    }

    #[test]
    fn is_fixed_category_rejects_substring_false_positives() {
        // "rent" is a substring of these but not a whole word
        assert!(!is_fixed_category("Concert Rentals"));
        assert!(!is_fixed_category("Apparent Overspending"));
        assert!(!is_fixed_category("Current Subscriptions"));
        assert!(!is_fixed_category("Parent Gifts"));
        // "dental" is a substring of "Accidental" but not a whole word
        assert!(!is_fixed_category("Accidental Purchases"));
        // "insurance" is a substring of "Reinsurance" but not a whole word
        assert!(!is_fixed_category("Reinsurance Hobby"));
    }

    #[test]
    fn range_for_months_count_zero_clamps_to_one_month() {
        let today = parse_iso_to_epoch_day("2026-05-15").unwrap();
        // months=0 must not panic; it clamps to 1, returning the prior month
        let (start, end) = range_for_months_count(today, 0);
        assert_eq!(start, "2026-04-01");
        assert_eq!(end, "2026-04-30");
        assert!(start <= end);
    }

    // -----------------------------------------------------------------------
    // range_start / range_end preserved in output
    // -----------------------------------------------------------------------

    #[test]
    fn history_preserves_range_start_and_end() {
        let history = compute_spending_history(&[], "2026-03-01", "2026-04-30");
        assert_eq!(history.range_start, "2026-03-01");
        assert_eq!(history.range_end, "2026-04-30");
    }

    // -----------------------------------------------------------------------
    // fixed + discretionary sums equal total_true_spending
    // -----------------------------------------------------------------------

    #[test]
    fn fixed_plus_discretionary_equals_total_true_spending() {
        let txns = vec![
            make_expense_txn("Lender", -2500.0, "Mortgage", "2026-03-01"),
            make_expense_txn("Utils Co", -150.0, "Utilities", "2026-03-10"),
            make_expense_txn("Restaurant", -85.0, "Dining", "2026-03-15"),
            make_expense_txn("Amazon", -60.0, "Shopping", "2026-03-20"),
        ];
        let history = compute_spending_history(&txns, "2026-03-01", "2026-03-31");
        let m = &history.months[0];
        let split_total = m.split.fixed + m.split.discretionary;
        assert!(
            (split_total - m.total_true_spending).abs() < 0.001,
            "fixed ({}) + discretionary ({}) = {split_total} != total_true_spending {}",
            m.split.fixed,
            m.split.discretionary,
            m.total_true_spending
        );
    }
}
