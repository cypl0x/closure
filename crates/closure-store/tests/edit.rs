//! Vault-level structural edits: add a sibling headline, remove a
//! subtree. Both run through kernel commands (undoable, I3) and keep
//! disk, in-memory documents, and the id index in sync.

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
fn add_sibling_appears_after_target_on_disk() {
    let td = write_vault(&[("a.org", "* First\n* Last\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "First");
    v.add_sibling(&id, "Middle").expect("add");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    let first = disk.find("* First").expect("first");
    let middle = disk.find("* Middle").expect("middle");
    let last = disk.find("* Last").expect("last");
    assert!(first < middle && middle < last, "order: {disk:?}");
}

#[test]
fn add_sibling_is_resolvable_in_the_index() {
    let td = write_vault(&[("a.org", "* First\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "First");
    v.add_sibling(&id, "Fresh").expect("add");
    let (h, _) = v.find_by_title("Fresh").expect("indexed");
    assert!(v.find_by_id(&h.id().clone()).is_some());
}

#[test]
fn add_sibling_unknown_id_errors_without_disk_change() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let before = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01XXXXXXXXXXXXXXXXXXXXXXXX");
    assert!(matches!(
        v.add_sibling(&bogus, "Nope"),
        Err(VaultError::UnknownId(_))
    ));
    assert_eq!(
        before,
        fs::read_to_string(td.path().join("a.org")).expect("read")
    );
}

#[test]
fn remove_subtree_deletes_headline_and_children() {
    let td = write_vault(&[("a.org", "* Keep\n* Doomed\n** Child\nbody\n* Tail\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Doomed");
    v.remove_subtree(&id).expect("remove");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("* Keep"));
    assert!(disk.contains("* Tail"));
    assert!(!disk.contains("Doomed"));
    assert!(!disk.contains("Child"));
}

#[test]
fn remove_subtree_unindexes_removed_ids() {
    let td = write_vault(&[("a.org", "* Keep\n* Doomed\n** Child\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let doomed = id_of(&v, "Doomed");
    let child = id_of(&v, "Child");
    v.remove_subtree(&doomed).expect("remove");
    assert!(v.find_by_id(&doomed).is_none());
    assert!(v.find_by_id(&child).is_none());
    assert!(v.find_by_title("Keep").is_some());
}

#[test]
fn edits_keep_vault_and_disk_in_sync() {
    let td = write_vault(&[("a.org", "* One\n* Two\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let one = id_of(&v, "One");
    v.add_sibling(&one, "One half").expect("add");
    let two = id_of(&v, "Two");
    v.remove_subtree(&two).expect("remove");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mem = v
        .document(&td.path().join("a.org"))
        .expect("doc cached")
        .source();
    assert_eq!(mem, disk);
}

#[test]
fn edited_file_reopens_cleanly() {
    let td = write_vault(&[("a.org", "* Base\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Base");
    v.add_sibling(&id, "Added").expect("add");
    let reopened = Vault::open(td.path()).expect("reopen");
    assert!(reopened.find_by_title("Added").is_some());
}

#[test]
fn set_property_writes_drawer_to_disk() {
    let td = write_vault(&[("a.org", "* Task\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Task");
    v.set_property(&id, "EFFORT", "2d").expect("set");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains(":EFFORT: 2d"));
    let (h, _) = v.find_by_id(&id).expect("resolves");
    assert_eq!(h.property("EFFORT"), Some("2d"));
}

#[test]
fn set_property_overwrites_existing_value() {
    let td = write_vault(&[(
        "a.org",
        "* Task\n:PROPERTIES:\n:EFFORT: 1d\n:END:\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Task");
    v.set_property(&id, "EFFORT", "5d").expect("set");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains(":EFFORT: 5d"));
    assert!(!disk.contains(":EFFORT: 1d"));
}

#[test]
fn set_property_unknown_id_errors() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01XXXXXXXXXXXXXXXXXXXXXXXX");
    assert!(matches!(
        v.set_property(&bogus, "K", "V"),
        Err(VaultError::UnknownId(_))
    ));
}

#[test]
fn set_body_replaces_headline_body() {
    let td = write_vault(&[("a.org", "* Task\nold body\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Task");
    v.set_body(&id, "new body line\nsecond\n").expect("set body");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("new body line\nsecond\n"));
    assert!(!disk.contains("old body"));
    let (h, _) = v.find_by_id(&id).expect("resolves");
    assert!(h.body_text().contains("new body line"));
}

#[test]
fn set_body_unknown_id_errors() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01XXXXXXXXXXXXXXXXXXXXXXXX");
    assert!(matches!(
        v.set_body(&bogus, "x\n"),
        Err(VaultError::UnknownId(_))
    ));
}

#[test]
fn set_body_is_undoable() {
    let td = write_vault(&[("a.org", "* T\norig\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "T");
    v.set_body(&id, "changed\n").expect("set");
    v.undo_in(&td.path().join("a.org")).expect("undo");
    let (h, _) = v.find_by_id(&id).expect("resolves");
    assert!(h.body_text().contains("orig"), "I3: body edit on undo-tree");
}

#[test]
fn promote_decreases_level_on_disk() {
    let td = write_vault(&[("a.org", "* Parent\n** Child\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Child");
    v.promote(&id).expect("promote");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("* Child"), "got {disk:?}");
    assert!(!disk.contains("** Child"));
}

#[test]
fn demote_increases_level() {
    let td = write_vault(&[("a.org", "* One\n* Two\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "Two");
    v.demote(&id).expect("demote");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("** Two"));
}

#[test]
fn promote_is_undoable() {
    let td = write_vault(&[(
        "a.org",
        "* P\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n\
         ** C\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = id_of(&v, "C");
    v.promote(&id).expect("promote");
    v.undo_in(&td.path().join("a.org")).expect("undo");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("** C"), "I3: level edit on undo-tree");
}

#[test]
fn promote_unknown_id_errors() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let bogus = BlockId::from_existing("01XXXXXXXXXXXXXXXXXXXXXXXX");
    assert!(matches!(v.promote(&bogus), Err(VaultError::UnknownId(_))));
}

#[test]
fn move_after_reorders_siblings() {
    let td = write_vault(&[(
        "a.org",
        "* One\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n\
         * Two\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    let one = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    let two = BlockId::from_existing("01HXBBBBBBBBBBBBBBBBBBBBBB");
    v.move_after(&one, &two).expect("move");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(
        disk.find("* Two").unwrap() < disk.find("* One").unwrap(),
        "One now after Two: {disk:?}"
    );
}

#[test]
fn move_after_unknown_id_errors() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let a = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    let bogus = BlockId::from_existing("01HXZZZZZZZZZZZZZZZZZZZZZZ");
    assert!(v.move_after(&bogus, &a).is_err());
}
