//! Q3-V1/V2 — refiling and archiving a subtree.
//!
//! Two moves org users make daily and no shell could make: filing an
//! inbox capture under the project it belongs to (across files), and
//! putting a finished tree out of the way without deleting it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;
use tempfile::TempDir;

const INBOX: &str = "\
* TODO Buy milk
:PROPERTIES:
:ID: 01HQREFILE0000000000000001
:END:
with the good label
** TODO and cheese
:PROPERTIES:
:ID: 01HQREFILE0000000000000002
:END:
";

const PROJECT: &str = "\
* Errands
:PROPERTIES:
:ID: 01HQREFILE0000000000000010
:END:
** Existing child
:PROPERTIES:
:ID: 01HQREFILE0000000000000011
:END:
* Other top level
:PROPERTIES:
:ID: 01HQREFILE0000000000000012
:END:
";

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("inbox.org"), INBOX).expect("write");
    fs::write(dir.path().join("project.org"), PROJECT).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, vault)
}

fn read(dir: &TempDir, name: &str) -> String {
    fs::read_to_string(dir.path().join(name)).expect("read")
}

#[test]
fn v1_refiling_moves_the_subtree_under_the_target() {
    let (dir, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    let target = BlockId::from_existing("01HQREFILE0000000000000010");

    vault.refile(&item, &target).expect("refile");

    let inbox = read(&dir, "inbox.org");
    assert!(!inbox.contains("Buy milk"), "left the inbox: {inbox}");
    let project = read(&dir, "project.org");
    assert!(project.contains("** TODO Buy milk"), "{project}");
    assert!(
        project.contains("*** TODO and cheese"),
        "the child came with it, one level deeper: {project}"
    );
}

#[test]
fn v1_a_refiled_subtree_keeps_its_ids() {
    let (_d, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    let child = BlockId::from_existing("01HQREFILE0000000000000002");
    let target = BlockId::from_existing("01HQREFILE0000000000000010");

    vault.refile(&item, &target).expect("refile");

    let (found, path) = vault.find_by_id(&item).expect("the item still resolves");
    assert_eq!(found.title(), "Buy milk");
    assert!(path.ends_with("project.org"), "it lives in the target file");
    assert!(vault.find_by_id(&child).is_some(), "and so does its child");
}

#[test]
fn v1_it_lands_after_the_children_the_target_already_had() {
    let (dir, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    let target = BlockId::from_existing("01HQREFILE0000000000000010");

    vault.refile(&item, &target).expect("refile");

    let project = read(&dir, "project.org");
    let existing = project.find("Existing child").expect("existing child");
    let refiled = project.find("Buy milk").expect("refiled");
    assert!(existing < refiled, "last child, not first: {project}");
    let other = project.find("Other top level").expect("other");
    assert!(refiled < other, "and still inside the target: {project}");
}

#[test]
fn v1_refiling_into_the_same_file_works_too() {
    let (dir, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000012");
    let target = BlockId::from_existing("01HQREFILE0000000000000010");

    vault.refile(&item, &target).expect("refile");

    let project = read(&dir, "project.org");
    assert!(project.contains("** Other top level"), "{project}");
    assert_eq!(
        project.matches("Other top level").count(),
        1,
        "moved, not copied: {project}"
    );
}

#[test]
fn v1_refiling_a_headline_into_itself_is_refused() {
    let (_d, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    assert!(
        vault.refile(&item, &item).is_err(),
        "a subtree cannot be its own parent"
    );
}

#[test]
fn v1_refiling_into_ones_own_child_is_refused() {
    let (_d, mut vault) = vault();
    let parent = BlockId::from_existing("01HQREFILE0000000000000001");
    let child = BlockId::from_existing("01HQREFILE0000000000000002");
    assert!(
        vault.refile(&parent, &child).is_err(),
        "that would take the tree with it"
    );
}

// === archive ===

#[test]
fn v2_archiving_moves_the_subtree_to_the_archive_file() {
    let (dir, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");

    let archive = vault.archive_subtree(&item, "2026-07-28").expect("archive");

    assert!(archive.ends_with("inbox.org_archive"), "{archive:?}");
    let inbox = read(&dir, "inbox.org");
    assert!(!inbox.contains("Buy milk"), "gone from the file: {inbox}");
    let archived = read(&dir, "inbox.org_archive");
    assert!(archived.contains("* TODO Buy milk"), "{archived}");
    assert!(
        archived.contains(":ARCHIVE_TIME: 2026-07-28"),
        "stamped with when: {archived}"
    );
    assert!(
        archived.contains(":ARCHIVE_FILE:"),
        "and with where it came from: {archived}"
    );
}

#[test]
fn v2_a_second_archive_appends_rather_than_replaces() {
    let (dir, mut vault) = vault();
    vault
        .archive_subtree(
            &BlockId::from_existing("01HQREFILE0000000000000001"),
            "2026-07-28",
        )
        .expect("first");
    vault
        .archive_subtree(
            &BlockId::from_existing("01HQREFILE0000000000000012"),
            "2026-07-29",
        )
        .expect("second");

    let archived = read(&dir, "inbox.org_archive");
    assert!(archived.contains("Buy milk"), "{archived}");
    let other = read(&dir, "project.org_archive");
    assert!(other.contains("Other top level"), "{other}");
}

#[test]
fn v2_an_archived_note_still_resolves_after_a_reopen() {
    let (dir, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    vault.archive_subtree(&item, "2026-07-28").expect("archive");
    drop(vault);

    let reopened = Vault::open(dir.path()).expect("reopen");
    let (found, path) = reopened
        .find_by_id(&item)
        .expect("an archived note is still a note");
    assert_eq!(found.title(), "Buy milk");
    assert!(path.ends_with("inbox.org_archive"));
}

#[test]
fn v2_the_archive_file_is_a_vault_file_like_any_other() {
    let (_d, mut vault) = vault();
    let item = BlockId::from_existing("01HQREFILE0000000000000001");
    vault.archive_subtree(&item, "2026-07-28").expect("archive");

    let (found, path) = vault
        .find_by_id(&item)
        .expect("the archived headline still resolves by id");
    assert_eq!(found.title(), "Buy milk");
    assert!(path.ends_with("inbox.org_archive"));
}
