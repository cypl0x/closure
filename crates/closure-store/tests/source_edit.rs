//! Editing a file as text.
//!
//! Every mutation so far went through a kernel command against one
//! block — which is what keeps ids stable and the file byte-exact. A
//! full-window editor over the file itself is the other thing an org
//! user expects: open the buffer, edit anything, save. That needs one
//! seam the store did not have — replace a document's whole source —
//! and it has to keep the same promises the block commands do: the
//! bytes on disk are exactly what was written (I1), and every id in the
//! new text is findable afterwards (I2).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_store::Vault;

const SRC: &str = "* One\n:PROPERTIES:\n:ID: 01HQSRC00000000000000001\n:END:\nbody one\n\
                   * Two\n:PROPERTIES:\n:ID: 01HQSRC00000000000000002\n:END:\nbody two\n";

fn vault() -> (tempfile::TempDir, Vault, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("notes.org");
    fs::write(&path, SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, vault, path)
}

#[test]
fn set_source_writes_the_bytes_it_was_given() {
    let (_d, mut vault, path) = vault();
    let edited = SRC.replace("body one", "body one, edited");
    vault.set_source(&path, &edited).expect("set");
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        edited,
        "byte-exact (I1)"
    );
}

#[test]
fn the_loaded_document_matches_what_was_saved() {
    let (_d, mut vault, path) = vault();
    let edited = SRC.replace("body one", "rewritten");
    vault.set_source(&path, &edited).expect("set");
    let doc = vault
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, d)| d)
        .expect("the document is still loaded");
    assert_eq!(doc.source(), edited, "no reload needed to see the edit");
}

#[test]
fn a_headline_added_as_text_becomes_findable_by_its_id() {
    // I2: the index is rebuilt from the new text, so an id typed into
    // the buffer is a real id the moment it is saved.
    let (_d, mut vault, path) = vault();
    let added =
        format!("{SRC}* Three\n:PROPERTIES:\n:ID: 01HQSRC00000000000000003\n:END:\nbody three\n");
    vault.set_source(&path, &added).expect("set");
    let id = closure_core::BlockId::from_existing("01HQSRC00000000000000003");
    let (headline, found_in) = vault.find_by_id(&id).expect("the new id is indexed");
    assert_eq!(headline.title(), "Three");
    assert_eq!(found_in, path);
}

#[test]
fn a_headline_deleted_as_text_leaves_the_index() {
    let (_d, mut vault, path) = vault();
    let cut = "* One\n:PROPERTIES:\n:ID: 01HQSRC00000000000000001\n:END:\nbody one\n";
    vault.set_source(&path, cut).expect("set");
    let gone = closure_core::BlockId::from_existing("01HQSRC00000000000000002");
    assert!(
        vault.find_by_id(&gone).is_none(),
        "a headline deleted in the buffer is deleted in the index"
    );
}

#[test]
fn the_revision_moves_so_the_shells_repaint() {
    let (_d, mut vault, path) = vault();
    let before = vault.revision();
    vault
        .set_source(&path, &SRC.replace("Two", "Second"))
        .expect("set");
    assert_ne!(vault.revision(), before, "a caller can see that it changed");
}

#[test]
fn set_source_on_an_unknown_file_is_an_error_not_a_new_file() {
    // The editor addresses a file it is *showing*; being handed a path
    // outside the vault means something is wrong, and writing it anyway
    // would scatter org files across the disk.
    let (_d, mut vault, _p) = vault();
    let stray = std::path::Path::new("/tmp/closure-not-in-the-vault.org");
    assert!(vault.set_source(stray, "* Nope\n").is_err());
    assert!(!stray.exists(), "and nothing was written");
}
