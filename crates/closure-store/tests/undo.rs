//! Vault-level undo/redo: walks the per-document undo-tree (I3) and
//! persists the reverted state to disk.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use closure_core::BlockId;
use closure_store::{Vault, VaultError};
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    dir
}

fn id_of(v: &Vault, title: &str) -> BlockId {
    let (h, _) = v.find_by_title(title).expect("headline exists");
    h.id().clone()
}

fn file(td: &TempDir) -> PathBuf {
    td.path().join("a.org")
}

#[test]
fn undo_reverts_rename_on_disk() {
    let td = write_vault(&[("a.org", "* Original\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Original");
    v.rename_headline(&id, "Changed").expect("rename");
    v.undo_in(&file(&td)).expect("undo");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("Original"));
    assert!(!disk.contains("Changed"));
}

#[test]
fn undo_reverts_added_sibling() {
    let td = write_vault(&[("a.org", "* Base\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Base");
    v.add_sibling(&id, "Ephemeral").expect("add");
    v.undo_in(&file(&td)).expect("undo");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(!disk.contains("Ephemeral"));
    assert!(v.find_by_title("Ephemeral").is_none(), "index follows");
}

#[test]
fn redo_reapplies_undone_edit() {
    let td = write_vault(&[("a.org", "* Original\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Original");
    v.rename_headline(&id, "Changed").expect("rename");
    v.undo_in(&file(&td)).expect("undo");
    v.redo_in(&file(&td)).expect("redo");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("Changed"));
    assert!(!disk.contains("Original"));
}

#[test]
fn undo_with_empty_history_errors() {
    let td = write_vault(&[("a.org", "* Untouched\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(matches!(v.undo_in(&file(&td)), Err(VaultError::Undo(_))));
}

#[test]
fn undo_on_unknown_file_errors() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(v.undo_in(&td.path().join("nope.org")).is_err());
}

#[test]
fn undo_keeps_vault_and_disk_in_sync() {
    let td = write_vault(&[("a.org", "* Original\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Original");
    v.rename_headline(&id, "Changed").expect("rename");
    v.undo_in(&file(&td)).expect("undo");
    let disk = fs::read_to_string(file(&td)).expect("read");
    let mem = v.document(&file(&td)).expect("doc").source();
    assert_eq!(mem, disk);
}

// === U2b: jump to any history node, persisted to disk (Q2). ===

#[test]
fn jump_history_walks_branches_and_persists() {
    let td = write_vault(&[("a.org", "* Old\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Old");
    v.rename_headline(&id, "A").expect("A");
    v.rename_headline(&id, "B").expect("B");
    v.undo_in(&file(&td)).expect("undo B");
    v.rename_headline(&id, "C").expect("C");
    // history nodes: 0 = A, 1 = B, 2 = C (C branches from A).
    v.jump_history_in(&file(&td), 1).expect("jump to B");
    let on_disk = fs::read_to_string(file(&td)).expect("read");
    assert!(on_disk.contains("* B"), "disk shows B: {on_disk}");
    v.jump_history_in(&file(&td), 2).expect("jump to C");
    let on_disk = fs::read_to_string(file(&td)).expect("read");
    assert!(on_disk.contains("* C"), "disk shows C: {on_disk}");
}

#[test]
fn jump_history_bad_args_error() {
    let td = write_vault(&[("a.org", "* Old\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Old");
    v.rename_headline(&id, "A").expect("A");
    assert!(v.jump_history_in(&file(&td), 9).is_err(), "bad index");
    assert!(
        v.jump_history_in(&PathBuf::from("/nope.org"), 0).is_err(),
        "unknown path"
    );
}
