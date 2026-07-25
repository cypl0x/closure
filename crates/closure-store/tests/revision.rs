//! The vault revision counter: a cheap, monotone "did anything
//! change?" token so shells can memoise derived views (row lists,
//! agendas, backlink tables) instead of re-walking every document on
//! every frame.
//!
//! Invariant: `revision()` is *stable* across reads and *strictly
//! increases* across every mutation, whatever path reached the
//! documents — kernel commands, undo/redo, file-level operations, or
//! a reload from disk. A shell may therefore treat an unchanged
//! revision as "my cached derivation is still exact".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;

fn vault_with(src: &str) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), src).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

const SRC: &str = "* TODO One\n:PROPERTIES:\n:ID: 01HQREV00000000000000001\n:END:\n\
                   * Two\n:PROPERTIES:\n:ID: 01HQREV00000000000000002\n:END:\n";

#[test]
fn reads_never_bump_the_revision() {
    let (_d, v) = vault_with(SRC);
    let before = v.revision();
    // Every read-only accessor a shell touches per frame.
    let _ = v.iter().count();
    let _ = v.find_by_id(&BlockId::from_existing("01HQREV00000000000000001"));
    let _ = v.backlinks_of("01HQREV00000000000000001");
    let _ = v.revision();
    assert_eq!(
        v.revision(),
        before,
        "reading the vault must leave the revision alone — otherwise a \
         cache keyed on it never hits"
    );
}

#[test]
fn every_block_mutation_bumps_the_revision() {
    let (_d, mut v) = vault_with(SRC);
    let id = BlockId::from_existing("01HQREV00000000000000001");
    let mut last = v.revision();
    let bumped = |v: &Vault, what: &str, last: &mut u64| {
        assert!(
            v.revision() > *last,
            "{what} must bump the revision ({} -> {})",
            *last,
            v.revision()
        );
        *last = v.revision();
    };

    v.rename_headline(&id, "One renamed").expect("rename");
    bumped(&v, "rename_headline", &mut last);

    v.set_todo(&id, Some("DONE")).expect("todo");
    bumped(&v, "set_todo", &mut last);

    v.set_priority(&id, Some('A')).expect("priority");
    bumped(&v, "set_priority", &mut last);

    v.set_tags(&id, &["work".to_owned()]).expect("tags");
    bumped(&v, "set_tags", &mut last);

    v.set_body(&id, "body\n").expect("body");
    bumped(&v, "set_body", &mut last);

    // The fold toggle rides this path — the outline row list depends
    // on it, so it must invalidate too.
    v.set_property(&id, "VISIBILITY", "folded").expect("prop");
    bumped(&v, "set_property", &mut last);

    v.demote(&id).expect("demote");
    bumped(&v, "demote", &mut last);

    v.promote(&id).expect("promote");
    bumped(&v, "promote", &mut last);

    v.add_sibling(&id, "Three").expect("add_sibling");
    bumped(&v, "add_sibling", &mut last);

    v.remove_subtree(&BlockId::from_existing("01HQREV00000000000000002"))
        .expect("remove");
    bumped(&v, "remove_subtree", &mut last);
}

#[test]
fn undo_and_redo_bump_the_revision() {
    let (dir, mut v) = vault_with(SRC);
    let path = dir.path().join("notes.org");
    let id = BlockId::from_existing("01HQREV00000000000000001");
    v.rename_headline(&id, "changed").expect("rename");
    let mut last = v.revision();

    v.undo_in(&path).expect("undo");
    assert!(
        v.revision() > last,
        "undo restores content — it is a change"
    );
    last = v.revision();

    v.redo_in(&path).expect("redo");
    assert!(v.revision() > last, "redo is a change too");
}

#[test]
fn file_level_operations_bump_the_revision() {
    let (dir, mut v) = vault_with(SRC);
    let mut last = v.revision();

    let created = v
        .create_file(std::path::Path::new("extra.org"), "* Extra\n")
        .expect("create");
    assert!(v.revision() > last, "create_file adds headlines");
    last = v.revision();

    let renamed = v
        .rename_file(&created, std::path::Path::new("moved.org"))
        .expect("rename_file");
    assert!(v.revision() > last, "rename_file moves rows between files");
    last = v.revision();

    v.delete_file(&renamed).expect("delete");
    assert!(v.revision() > last, "delete_file drops headlines");
    last = v.revision();

    // An out-of-band edit picked up by the incremental reload is a
    // change like any other (the watcher path).
    fs::write(dir.path().join("notes.org"), "* Rewritten\n").expect("write");
    assert!(v.reload_incremental().expect("reload") > 0);
    assert!(
        v.revision() > last,
        "reload_incremental that re-parsed a file must bump"
    );
    last = v.revision();

    // …and a reload that finds nothing new must NOT bump, or the
    // watcher tick would invalidate every cache every poll.
    assert_eq!(v.reload_incremental().expect("reload"), 0);
    assert_eq!(
        v.revision(),
        last,
        "a no-op incremental reload must leave the revision alone"
    );
}

#[test]
fn full_reload_bumps_the_revision() {
    let (dir, mut v) = vault_with(SRC);
    let last = v.revision();
    fs::write(dir.path().join("notes.org"), "* Different\n").expect("write");
    v.reload().expect("reload");
    assert!(v.revision() > last, "a full reload replaces every document");
}
