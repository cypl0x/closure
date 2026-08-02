//! `C-d` / `C-u`: half a screen at a time.
//!
//! Reported 2026-08-02: "ctlr+d/ctlr+u for faster jumping/scrolling".
//!
//! The body editor already had them — the vim engine treats them as
//! scroll motions — but the outline, which is where you spend most of
//! the session, moved one row at a time or jumped to an end. Half a
//! screen is the step between those, and it is the one vim put on
//! these chords.
//!
//! Half of *what* is the shell's answer, not the kernel's: the core
//! knows where the cursor is and the window knows how tall it is, so
//! the window reports its row count the way it already does for the
//! body ([`ModalApp::set_body_viewport`]).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

/// A vault with `n` top-level headlines.
fn fixture(n: usize) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    let mut src = String::new();
    for i in 0..n {
        use std::fmt::Write as _;
        let _ = writeln!(src, "* Row {i:02}");
    }
    fs::write(dir.path().join("notes.org"), src).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn ctrl_d_moves_down_half_a_screen() {
    let (_d, mut sh, mut app) = fixture(60);
    app.set_outline_viewport(20);
    app.select(0, &sh);
    app.run(&mut sh, "half-page-down");
    assert_eq!(app.selected(), 10, "half of twenty");
}

#[test]
fn ctrl_u_moves_back_up_by_the_same_step() {
    let (_d, mut sh, mut app) = fixture(60);
    app.set_outline_viewport(20);
    app.select(30, &sh);
    app.run(&mut sh, "half-page-up");
    assert_eq!(app.selected(), 20);
}

#[test]
fn it_stops_at_the_ends_rather_than_running_off() {
    let (_d, mut sh, mut app) = fixture(12);
    app.set_outline_viewport(20);
    app.select(0, &sh);
    app.run(&mut sh, "half-page-up");
    assert_eq!(app.selected(), 0, "already at the top");
    app.select(11, &sh);
    app.run(&mut sh, "half-page-down");
    assert_eq!(app.selected(), 11, "and at the bottom");
}

#[test]
fn a_window_that_never_says_how_tall_it_is_still_moves() {
    // Same contract as the body viewport: a shell that does not report
    // gets a sane default rather than a motion that does nothing.
    let (_d, mut sh, mut app) = fixture(60);
    app.select(0, &sh);
    app.run(&mut sh, "half-page-down");
    assert!(app.selected() > 0, "it moved: {}", app.selected());
}

#[test]
fn the_chords_are_bound_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert_eq!(
            closure_input::command_for(mode, "C-d"),
            Some("half-page-down"),
            "{mode:?}"
        );
        assert_eq!(
            closure_input::command_for(mode, "C-u"),
            Some("half-page-up"),
            "{mode:?}"
        );
    }
}

#[test]
fn pressing_them_in_the_outline_moves_the_selection() {
    let (_d, mut sh, mut app) = fixture(60);
    app.set_outline_viewport(20);
    app.select(0, &sh);
    app.on_key(&mut sh, "d", true, false, None);
    assert_eq!(app.selected(), 10);
    app.on_key(&mut sh, "u", true, false, None);
    assert_eq!(app.selected(), 0);
}
