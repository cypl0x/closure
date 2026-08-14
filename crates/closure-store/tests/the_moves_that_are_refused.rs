//! What the vault refuses, and the graph statistics nothing had asked
//! for.
//!
//! `refile` and `archive_subtree` move whole subtrees between files.
//! Their happy paths have tests; their refusals did not, and the
//! refusals are the load-bearing half. A move that goes ahead when it
//! should not does not report anything — it relocates somebody's notes,
//! and the pathological case (filing a tree inside itself) takes the
//! target with it, which is how a subtree disappears entirely.
//!
//! `ensure_todo_keywords` is here because its whole job is *not*
//! stacking a second `#+TODO:` line: two declarations are two sequences
//! in org, so getting this wrong changes what every keyword in the file
//! means.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;
use tempfile::TempDir;

const A: &str = "01STOREMOVE000000001";
const CHILD: &str = "01STOREMOVE000000002";
const B: &str = "01STOREMOVE000000003";

fn vault() -> (TempDir, Vault) {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* Parent\n:PROPERTIES:\n:ID: {A}\n:END:\n\
             the parent body\n\
             ** A child\n:PROPERTIES:\n:ID: {CHILD}\n:END:\n\
             the child body\n\
             * Elsewhere\n:PROPERTIES:\n:ID: {B}\n:END:\n"
        ),
    )
    .expect("write");
    let v = Vault::open(d.path()).expect("open");
    (d, v)
}

fn id(s: &str) -> BlockId {
    BlockId::from_existing(s)
}

#[test]
fn a_headline_cannot_be_filed_under_itself() {
    let (d, mut v) = vault();
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert!(
        v.refile(&id(A), &id(A)).is_err(),
        "filed a tree into itself"
    );
    assert_eq!(
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        before,
        "a refused refile changed the file"
    );
}

#[test]
fn a_headline_cannot_be_filed_under_its_own_descendant() {
    // The one the doc calls out: the move "would put a subtree inside
    // itself — which would take the target with it". The target is
    // inside the thing being moved, so a naive implementation deletes
    // the source, then cannot find where to put it.
    let (d, mut v) = vault();
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert!(
        v.refile(&id(A), &id(CHILD)).is_err(),
        "filed a parent under its own child"
    );
    assert_eq!(
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        before,
        "the vault changed on a refused refile"
    );
}

#[test]
fn refiling_to_or_from_an_id_that_is_not_there_is_refused() {
    let (d, mut v) = vault();
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");
    let missing = id("01NOSUCHIDATALL00000");
    assert!(v.refile(&missing, &id(A)).is_err(), "moved a ghost");
    assert!(v.refile(&id(A), &missing).is_err(), "moved into a ghost");
    assert_eq!(
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        before
    );
}

#[test]
fn a_refile_that_is_allowed_keeps_the_id_so_links_still_resolve() {
    // I2, and the doc states it: "a refiled note is the same note, and
    // every `id:` link into it still resolves". A move that reissued
    // ids would break every link into the moved tree, silently.
    let (_d, mut v) = vault();
    v.refile(&id(CHILD), &id(B)).expect("refile");
    assert!(
        v.find_by_id(&id(CHILD)).is_some(),
        "the refiled headline lost its id"
    );
    assert!(
        v.find_by_id(&id(A)).is_some() && v.find_by_id(&id(B)).is_some(),
        "another headline lost its id in the move"
    );
}

#[test]
fn archiving_an_id_that_is_not_there_is_refused() {
    let (_d, mut v) = vault();
    assert!(
        v.archive_subtree(&id("01NOSUCHIDATALL00000"), "2026-08-14")
            .is_err()
    );
}

#[test]
fn archiving_moves_the_tree_out_and_leaves_the_rest() {
    let (_d, mut v) = vault();
    let path = v
        .archive_subtree(&id(CHILD), "2026-08-14")
        .expect("archive");
    assert!(
        path.to_string_lossy().contains("archive"),
        "archived to {}",
        path.display()
    );
    // The sibling is untouched and still addressable.
    assert!(v.find_by_id(&id(B)).is_some());
}

#[test]
fn setting_a_subtree_on_an_id_that_is_not_there_is_refused() {
    let (d, mut v) = vault();
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert!(
        v.set_subtree(&id("01NOSUCHIDATALL00000"), "body\n", "")
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        before
    );
}

#[test]
fn setting_a_subtree_replaces_the_body_and_the_children_together() {
    let (_d, mut v) = vault();
    v.set_subtree(&id(A), "a new body\n", "** A new child\n")
        .expect("set");
    let src = v
        .document(&v.root().join("notes.org"))
        .map(closure_core::Document::source)
        .expect("document");
    assert!(src.contains("a new body"), "{src}");
    assert!(src.contains("A new child"), "{src}");
    assert!(!src.contains("the parent body"), "{src}");
    assert!(
        !src.contains("A child\n"),
        "the old child survived a full subtree replacement: {src}"
    );
}

