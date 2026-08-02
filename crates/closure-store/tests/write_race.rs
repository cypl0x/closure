//! A write must not throw away what someone else just wrote.
//!
//! closure writes a whole file on any mutation, from its in-memory
//! copy, and re-reads the vault on a 1.5-second poll. Between those two
//! facts is a window: change a file outside the app, mutate anything in
//! the same file inside it before the poll comes round, and the older
//! copy goes back over the newer one without a word.
//!
//! Observed twice on 2026-08-02, both times destroying real work — a
//! capture whose headline never reached disk, and three `TODO -> DONE`
//! toggles reverted by a closure that had been sitting open on the
//! vault. The poll is not the fix: shortening it shrinks the window
//! rather than closing it.
//!
//! So a mutation refreshes its file first when what is on disk is no
//! longer what was parsed, and applies on top of that.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQRACE00000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQRACE00000000000002
:END:
beta body
";

fn vault() -> (tempfile::TempDir, Vault, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("notes.org");
    fs::write(&path, NOTES).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v, path)
}

#[test]
fn an_external_edit_survives_a_rename_in_the_same_file() {
    let (_d, mut v, path) = vault();
    // Someone else — another closure, Emacs, a script — adds a headline.
    fs::write(&path, format!("{NOTES}* Gamma\ngamma body\n")).expect("write");

    v.rename_headline(&BlockId::from_existing("01HQRACE00000000000001"), "Renamed")
        .expect("rename");

    let on_disk = fs::read_to_string(&path).expect("read");
    assert!(on_disk.contains("Renamed"), "our edit landed: {on_disk}");
    assert!(
        on_disk.contains("* Gamma"),
        "and theirs is still there: {on_disk}"
    );
}

#[test]
fn an_external_edit_survives_a_body_write() {
    let (_d, mut v, path) = vault();
    fs::write(
        &path,
        NOTES.replace("beta body", "beta body edited elsewhere"),
    )
    .expect("write");

    v.set_body(
        &BlockId::from_existing("01HQRACE00000000000001"),
        "alpha body rewritten\n",
    )
    .expect("set_body");

    let on_disk = fs::read_to_string(&path).expect("read");
    assert!(on_disk.contains("alpha body rewritten"), "{on_disk}");
    assert!(on_disk.contains("beta body edited elsewhere"), "{on_disk}");
}

#[test]
fn a_todo_toggle_does_not_revert_someone_elses() {
    // The exact shape of what happened to the vault: two DONE toggles,
    // one made outside, one made inside.
    let (_d, mut v, path) = vault();
    fs::write(&path, NOTES.replace("* Beta", "* DONE Beta")).expect("write");

    v.set_todo(
        &BlockId::from_existing("01HQRACE00000000000001"),
        Some("DONE"),
    )
    .expect("set_todo");

    let on_disk = fs::read_to_string(&path).expect("read");
    assert!(on_disk.contains("* DONE Alpha"), "ours: {on_disk}");
    assert!(on_disk.contains("* DONE Beta"), "theirs: {on_disk}");
}

#[test]
fn a_file_nobody_touched_is_written_as_before() {
    let (_d, mut v, path) = vault();
    v.rename_headline(&BlockId::from_existing("01HQRACE00000000000002"), "Beta II")
        .expect("rename");
    let on_disk = fs::read_to_string(&path).expect("read");
    assert!(on_disk.contains("Beta II"), "{on_disk}");
    assert!(on_disk.contains("* Alpha"), "{on_disk}");
}

#[test]
fn a_headline_added_outside_can_be_addressed_immediately() {
    // The refresh has to reindex, or the new id is unknown to us until
    // the poll comes round.
    let (_d, mut v, path) = vault();
    fs::write(
        &path,
        format!("{NOTES}* Gamma\n:PROPERTIES:\n:ID: 01HQRACE00000000000003\n:END:\ngamma\n"),
    )
    .expect("write");

    v.rename_headline(
        &BlockId::from_existing("01HQRACE00000000000001"),
        "Alpha II",
    )
    .expect("rename");
    assert!(
        v.find_by_id(&BlockId::from_existing("01HQRACE00000000000003"))
            .is_some(),
        "the headline written under us is addressable"
    );
}
