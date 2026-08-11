//! "`M-x manual` opens, the pane paints a scrollbar, and `C-d` / `C-u`
//! move nothing — so everything below the first screen of the manual is
//! unreachable."
//!
//! The manual is the shell's answer to "self-documented like Emacs",
//! and all but its first screen could not be read. The cursor was not
//! the problem: `on_manual_key` walks `pane_cursor` with `j`/`k`
//! perfectly well. Nothing was *looking* at it. Every read-only pane
//! paints all of its rows, unwindowed, so the cursor moved behind a
//! view that never followed and the surplus rows fell off the bottom.
//!
//! The outline solved this long ago — `view_window` derives the offset
//! from the selection on every call and holds no scroll state of its
//! own. The panes get the same rule rather than a second one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01HQPANE0000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open the manual — through the keymap, the way a reader does — and
/// walk `n` rows down it.
fn manual_at(app: &mut ModalApp, shell: &mut Shell, n: usize) {
    // Doom binds `SPC h m`.
    app.on_key(shell, "space", false, false, Some(' '));
    app.on_key(shell, "h", false, false, Some('h'));
    app.on_key(shell, "m", false, false, Some('m'));
    for _ in 0..n {
        app.on_key(shell, "j", false, false, Some('j'));
    }
}

#[test]
fn the_manual_is_longer_than_a_screen() {
    // Otherwise there is nothing to scroll and the rest of this file
    // proves nothing.
    let (_d, _s, app) = app();
    assert!(app.manual_rows().len() > 40, "{}", app.manual_rows().len());
}

#[test]
fn the_window_follows_the_cursor_down() {
    let (_d, mut shell, mut app) = app();
    manual_at(&mut app, &mut shell, 40);
    let (offset, rows) = app.pane_window(app.manual_rows(), 20);
    assert!(
        (offset..offset + rows.len()).contains(&app.pane_cursor()),
        "cursor at {} is outside the painted window {offset}..{}",
        app.pane_cursor(),
        offset + rows.len()
    );
}

#[test]
fn it_starts_at_the_top() {
    let (_d, mut shell, mut app) = app();
    manual_at(&mut app, &mut shell, 0);
    let (offset, _) = app.pane_window(app.manual_rows(), 20);
    assert_eq!(offset, 0, "opening a pane scrolled it");
}

#[test]
fn a_pane_that_fits_is_not_windowed() {
    let (_d, _s, app) = app();
    let rows = vec!["a".to_owned(), "b".to_owned()];
    let (offset, out) = app.pane_window(rows.clone(), 20);
    assert_eq!(offset, 0);
    assert_eq!(out, rows);
}

#[test]
fn half_a_page_moves_the_pane_not_the_outline() {
    // `C-d` is bound to `half-page-down`, which only ever moved the
    // outline's selection — in a read-only pane it scrolled a list
    // nobody was looking at.
    let (_d, mut shell, mut app) = app();
    manual_at(&mut app, &mut shell, 0);
    let before_outline = app.selected();
    // `C-d`.
    app.on_key(&mut shell, "d", true, false, None);
    assert!(app.pane_cursor() > 0, "the pane did not move");
    assert_eq!(
        app.selected(),
        before_outline,
        "the outline moved while a pane was open"
    );
}
