# ADR 0006 — Reckon month boundaries in the host's local timezone

**Status:** Accepted  
**Issues:** #34 (surfaced), #36 (fix)

## Context

All time-based tools (`financial_overview`, `spending_report`, `triage_uncategorized`,
`cashflow_forecast`, `progress_vs_goals`, `recurring_scan`, `inspect_transactions`) compute
"this month" and "prior month" by calling `today_epoch_day()`, which feeds
`current_month_range_for_day` and `prior_month_range_for_day`.

Before this ADR, `today_epoch_day()` fell back to:

```rust
(std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs()
    / 86_400) as i64
```

This divides UTC seconds by 86 400 — equivalent to flooring at midnight UTC. For a user in
UTC−07:00, any time between 00:00 and 01:00 UTC (17:00–18:00 local the evening before) is
still in yesterday's UTC date. At month boundaries this mislabels "this month": on 2026-05-31
at 18:00 local (01:00 UTC June 1) the server computed June as the current month, while the
user's browser and statement both showed May.

Issue #34 surfaced this bug during the first live budget session.

## Decision

`today_epoch_day()` uses `chrono::Local` to obtain the current date in the host's local
timezone, then converts it to an epoch day via the existing `civil_to_epoch_day` helper:

```rust
use chrono::{Datelike, Local};
let today = Local::now().date_naive();
civil_to_epoch_day(today.year() as i64, today.month() as i64, today.day() as i64)
```

The `MONARCH_NOW` test seam (an ISO `YYYY-MM-DD` environment variable) remains unchanged and
takes precedence over the local clock, preserving full hermeticity in tests.

## Rationale

- **Matches user perception.** The advisor runs locally on the user's machine. "This month"
  must mean the same thing to the advisor as it does to the user's calendar and bank statement.
- **Near-zero added cost.** `chrono 0.4` (with the `clock` feature) was already a transitive
  dependency via `rmcp` and `schemars`. Making it a direct `[dependencies]` entry with
  `default-features = false, features = ["clock"]` adds no binary size and no new crate
  version — `cargo tree -i chrono` confirms a single 0.4.x version feature-unified across all
  consumers.
- **Narrow scope.** `chrono` is used solely to acquire the current local date as a
  `(year, month, day)` triple. All month/range arithmetic remains hand-rolled
  (`civil_to_epoch_day`, `epoch_days_to_ymd`, `days_in_month`, `current_month_range_for_day`,
  `prior_month_range_for_day`, `months_ago_start_for_day`) so the logic is fully testable
  without a real clock.

## Scope of change

Only `today_epoch_day()` in `src/tools.rs` changed behavior. The pure `*_for_day` functions,
all consumers, and the mutation allowlist are untouched.

`civil_to_epoch_day(year, month, day) -> i64` was extracted from the body of
`parse_iso_date_to_epoch_day` as a shared, pure helper. Both the ISO-string path and the
`chrono::Local` path now call it, so the Hinnant formula lives in exactly one place.

## Consequences and caveats

- **"Local" means the host's OS timezone**, which is correct for a locally-run CLI advisor.
- **Revisit if ever hosted remotely.** A server hosted in UTC would compute its own local date
  (UTC), not the household's. In that scenario, read the user's Monarch profile timezone
  instead.
- **`MONARCH_NOW` override pins to a tz-agnostic civil date.** Tests that set `MONARCH_NOW`
  are unaffected by this change — the override is parsed before `Local::now()` is ever called.
