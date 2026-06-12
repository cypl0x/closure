//! Vault-level rename: applies the kernel `RenameHeadline` command
//! (undoable, I3) and persists the result to disk.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

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

#[test]
fn rename_changes_title_in_memory_and_on_disk() {
    let td = write_vault(&[("a.org", "* Old name\nbody\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Old name");
    v.rename_headline(&id, "New name").expect("rename");
    let (h, _) = v.find_by_id(&id).expect("still resolvable");
    assert_eq!(h.title(), "New name");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("New name"));
    assert!(!disk.contains("Old name"));
}

#[test]
fn rename_preserves_body_and_siblings() {
    let td = write_vault(&[("a.org", "* Keep\nkeep body\n* Target\n* Tail\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Target");
    v.rename_headline(&id, "Renamed").expect("rename");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("* Keep\nkeep body\n"));
    assert!(disk.contains("* Tail"));
    assert!(disk.contains("* Renamed"));
}

#[test]
fn rename_unknown_id_errors_and_touches_nothing() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let before = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01XXXXXXXXXXXXXXXXXXXXXXXX");
    let err = v.rename_headline(&bogus, "Nope");
    assert!(matches!(err, Err(VaultError::UnknownId(_))));
    let after = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert_eq!(before, after);
}

#[test]
fn rename_keeps_vault_and_disk_in_sync() {
    let td = write_vault(&[("a.org", "* Sync me\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Sync me");
    v.rename_headline(&id, "Synced").expect("rename");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mem = v
        .document(&td.path().join("a.org"))
        .expect("doc cached")
        .source();
    assert_eq!(mem, disk);
}

#[test]
fn rename_keeps_block_id_stable() {
    let td = write_vault(&[("a.org", "* Stable\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Stable");
    v.rename_headline(&id, "Still stable").expect("rename");
    let (h, _) = v.find_by_id(&id).expect("same id resolves");
    assert_eq!(h.id(), &id, "I2: edits never regenerate ids");
}

#[test]
fn renamed_file_roundtrips_on_reopen() {
    let td = write_vault(&[("a.org", "* Before\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Before");
    v.rename_headline(&id, "After").expect("rename");
    let reopened = Vault::open(td.path()).expect("reopen");
    assert!(reopened.find_by_title("After").is_some());
}
