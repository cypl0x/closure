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

// === Iteration order ===
//
// Every list the shells paint — the outline, the block list, the
// database view, the agenda — is derived by walking `iter()`. It was
// backed by a `HashMap`, so that order was the hash seed's, and the
// same vault listed its headlines in a different order on every
// launch. A list that reshuffles between runs is not a list.

#[test]
fn iteration_is_ordered_by_path() {
    let dir = tempfile::tempdir().expect("tmp");
    for name in ["zeta.org", "alpha.org", "middle.org", "beta.org"] {
        fs::write(dir.path().join(name), "* H\n").expect("write");
    }
    let v = Vault::open(dir.path()).expect("open");
    let seen: Vec<String> = v
        .iter()
        .map(|(p, _)| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        seen,
        vec!["alpha.org", "beta.org", "middle.org", "zeta.org"],
        "files come out sorted by path"
    );
}

#[test]
fn iteration_order_is_stable_across_reopens() {
    let dir = tempfile::tempdir().expect("tmp");
    for name in ["c.org", "a.org", "d.org", "b.org", "e.org", "f.org"] {
        fs::write(dir.path().join(name), "* H\n").expect("write");
    }
    let order =
        |v: &Vault| -> Vec<std::path::PathBuf> { v.iter().map(|(p, _)| p.to_path_buf()).collect() };
    let first = order(&Vault::open(dir.path()).expect("open"));
    // A fresh process would reseed the hasher; within one process,
    // reopening is the closest reachable proxy — combined with the
    // sort assertion above, that pins the order for good.
    for _ in 0..5 {
        assert_eq!(order(&Vault::open(dir.path()).expect("open")), first);
    }
}

#[test]
fn iteration_order_survives_a_mutation() {
    let dir = tempfile::tempdir().expect("tmp");
    for name in ["b.org", "a.org"] {
        fs::write(dir.path().join(name), "* H\n").expect("write");
    }
    let mut v = Vault::open(dir.path()).expect("open");
    assert_eq!(v.iter().count(), 2);
    v.create_file(std::path::Path::new("aa.org"), "* New\n")
        .expect("create");
    let after: Vec<String> = v
        .iter()
        .map(|(p, _)| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        after,
        vec!["a.org", "aa.org", "b.org"],
        "a new file slots into place rather than landing wherever"
    );
}

#[test]
fn capture_bumps_the_revision_like_every_other_mutation() {
    // The one write path that reached the documents without going
    // through `reindex_file`, and so without moving the token every
    // shell memoises against: a captured item did not appear in the
    // outline until something *else* happened to change the vault.
    let (_d, mut v) = vault_with(SRC);
    let before = v.revision();
    let template = closure_store::CaptureTemplate {
        target: std::path::PathBuf::from("inbox.org"),
        headline_prefix: "TODO ".to_owned(),
        body: String::new(),
    };
    v.capture(&template, "Buy milk").expect("captured");
    assert_ne!(
        v.revision(),
        before,
        "a shell holding a cached row list must be told to rebuild it"
    );
}

#[test]
fn a_captured_headline_is_findable_by_id_immediately() {
    // The other half of the same bug: the id index has to know about
    // the new block before anything asks to select or sync it.
    let (_d, mut v) = vault_with(SRC);
    let template = closure_store::CaptureTemplate {
        target: std::path::PathBuf::from("inbox.org"),
        headline_prefix: "TODO ".to_owned(),
        body: String::new(),
    };
    let id = v.capture(&template, "Buy milk").expect("captured");
    assert!(
        v.find_by_id(&id).is_some(),
        "the capture is in the vault the moment it returns"
    );
}
