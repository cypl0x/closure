//! "jump to bottom/start when you are at the end or start and want to
//! go beyond the limit it should 'overflow'".
//!
//! Every popup list stopped dead at both ends, so reaching the last
//! entry from the first meant holding a key down the whole way — and
//! the entry one *past* the end, which is the one you were reaching
//! for, took the full trip back. Every completion popup worth using
//! wraps; these do now.
//!
//! The outline does not, deliberately. It is a document, not a
//! candidate list: `j` at the last headline jumping to the first would
//! lose your place in the thing you are reading.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQWRAP00000000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQWRAP00000000000000002
:END:
beta body
* Gamma
:PROPERTIES:
:ID: 01HQWRAP00000000000000003
:END:
gamma body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn down(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "down", false, false, None);
}

fn up(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "up", false, false, None);
}

#[test]
fn the_palette_wraps_backwards_off_the_top() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let last = app.palette_entries().len() - 1;
    assert_eq!(app.palette_cursor(), 0, "starts at the top");

    up(&mut app, &mut shell);
    assert_eq!(app.palette_cursor(), last, "up from the top is the bottom");
}

#[test]
fn the_palette_wraps_forwards_off_the_bottom() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let n = app.palette_entries().len();
    for _ in 0..n - 1 {
        down(&mut app, &mut shell);
    }
    assert_eq!(app.palette_cursor(), n - 1, "at the bottom");

    down(&mut app, &mut shell);
    assert_eq!(app.palette_cursor(), 0, "down from the bottom is the top");
}

#[test]
fn the_chords_wrap_too() {
    // `C-p` and `C-k` are the same gesture as `up` and must not stop
    // where the arrow no longer does.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let last = app.palette_entries().len() - 1;
    app.on_key(&mut shell, "p", true, false, None);
    assert_eq!(app.palette_cursor(), last);
}

#[test]
fn a_search_wraps_at_both_ends() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "search");
    for c in "a".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let n = app.rows(&shell).len();
    assert!(n > 1, "several matches to walk: {n}");

    up(&mut app, &mut shell);
    assert_eq!(app.selected(), n - 1, "up from the first is the last");
    down(&mut app, &mut shell);
    assert_eq!(app.selected(), 0, "and back round");
}

#[test]
fn the_file_picker_wraps() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "recent-files");
    let n = app.file_rows(&shell).len();
    if n < 2 {
        return; // nothing to wrap around in this vault
    }
    up(&mut app, &mut shell);
    assert_eq!(app.selected(), n - 1);
}

#[test]
fn the_outline_does_not_wrap() {
    // A document, not a candidate list: `k` at the top must stay at the
    // top, or reading one loses your place.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQWRAP00000000000000001"));
    app.run(&mut shell, "prev-file");
    assert_eq!(app.selected(), 0, "still on the first headline");

    app.run(&mut shell, "last-file");
    let last = app.rows(&shell).len() - 1;
    app.run(&mut shell, "next-file");
    assert_eq!(app.selected(), last, "and still on the last");
}

#[test]
fn a_one_entry_list_stays_where_it_is() {
    // Wrapping a single candidate must not divide by its own length.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    for c in "zoom-reset".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(app.palette_entries().len(), 1, "one match");
    down(&mut app, &mut shell);
    assert_eq!(app.palette_cursor(), 0);
    up(&mut app, &mut shell);
    assert_eq!(app.palette_cursor(), 0);
}

#[test]
fn an_empty_list_does_not_panic() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    for c in "zzzznothingmatches".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(app.palette_entries().is_empty());
    down(&mut app, &mut shell);
    up(&mut app, &mut shell);
    assert_eq!(app.palette_cursor(), 0);
}
