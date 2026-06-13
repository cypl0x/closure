//! Kill ring: cut a headline subtree to an in-memory ring, paste it
//! after another headline. Cut+paste is a move, so ids stay unique
//! (I2); both ends ride kernel-backed writes.

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

fn ids(td: &TempDir) -> (BlockId, BlockId) {
    let v = Vault::open(td.path()).expect("open");
    (
        v.find_by_title("One").expect("one").0.id().clone(),
        v.find_by_title("Three").expect("three").0.id().clone(),
    )
}

fn three() -> TempDir {
    write_vault(&[(
        "a.org",
        "* One\n:PROPERTIES:\n:ID: 01HX0000000000000000000001\n:END:\nbody one\n\
         * Two\n:PROPERTIES:\n:ID: 01HX0000000000000000000002\n:END:\n\
         * Three\n:PROPERTIES:\n:ID: 01HX0000000000000000000003\n:END:\n",
    )])
}

#[test]
fn cut_removes_subtree_and_fills_ring() {
    let td = three();
    let (one, _) = ids(&td);
    let mut v = Vault::open(td.path()).expect("open");
    v.cut(&td.path().join("a.org"), &one).expect("cut");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(!disk.contains("* One"), "cut removed it: {disk:?}");
    assert!(v.find_by_id(&one).is_none());
    assert!(v.ring_top().is_some_and(|s| s.contains("body one")));
}

#[test]
fn paste_inserts_ring_after_target() {
    let td = three();
    let (one, three) = ids(&td);
    let mut v = Vault::open(td.path()).expect("open");
    v.cut(&td.path().join("a.org"), &one).expect("cut");
    v.paste(&td.path().join("a.org"), &three).expect("paste");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("* One"), "pasted back: {disk:?}");
    assert!(
        disk.find("* Three").unwrap() < disk.find("* One").unwrap(),
        "One now after Three"
    );
    // id preserved through the round-trip (I2)
    assert!(v.find_by_id(&one).is_some());
}

#[test]
fn paste_with_empty_ring_errors() {
    let td = three();
    let (_, three) = ids(&td);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(matches!(
        v.paste(&td.path().join("a.org"), &three),
        Err(VaultError::Command(_))
    ));
}

#[test]
fn cut_unknown_id_errors() {
    let td = three();
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01HXZZZZZZZZZZZZZZZZZZZZZZ");
    assert!(v.cut(&td.path().join("a.org"), &bogus).is_err());
}

#[test]
fn cut_paste_keeps_disk_and_memory_in_sync() {
    let td = three();
    let (one, three) = ids(&td);
    let mut v = Vault::open(td.path()).expect("open");
    v.cut(&td.path().join("a.org"), &one).expect("cut");
    v.paste(&td.path().join("a.org"), &three).expect("paste");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mem = v.document(&td.path().join("a.org")).expect("doc").source();
    assert_eq!(mem, disk);
}
