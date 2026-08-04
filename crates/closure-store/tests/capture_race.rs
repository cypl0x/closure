//! An item in the vault reads, in full:
//!
//!   ** TODO (title lost in a capture race — please rename)
//!
//! So a capture once produced a headline with no title. This looks for
//! the race that could do that, on the current code, rather than
//! assuming it is still there.
//!
//! A capture writes the *whole* target file from a string it builds:
//! read what is on disk, append the new headline, write it back. Every
//! way that can go wrong loses whatever the other writer put there —
//! or loses the title, if the read and the write disagree about what
//! the file is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::{CaptureTemplate, Vault};

fn template() -> CaptureTemplate {
    CaptureTemplate {
        target: "inbox.org".into(),
        headline_prefix: "TODO ".into(),
        body: String::new(),
    }
}

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("inbox.org"),
        "* Existing\n:PROPERTIES:\n:ID: 01CAPRACE000000000000000\n:END:\nbody\n",
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, vault)
}

#[test]
fn a_capture_keeps_its_title() {
    let (dir, mut vault) = vault();
    vault
        .capture(&template(), "a thought worth keeping")
        .unwrap();
    let text = std::fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(
        text.contains("* TODO a thought worth keeping"),
        "the title is not in the file:\n{text}"
    );
}

#[test]
fn a_capture_with_an_empty_title_is_refused() {
    // The shape the vault item is in: a headline with a prefix and no
    // title. Whatever produced it, the store should not be willing to
    // write one — an entry you cannot see in the outline is an entry
    // you have lost.
    let (_d, mut vault) = vault();
    let made = vault.capture(&template(), "");
    assert!(
        made.is_err(),
        "an empty capture was written; that is the item this test comes from"
    );
}

#[test]
fn a_capture_with_only_whitespace_is_refused() {
    let (_d, mut vault) = vault();
    assert!(vault.capture(&template(), "   ").is_err());
}

#[test]
fn a_capture_survives_the_file_changing_underneath_it() {
    // The race in its plainest form: something else appends to the
    // target between the vault being opened and the capture landing.
    // Both entries have to be there afterwards.
    let (dir, mut vault) = vault();
    std::fs::write(
        dir.path().join("inbox.org"),
        "* Existing\n:PROPERTIES:\n:ID: 01CAPRACE000000000000000\n:END:\nbody\n\
         * From Emacs\n:PROPERTIES:\n:ID: 01CAPRACE111111111111111\n:END:\n",
    )
    .unwrap();
    vault.capture(&template(), "mine").unwrap();
    let text = std::fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(
        text.contains("From Emacs"),
        "the other write was lost:\n{text}"
    );
    assert!(text.contains("* TODO mine"), "my capture was lost:\n{text}");
}

#[test]
fn two_captures_in_a_row_both_survive() {
    let (dir, mut vault) = vault();
    vault.capture(&template(), "first").unwrap();
    vault.capture(&template(), "second").unwrap();
    let text = std::fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(text.contains("* TODO first"), "{text}");
    assert!(text.contains("* TODO second"), "{text}");
}
