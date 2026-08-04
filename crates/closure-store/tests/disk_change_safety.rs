//! "everytime I want to write headline, the whole file (e.g.
//! inbox.org) will be overwritten with my stale version … So what is
//! the best way to handle this? What is the benefit of writing the
//! whole file? … I am not sure if I want to change it if it is already
//! robust."
//!
//! These answer that by measurement rather than by assertion: each one
//! is a way the file can change under closure, and what survives.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::BlockId;
use closure_store::Vault;

const ORG: &str = "\
* One
:PROPERTIES:
:ID: disk-one
:END:
first body
* Two
:PROPERTIES:
:ID: disk-two
:END:
second body
";

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), ORG).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

/// Append a headline the way another editor would.
fn external_append(dir: &std::path::Path, text: &str) {
    let path = dir.join("a.org");
    let mut src = std::fs::read_to_string(&path).unwrap();
    src.push_str(text);
    std::fs::write(&path, src).unwrap();
}

#[test]
fn an_external_headline_survives_a_mutation_in_the_same_file() {
    // The case that destroyed a capture on 2026-08-02.
    let (dir, mut v) = vault();
    external_append(
        &dir.path().to_path_buf(),
        "* From Emacs\n:PROPERTIES:\n:ID: disk-ext\n:END:\n",
    );
    v.set_todo(&BlockId::from_existing("disk-one"), Some("TODO"))
        .unwrap();
    let disk = std::fs::read_to_string(dir.path().join("a.org")).unwrap();
    assert!(
        disk.contains("From Emacs"),
        "the external headline was lost:\n{disk}"
    );
    assert!(disk.contains("* TODO One"), "{disk}");
}

#[test]
fn an_external_edit_to_the_very_block_being_changed_is_not_lost_silently() {
    // Harder: the other writer touched the *same* headline. One of the
    // two edits must win — but the file must stay coherent org, and the
    // change we make must be the one that lands.
    let (dir, mut v) = vault();
    let path = dir.path().join("a.org");
    let src = std::fs::read_to_string(&path)
        .unwrap()
        .replace("first body", "body from Emacs");
    std::fs::write(&path, src).unwrap();

    v.set_todo(&BlockId::from_existing("disk-one"), Some("TODO"))
        .unwrap();
    let disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        disk.contains("* TODO One"),
        "our edit did not land:\n{disk}"
    );
    assert!(
        disk.contains("body from Emacs"),
        "their edit to the same headline was overwritten:\n{disk}"
    );
}

#[test]
fn a_file_deleted_under_us_does_not_get_resurrected_from_cache() {
    // Writing the whole file from an in-memory copy could recreate a
    // file the user deleted. It must fail instead.
    let (dir, mut v) = vault();
    std::fs::remove_file(dir.path().join("a.org")).unwrap();
    let _ = v.set_todo(&BlockId::from_existing("disk-one"), Some("TODO"));
    assert!(
        !dir.path().join("a.org").exists(),
        "a deleted file came back from the cache"
    );
}

#[test]
fn a_second_mutation_sees_the_first() {
    // Two edits in a row against the same file, with the reparse in
    // between: the second must not resurrect the pre-first text.
    let (dir, mut v) = vault();
    v.set_todo(&BlockId::from_existing("disk-one"), Some("TODO"))
        .unwrap();
    v.set_todo(&BlockId::from_existing("disk-two"), Some("DONE"))
        .unwrap();
    let disk = std::fs::read_to_string(dir.path().join("a.org")).unwrap();
    assert!(disk.contains("* TODO One"), "{disk}");
    assert!(disk.contains("* DONE Two"), "{disk}");
}

#[test]
fn the_write_is_the_whole_file_and_it_round_trips() {
    // Why whole-file: the parse is a check, not a rewrite (I1). What
    // goes to disk is what the parser can read back byte-exact, so a
    // partial splice cannot leave the file half-valid.
    let (dir, mut v) = vault();
    v.set_todo(&BlockId::from_existing("disk-one"), Some("TODO"))
        .unwrap();
    let disk = std::fs::read_to_string(dir.path().join("a.org")).unwrap();
    let reparsed = closure_core::Document::load_str(&disk).expect("still org");
    assert_eq!(reparsed.source(), disk, "the write does not round-trip");
}
