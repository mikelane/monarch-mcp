//! Goals store — reads and parses the household's TOML goals file.
#![allow(dead_code)] // Public API consumed by progress_vs_goals tool (A7)
//!
//! The file path comes from the `MONARCH_GOALS_FILE` environment variable.
//! Missing goals are simply absent (not errors). A missing file or an empty
//! file yields an empty `Goals` struct with all fields `None`.

use crate::error::MonarchError;
use serde::Deserialize;
use std::path::Path;

/// All household goals. Every field is optional — a goal that hasn't been set
/// is simply absent and will not be reported by `progress_vs_goals`.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Goals {
    /// Target savings rate as a percentage (0–100). E.g. `20.0` means 20 %.
    pub savings_rate_pct: Option<f64>,

    /// Target emergency-fund runway in months of expenses.
    pub emergency_fund_months: Option<f64>,

    /// Optional debt-payoff goal. Present only when the household has set one.
    pub debt_payoff: Option<DebtPayoffGoal>,
}

/// A debt-payoff goal with a target payoff date and optional monthly payment.
#[derive(Debug, Deserialize, PartialEq)]
pub struct DebtPayoffGoal {
    /// ISO-8601 target date, e.g. `"2027-12-01"`.
    pub target_date: String,
    /// Optional minimum monthly payment amount in dollars.
    pub monthly_payment: Option<f64>,
}

impl Goals {
    /// Load goals from the file at `path`. Returns `Ok(Goals::default())` when
    /// the file is empty. Returns `Err(MonarchError::GoalsFile)` on I/O or
    /// parse failures.
    pub fn load_from_path(path: &Path) -> Result<Self, MonarchError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            MonarchError::GoalsFile(format!("cannot read {}: {e}", path.display()))
        })?;

        if contents.trim().is_empty() {
            return Ok(Goals::default());
        }

        toml::from_str::<Goals>(&contents)
            .map_err(|e| MonarchError::GoalsFile(format!("TOML parse error: {e}")))
    }

    /// Load goals from `MONARCH_GOALS_FILE`. Returns `Ok(Goals::default())`
    /// when the env var is unset.
    pub fn load_from_env() -> Result<Self, MonarchError> {
        match std::env::var("MONARCH_GOALS_FILE") {
            Ok(path) if !path.is_empty() => Self::load_from_path(Path::new(&path)),
            _ => Ok(Goals::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_goals_file(contents: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{contents}").unwrap();
        f
    }

    // --- RED: empty file yields default Goals ---

    #[test]
    fn empty_file_yields_default_goals() {
        let f = write_goals_file("");
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals, Goals::default());
    }

    // --- TRIANGULATE: whitespace-only is also treated as empty ---

    #[test]
    fn whitespace_only_file_yields_default_goals() {
        let f = write_goals_file("   \n\t  ");
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals, Goals::default());
    }

    // --- RED: savings_rate_pct is parsed ---

    #[test]
    fn savings_rate_goal_is_parsed() {
        let f = write_goals_file("savings_rate_pct = 20.0\n");
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.savings_rate_pct, Some(20.0));
        assert_eq!(goals.emergency_fund_months, None);
        assert_eq!(goals.debt_payoff, None);
    }

    // --- TRIANGULATE: emergency_fund_months ---

    #[test]
    fn emergency_fund_goal_is_parsed() {
        let f = write_goals_file("emergency_fund_months = 6.0\n");
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.emergency_fund_months, Some(6.0));
        assert_eq!(goals.savings_rate_pct, None);
    }

    // --- TRIANGULATE: both goals together ---

    #[test]
    fn both_numeric_goals_are_parsed_together() {
        let toml = "savings_rate_pct = 15.0\nemergency_fund_months = 3.0\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.savings_rate_pct, Some(15.0));
        assert_eq!(goals.emergency_fund_months, Some(3.0));
    }

    // --- RED: debt_payoff goal ---

    #[test]
    fn debt_payoff_goal_is_parsed() {
        let toml = "[debt_payoff]\ntarget_date = \"2027-12-01\"\nmonthly_payment = 500.0\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        let dp = goals.debt_payoff.unwrap();
        assert_eq!(dp.target_date, "2027-12-01");
        assert_eq!(dp.monthly_payment, Some(500.0));
    }

    // --- TRIANGULATE: debt_payoff without monthly_payment ---

    #[test]
    fn debt_payoff_without_payment_is_parsed() {
        let toml = "[debt_payoff]\ntarget_date = \"2028-06-01\"\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        let dp = goals.debt_payoff.unwrap();
        assert_eq!(dp.target_date, "2028-06-01");
        assert_eq!(dp.monthly_payment, None);
    }

    // --- RED: absent debt_payoff means None ---

    #[test]
    fn absent_debt_payoff_is_none() {
        let f = write_goals_file("savings_rate_pct = 10.0\n");
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert!(goals.debt_payoff.is_none());
    }

    // --- RED: invalid TOML returns GoalsFile error ---

    #[test]
    fn invalid_toml_returns_goals_file_error() {
        let f = write_goals_file("this is not toml %%% !!!");
        let err = Goals::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, MonarchError::GoalsFile(_)),
            "expected GoalsFile error, got: {err:?}"
        );
    }

    // --- TRIANGULATE: missing file returns GoalsFile error ---

    #[test]
    fn missing_file_returns_goals_file_error() {
        let err = Goals::load_from_path(Path::new("/nonexistent/goals.toml")).unwrap_err();
        assert!(matches!(err, MonarchError::GoalsFile(_)));
    }

    // --- load_from_env: unset returns default ---

    #[test]
    fn unset_env_var_yields_default_goals() {
        // Temporarily remove the env var (if set by the test harness)
        let old = std::env::var("MONARCH_GOALS_FILE").ok();
        unsafe { std::env::remove_var("MONARCH_GOALS_FILE") };
        let goals = Goals::load_from_env().unwrap();
        assert_eq!(goals, Goals::default());
        if let Some(v) = old {
            unsafe { std::env::set_var("MONARCH_GOALS_FILE", v) };
        }
    }
}
