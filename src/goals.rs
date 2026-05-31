//! Goals store — reads and parses the household's TOML goals file.
#![allow(dead_code)] // Public API consumed by progress_vs_goals tool (A7)
//!
//! The file path comes from the `MONARCH_GOALS_FILE` environment variable.
//! Missing goals are simply absent (not errors). A missing file or an empty
//! file yields an empty `Goals` struct with all fields `None`.
//!
//! # TOML format
//!
//! Goals are stored as nested tables so each goal type groups its parameters:
//!
//! ```toml
//! [savings_rate]
//! target_percent = 20.0
//!
//! [emergency_fund]
//! target_months = 6.0
//!
//! [debt_payoff]
//! target_date = "2027-12-01"
//! monthly_payment = 500.0   # optional
//! ```

use crate::error::MonarchError;
use serde::Deserialize;
use std::path::Path;

/// All household goals. Every field is optional — a goal that hasn't been set
/// is simply absent and will not be reported by `progress_vs_goals`.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Goals {
    /// Target savings rate. Present only when the household has set one.
    pub savings_rate: Option<SavingsRateGoal>,

    /// Target emergency-fund runway. Present only when the household has set one.
    pub emergency_fund: Option<EmergencyFundGoal>,

    /// Optional debt-payoff goal. Present only when the household has set one.
    pub debt_payoff: Option<DebtPayoffGoal>,
}

/// A savings-rate goal expressed as a percentage (0–100).
#[derive(Debug, Deserialize, PartialEq)]
pub struct SavingsRateGoal {
    /// Target savings rate as a percentage. E.g. `20.0` means 20 %.
    pub target_percent: f64,
}

/// An emergency-fund runway goal expressed in months of expenses.
#[derive(Debug, Deserialize, PartialEq)]
pub struct EmergencyFundGoal {
    /// Target number of months of expenses to hold in reserve.
    pub target_months: f64,
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
        let contents = std::fs::read_to_string(path)
            .map_err(|e| MonarchError::GoalsFile(format!("cannot read {}: {e}", path.display())))?;

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

    // --- RED: savings_rate goal is parsed from nested table ---

    #[test]
    fn savings_rate_goal_is_parsed() {
        let toml = "[savings_rate]\ntarget_percent = 20.0\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.savings_rate.unwrap().target_percent, 20.0);
        assert!(goals.emergency_fund.is_none());
        assert!(goals.debt_payoff.is_none());
    }

    // --- TRIANGULATE: emergency_fund goal is parsed from nested table ---

    #[test]
    fn emergency_fund_goal_is_parsed() {
        let toml = "[emergency_fund]\ntarget_months = 6.0\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.emergency_fund.unwrap().target_months, 6.0);
        assert!(goals.savings_rate.is_none());
    }

    // --- TRIANGULATE: both goals together ---

    #[test]
    fn both_numeric_goals_are_parsed_together() {
        let toml =
            "[savings_rate]\ntarget_percent = 15.0\n\n[emergency_fund]\ntarget_months = 3.0\n";
        let f = write_goals_file(toml);
        let goals = Goals::load_from_path(f.path()).unwrap();
        assert_eq!(goals.savings_rate.unwrap().target_percent, 15.0);
        assert_eq!(goals.emergency_fund.unwrap().target_months, 3.0);
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
        let toml = "[savings_rate]\ntarget_percent = 10.0\n";
        let f = write_goals_file(toml);
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

    // --- RED: missing file returns default Goals (not an error) ---

    #[test]
    fn missing_goals_file_returns_default() {
        let goals = Goals::load_from_path(Path::new("/nonexistent/goals.toml")).unwrap();
        assert_eq!(goals, Goals::default());
    }

    // --- RED: permission-denied still returns a GoalsFile error ---

    #[test]
    fn permission_denied_goals_file_returns_error() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        // Skip this test when running as root — root bypasses file permissions
        let uid_output = Command::new("id").arg("-u").output().unwrap();
        let uid_str = String::from_utf8_lossy(&uid_output.stdout);
        if uid_str.trim() == "0" {
            return;
        }

        let f = write_goals_file("[savings_rate]\ntarget_percent = 20.0\n");
        let path = f.path().to_path_buf();
        // Seal the file so even the owner cannot read it
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let err = Goals::load_from_path(&path).unwrap_err();
        assert!(
            matches!(err, MonarchError::GoalsFile(_)),
            "expected GoalsFile error for permission-denied, got: {err:?}"
        );
    }

    // --- load_from_env: unset returns default ---

    #[test]
    fn unset_env_var_yields_default_goals() {
        temp_env::with_var_unset("MONARCH_GOALS_FILE", || {
            let goals = Goals::load_from_env().unwrap();
            assert_eq!(goals, Goals::default());
        });
    }
}
