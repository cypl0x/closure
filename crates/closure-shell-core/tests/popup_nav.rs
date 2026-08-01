//! Walking a popup's list without leaving the home row.
//!
//! Doom binds `C-j`/`C-k` alongside `C-n`/`C-p` wherever a popup is
//! showing — company's active map, vertico's minibuffer — because the
//! hand that types `j`/`k` in NORMAL wants the same pair when a list is
//! in front of it. closure had `C-n`/`C-p` on the completion popup and
//! nothing but the arrow keys on the palette.
//!
//! `C-k` is also readline's kill-to-end-of-line, and that is not
//! negotiable: it only walks a list while a list is actually open,
//! which is exactly the scope company's map has.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

/// A vault with two words sharing a prefix, so a completion cycle has
/// somewhere to go.
fn shell() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* alpha one\n* alphabet two\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

/// The body editor, in INSERT, with `text` typed into it.
fn typing(sh: &mut Shell, text: &str) -> ModalApp {
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(sh, "i", false, false, Some('i')); // open the body
    app.on_key(sh, "i", false, false, Some('i')); // INSERT
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app
}

// === The command palette ===

#[test]
fn ctrl_j_and_k_walk_the_palette() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "palette");
    assert_eq!(app.surface(), ModalSurface::Palette);
    assert!(app.palette_entries().len() > 2, "there is a list to walk");
    app.on_key(&mut sh, "j", true, false, None);
    assert_eq!(app.palette_cursor(), 1, "C-j is down");
    app.on_key(&mut sh, "j", true, false, None);
    assert_eq!(app.palette_cursor(), 2);
    app.on_key(&mut sh, "k", true, false, None);
    assert_eq!(app.palette_cursor(), 1, "C-k is up");
}

#[test]
fn ctrl_n_and_p_walk_the_palette_too() {
    // The Emacs pair is as dead here as the Doom one was, and the
    // palette is a minibuffer by another name.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.run(&mut sh, "palette");
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.palette_cursor(), 1, "C-n is down");
    app.on_key(&mut sh, "p", true, false, None);
    assert_eq!(app.palette_cursor(), 0, "C-p is up");
}

#[test]
fn walking_the_palette_stops_at_the_ends() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "palette");
    app.on_key(&mut sh, "k", true, false, None);
    assert_eq!(app.palette_cursor(), 0, "no wrap off the top");
    let last = app.palette_entries().len() - 1;
    for _ in 0..=last + 3 {
        app.on_key(&mut sh, "j", true, false, None);
    }
    assert_eq!(app.palette_cursor(), last, "and none off the bottom");
}

#[test]
fn a_ctrl_chord_leaves_the_palette_filter_alone() {
    // Whatever the platform hands us as `key_char`, a modified letter
    // is a chord and not a character to filter by.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "palette");
    app.on_key(&mut sh, "j", true, false, Some('j'));
    assert_eq!(app.field_buffer(), "", "C-j typed nothing");
}

// === The body-editor completion popup ===

#[test]
fn ctrl_j_and_k_walk_the_completion_popup() {
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "alph");
    app.open_completion_popup(&sh);
    assert!(!app.body_completion_items().is_empty(), "the popup is up");
    app.on_key(&mut sh, "j", true, false, None);
    assert_eq!(app.body_buffer(), "alpha", "C-j takes the first");
    app.on_key(&mut sh, "j", true, false, None);
    assert_eq!(app.body_buffer(), "alphabet");
    app.on_key(&mut sh, "k", true, false, None);
    assert_eq!(app.body_buffer(), "alpha", "C-k goes back");
}

#[test]
fn ctrl_k_still_kills_to_end_of_line_with_no_popup_open() {
    // company's map is only live while the popup is: the readline
    // chord underneath it has to survive this change untouched.
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "alpha beta");
    assert!(app.body_completion_items().is_empty(), "no popup");
    app.on_key(&mut sh, "a", true, false, None); // C-a, line home
    app.on_key(&mut sh, "k", true, false, None); // C-k, kill to end
    assert_eq!(app.body_buffer(), "", "the line was killed, not walked");
}

#[test]
fn ctrl_k_kills_the_line_again_once_the_popup_is_gone() {
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "alph");
    app.open_completion_popup(&sh);
    app.on_key(&mut sh, "escape", false, false, None); // popup away
    app.on_key(&mut sh, "i", false, false, Some('i')); // back to INSERT
    app.on_key(&mut sh, "a", true, false, None);
    app.on_key(&mut sh, "k", true, false, None);
    assert_eq!(app.body_buffer(), "");
}
