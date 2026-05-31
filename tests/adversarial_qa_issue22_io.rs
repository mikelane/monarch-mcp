//! Adversarial QA (Gate 3) — I/O edge cases for Goals::load_from_path.
//!
//! Probes how load_from_path classifies non-trivial filesystem states:
//! directory-as-path, dangling symlink, missing-parent path. These document
//! the boundary behavior the change introduces (NotFound => default, everything
//! else => GoalsFile error). Tests assert the CURRENTLY-OBSERVED contract; a
//! failure here would mean a regression in that contract.

use monarch_mcp::error::MonarchError;
use monarch_mcp::goals::Goals;
use std::path::Path;

// A path pointing at a DIRECTORY is a misconfiguration. read_to_string returns
// ErrorKind::IsADirectory (not NotFound), so it must surface as a GoalsFile
// error — NOT be swallowed as "no goals configured".
#[test]
fn directory_path_surfaces_as_goals_file_error() {
    let dir = std::env::temp_dir();
    let result = Goals::load_from_path(&dir);
    assert!(
        matches!(result, Err(MonarchError::GoalsFile(_))),
        "a directory path should surface as a GoalsFile error, got: {result:?}"
    );
}

// A DANGLING symlink (target deleted) reports NotFound, so the change treats it
// as "no goals configured" and returns the default. This documents that a
// broken symlink is indistinguishable from a missing file by design.
#[test]
fn dangling_symlink_is_treated_as_missing_file() {
    let base = std::env::temp_dir();
    let link = base.join(format!("aqa22_dangling_{}", std::process::id()));
    let target = base.join(format!("aqa22_no_such_target_{}", std::process::id()));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = Goals::load_from_path(&link);
    let _ = std::fs::remove_file(&link);

    assert_eq!(
        result.unwrap(),
        Goals::default(),
        "a dangling symlink reports NotFound and is treated as no goals configured"
    );
}

// A path whose PARENT directory does not exist also reports NotFound => default.
#[test]
fn missing_parent_dir_is_treated_as_missing_file() {
    let p = Path::new("/aqa22_nonexistent_parent_dir/goals.toml");
    let result = Goals::load_from_path(p);
    assert_eq!(result.unwrap(), Goals::default());
}