#[test]
fn ensure_todo_keywords_writes_one_declaration_and_not_two() {
    // The whole job. Two `#+TODO:` lines are two sequences in org, so
    // stacking a second changes what every keyword in the file means.
    let (d, mut v) = vault();
    let path = d.path().join("notes.org");

    // Not TODO/DONE: those are org's own defaults, so a file that
    // does not declare them already has them and there is nothing to
    // write. Correct, and the reason this needs a keyword org does not
    // ship with.
    let changed = v
        .ensure_todo_keywords(
            &path,
            &["TODO".to_owned(), "DOING".to_owned(), "DONE".to_owned()],
        )
        .expect("first");
    assert!(changed, "the first call declared nothing");
    let once = v
        .document(&path)
        .map(closure_core::Document::source)
        .expect("doc");
    assert_eq!(
        once.matches("#+TODO:").count(),
        1,
        "not exactly one declaration: {once}"
    );

    // A second call with different keywords replaces rather than adds.
    let changed = v
        .ensure_todo_keywords(
            &path,
            &[
                "TODO".to_owned(),
                "DOING".to_owned(),
                "BLOCKED".to_owned(),
                "DONE".to_owned(),
            ],
        )
        .expect("second");
    assert!(changed, "changing the keywords declared nothing");
    let twice = v
        .document(&path)
        .map(closure_core::Document::source)
        .expect("doc");
    assert_eq!(
        twice.matches("#+TODO:").count(),
        1,
        "a second declaration was stacked: {twice}"
    );
    assert!(twice.contains("BLOCKED"), "{twice}");
}

#[test]
fn ensure_todo_keywords_does_nothing_when_there_is_nothing_to_do() {
    let (d, mut v) = vault();
    let path = d.path().join("notes.org");

    // No keywords at all: nothing to declare.
    assert!(!v.ensure_todo_keywords(&path, &[]).expect("empty"));

    // Org's own defaults need no declaration at all: a file that says
    // nothing already means TODO/DONE.
    assert!(
        !v.ensure_todo_keywords(&path, &["TODO".to_owned(), "DONE".to_owned()])
            .expect("defaults"),
        "org's defaults were written out as if they were custom"
    );

    // And once a custom set is declared, declaring it again is no
    // rewrite — no revision churn, no spurious diff in a git history.
    let custom = ["TODO".to_owned(), "DOING".to_owned(), "DONE".to_owned()];
    v.ensure_todo_keywords(&path, &custom).expect("first");
    assert!(
        !v.ensure_todo_keywords(&path, &custom).expect("second"),
        "an unchanged declaration was rewritten"
    );
}

#[test]
fn ensure_todo_keywords_on_a_file_the_vault_does_not_have_is_a_no_op() {
    let (d, mut v) = vault();
    assert!(
        !v.ensure_todo_keywords(&d.path().join("absent.org"), &["TODO".to_owned()])
            .expect("no-op")
    );
}

#[test]
fn the_most_referenced_id_is_the_one_with_the_most_links_in() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* Hub\n:PROPERTIES:\n:ID: {A}\n:END:\n\
             * One\n:PROPERTIES:\n:ID: {CHILD}\n:END:\nsee [[id:{A}]]\n\
             * Two\n:PROPERTIES:\n:ID: {B}\n:END:\nalso [[id:{A}]] and [[id:{CHILD}]]\n"
        ),
    )
    .expect("write");
    let v = Vault::open(d.path()).expect("open");

    let (winner, count) = v.most_referenced().expect("a most-referenced id");
    assert_eq!(winner, A, "the wrong id won");
    assert_eq!(count, 2);
}

#[test]
fn a_vault_with_no_links_has_no_most_referenced_id() {
    let (_d, v) = vault();
    assert_eq!(v.most_referenced(), None);
}

#[test]
fn dead_links_are_counted_and_live_ones_are_not() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* One\n:PROPERTIES:\n:ID: {A}\n:END:\n\
             a live [[id:{B}]] and a dead [[id:01NOSUCHIDATALL00000]]\n\
             * Two\n:PROPERTIES:\n:ID: {B}\n:END:\n"
        ),
    )
    .expect("write");
    let v = Vault::open(d.path()).expect("open");
    assert_eq!(v.dead_link_count(), 1, "the live link was counted too");
}

#[test]
fn a_vault_with_only_live_links_has_no_dead_ones() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* One\n:PROPERTIES:\n:ID: {A}\n:END:\nsee [[id:{B}]]\n\
             * Two\n:PROPERTIES:\n:ID: {B}\n:END:\n"
        ),
    )
    .expect("write");
    let v = Vault::open(d.path()).expect("open");
    assert_eq!(v.dead_link_count(), 0);
}
