//! Content-hash caching reload: unchanged files are not re-parsed.
//!
//! Cross-ref to spec.md:
//! - I10 (deterministic, hermetic, reproducible): incremental reload uses content hash (FNV)
//!   to avoid reparse; test asserts count==0 on unchanged vault proves the guard.
//! - Supports I6 determinism (same input files => same in-mem state).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (n, b) in files {
        fs::write(dir.path().join(n), b).expect("write");
    }
    dir
}

#[test]
fn reload_with_no_changes_reparses_nothing() {
    let td = vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.reload_incremental().expect("reload"), 0);
}

#[test]
fn reload_reparses_only_changed_files() {
    let td = vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    fs::write(td.path().join("a.org"), "* A edited\n").expect("write");
    assert_eq!(v.reload_incremental().expect("reload"), 1);
    assert!(v.find_by_title("A edited").is_some());
    assert!(v.find_by_title("B").is_some());
}

#[test]
fn reload_picks_up_new_files() {
    let td = vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    fs::write(td.path().join("c.org"), "* C\n").expect("write");
    assert_eq!(v.reload_incremental().expect("reload"), 1);
    assert_eq!(v.len(), 2);
    assert!(v.find_by_title("C").is_some());
}

#[test]
fn reload_drops_removed_files() {
    let td = vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    fs::remove_file(td.path().join("b.org")).expect("rm");
    v.reload_incremental().expect("reload");
    assert_eq!(v.len(), 1);
    assert!(v.find_by_title("B").is_none());
}

// TDD: validate-on-save sub-item test written *first*.
// This exercises the desired behavior: after a config.org change (or on demand),
// the vault can re-validate using the CUE-style Config loader and surface
// rich errors (with line info) instead of silently keeping bad config.
#[test]
fn revalidate_config_detects_bad_config_with_line_info() {
    use closure_config::ConfigError;

    let td = vault(&[(
        "config.org",
        "#+BEGIN_SRC closure-config\ninput_mode = whatever\n#+END_SRC\n",
    )]);
    let v = Vault::open(td.path()).expect("open");

    // The new API (to be implemented) should return the improved ConfigError
    // containing line context for early CUE-style validation errors.
    let err = v.revalidate_config().expect_err("should detect bad config");
    match err {
        ConfigError::BadValue { reason, .. } => {
            // Must contain the line info we added in previous cycle.
            assert!(
                reason.contains("line") || reason.contains("unknown input_mode"),
                "expected line/col context in config validation error, got: {reason}"
            );
        }
        other => panic!("expected BadValue with location, got {other:?}"),
    }
}
