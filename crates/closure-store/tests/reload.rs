//! Content-hash caching reload: unchanged files are not re-parsed.

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
