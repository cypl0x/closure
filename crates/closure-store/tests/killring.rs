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

#[test]
fn text_can_be_pushed_onto_the_ring_directly() {
    // Not everything worth copying is a subtree. A shell offering
    // "copy this" — a sync ticket, a block id — needs the same ring,
    // or it needs a second clipboard that nothing else can read.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* H\n").expect("write");
    let mut v = Vault::open(dir.path()).expect("open");
    assert_eq!(v.ring_top(), None);
    v.push_kill_ring("closure-sync:127.0.0.1:7420|abc".to_owned());
    assert_eq!(v.ring_top(), Some("closure-sync:127.0.0.1:7420|abc"));
    v.push_kill_ring("second".to_owned());
    assert_eq!(v.ring_top(), Some("second"), "most recent is on top");
}

// === A headline whose id was never written to the file ===
//
// `cut` located its subtree through `OrgDoc::subtree_of`, which reads
// the `:ID:` property out of the *source text*. A headline that has
// never been given a drawer — most of them, in a file a person typed —
// has its id only in the in-memory index, so `cut` could not find it
// and refused. Every other operation addresses those headlines fine,
// which is how it went unnoticed until `delete` started cutting.

#[test]
fn cut_works_on_a_headline_with_no_id_drawer() {
    let dir = write_vault(&[("n.org", "* Alpha\n** Kid\nbody\n* Beta\n")]);
    let mut v = Vault::open(dir.path()).expect("open");
    let path = dir.path().join("n.org");
    let alpha = v
        .document(&path)
        .expect("doc")
        .all_block_ids()
        .first()
        .cloned()
        .expect("an id");
    v.cut(&path, &alpha).expect("cut a drawerless headline");
    assert!(
        v.ring_top()
            .is_some_and(|s| s.contains("Alpha") && s.contains("Kid")),
        "the whole subtree is on the ring: {:?}",
        v.ring_top()
    );
    let left = v.document(&path).expect("doc").org().source().to_owned();
    assert!(!left.contains("Alpha"), "and out of the file: {left}");
    assert!(left.contains("Beta"), "its sibling stayed");
}

#[test]
fn cut_takes_the_headline_it_was_asked_for() {
    // The subtree is found by walking to the right node, so picking the
    // second one must not hand back the first.
    let dir = write_vault(&[("n.org", "* Alpha\n* Beta\n** Kid\n* Gamma\n")]);
    let mut v = Vault::open(dir.path()).expect("open");
    let path = dir.path().join("n.org");
    let beta = v
        .document(&path)
        .expect("doc")
        .all_block_ids()
        .get(1)
        .cloned()
        .expect("second id");
    v.cut(&path, &beta).expect("cut");
    let top = v.ring_top().expect("ring").to_owned();
    assert!(top.contains("Beta") && top.contains("Kid"), "{top}");
    assert!(!top.contains("Alpha") && !top.contains("Gamma"), "{top}");
}
