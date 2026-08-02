//! "editor view headre doesn't show prominent headline (we have so
//! many colors and font variants, why not use them?) nor ID or other
//! helpful metadata for the currently editing subtree of this headline
//! — Like lines, words, indentation level, mtime, created at" and
//! "Please make this header more appealing. Don't just use this dimmed
//! color."
//!
//! The header showed the title, a `+ todo` placeholder, the tags, the
//! property drawer and the path. Everything below the title was one
//! dimmed grey, and none of it was the thing you actually want to know
//! about the note you are looking at: how big it is, how deep it sits,
//! when it was written.
//!
//! Two of those are free and nobody was taking them. A closure id *is*
//! a ULID, and a ULID's first ten characters are the millisecond it
//! was minted — so "created at" needs no new field in anybody's file.
//! The file's mtime is a `stat` away.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

/// A real ULID. Its first ten characters decode to
/// 2024-04-17T23:00:02.370Z — checked against an independent decoder,
/// not assumed.
const ID: &str = "01HVQ4KQP2ABCDEFGHJKMNPQRS";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!(
            "* Parent\n\
             :PROPERTIES:\n:ID: 01HVQ4KQP1ZZZZZZZZZZZZZZZZ\n:END:\n\
             ** Child note\n\
             :PROPERTIES:\n:ID: {ID}\n:END:\n\
             one two three\n\
             four five\n"
        ),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, ID));
    (dir, shell, app)
}

#[test]
fn the_detail_knows_which_note_it_is() {
    // "nor ID": the pane showed the whole property drawer as grey text
    // and had no id of its own to hand a shell that wants to style it.
    let (_d, shell, app) = fixture();
    let d = app.selected_detail(&shell).expect("a detail");
    assert_eq!(d.id, ID);
}

#[test]
fn it_knows_how_deep_it_sits() {
    let (_d, shell, app) = fixture();
    let d = app.selected_detail(&shell).expect("a detail");
    assert_eq!(d.level, 2, "a child of a top-level headline");
}

#[test]
fn it_counts_its_own_lines_and_words() {
    let (_d, shell, app) = fixture();
    let d = app.selected_detail(&shell).expect("a detail");
    assert_eq!(d.lines, 2, "two lines of body");
    assert_eq!(d.words, 5, "one two three four five");
}

#[test]
fn an_empty_note_counts_nothing_rather_than_one_blank_line() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* Empty\n:PROPERTIES:\n:ID: {ID}\n:END:\n"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, ID));
    let d = app.selected_detail(&shell).expect("a detail");
    assert_eq!((d.lines, d.words), (0, 0));
}

#[test]
fn created_at_comes_out_of_the_id_itself() {
    // A closure id is a ULID and a ULID carries the millisecond it was
    // minted, so "created at" costs nothing and needs no new property
    // in anybody's file.
    let (_d, shell, app) = fixture();
    let d = app.selected_detail(&shell).expect("a detail");
    assert_eq!(d.created.as_deref(), Some("2024-04-17"), "{:?}", d.created);
}

#[test]
fn the_epoch_and_the_specs_own_example_decode() {
    // Two fixed points, so a wrong answer cannot look plausible: all
    // zeroes is the epoch, and `01BX5ZZKBK…` is the example in the
    // ULID spec itself, minted 2017-10-24.
    assert_eq!(
        closure_shell_core::ulid_date("00000000000000000000000000").as_deref(),
        Some("1970-01-01")
    );
    assert_eq!(
        closure_shell_core::ulid_date("01BX5ZZKBKACTAV9WEVGEMMVRZ").as_deref(),
        Some("2017-10-24")
    );
}

#[test]
fn an_id_that_is_not_a_ulid_has_no_creation_date() {
    // Ids from before closure minted them, or hand-written ones.
    assert_eq!(closure_shell_core::ulid_date("not-a-ulid"), None);
    assert_eq!(closure_shell_core::ulid_date(""), None);
}

#[test]
fn the_file_says_when_it_was_last_touched() {
    let (_d, shell, app) = fixture();
    let d = app.selected_detail(&shell).expect("a detail");
    assert!(d.modified.is_some(), "the file was written a moment ago");
}
