//! "folding/unfolding of headlines and properties does not work in
//! editor. I am pretty sure that folding headlines in the editor has
//! worked before :thinking:"
//!
//! It did, from the palette, and nowhere else. `toggle-fold` is bound
//! to `z` and `TAB`, and a buffer owns both: `TAB` expands a tempo
//! snippet or accepts a completion, and `z` opens the editor's own
//! viewport prefix (`zz`, `zt`, `zb`). So the command existed, the
//! folds existed, and there was no key that reached them from inside
//! the text they fold.
//!
//! `z a` is where vim puts it, in the prefix that was already there.
//! `M-TAB` is the spelling for the modes with no NORMAL to press it in,
//! and it survives a buffer for the same reason `C-s` does.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

/// A note whose body is a subtree: a child headline with a drawer, the
/// shape the editor shows since bodies started carrying their children.
const NOTES: &str = "\
* Parent
:PROPERTIES:
:ID: 01HQFOLD00000000000001
:END:
first body line
** Child one
:PROPERTIES:
:ID: 01HQFOLD00000000000002
:END:
child one body
** Child two
:PROPERTIES:
:ID: 01HQFOLD00000000000003
:END:
child two body
";

fn editing(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(mode);
    assert!(app.select_by_id(&shell, "01HQFOLD00000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

/// Which body lines the shells are told to hide.
fn hidden(app: &ModalApp) -> Vec<usize> {
    app.body_hidden_lines()
}

#[test]
fn the_drawers_start_folded() {
    // Already true before this item, and the reason the buffer is
    // readable at all: every child brings four lines of bookkeeping.
    let (_d, _sh, app) = editing(InputMode::Doom);
    assert!(!hidden(&app).is_empty(), "a drawer is hidden on open");
}

#[test]
fn z_a_folds_the_headline_the_cursor_is_in() {
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    let before = hidden(&app).len();
    // Onto the `** Child one` line, then vim's own fold chord.
    let line = app
        .body_buffer()
        .lines()
        .position(|l| l.starts_with("** Child one"))
        .expect("child one");
    app.body_click(line, 0);
    app.on_key(&mut shell, "z", false, false, Some('z'));
    app.on_key(&mut shell, "a", false, false, Some('a'));

    assert!(
        hidden(&app).len() > before,
        "nothing folded: {:?}",
        hidden(&app)
    );
}

#[test]
fn z_a_again_unfolds_it() {
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    let line = app
        .body_buffer()
        .lines()
        .position(|l| l.starts_with("** Child one"))
        .expect("child one");
    app.body_click(line, 0);
    app.on_key(&mut shell, "z", false, false, Some('z'));
    app.on_key(&mut shell, "a", false, false, Some('a'));
    let folded = hidden(&app).len();
    app.on_key(&mut shell, "z", false, false, Some('z'));
    app.on_key(&mut shell, "a", false, false, Some('a'));

    assert!(hidden(&app).len() < folded, "still folded");
}

#[test]
fn the_viewport_prefix_still_works() {
    // `z` is the recentre prefix; taking `za` must not take `zz`.
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    app.set_body_viewport(10);
    app.on_key(&mut shell, "z", false, false, Some('z'));
    app.on_key(&mut shell, "z", false, false, Some('z'));
    // No panic and no fold: the framing chord is untouched.
    assert!(!app.status().contains("folded"), "{}", app.status());
}

#[test]
fn a_mode_with_no_normal_can_fold_too() {
    // Notion and Emacs never leave INSERT, so a bare `z` is the letter
    // z. `M-TAB` is the chord that survives a buffer.
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, mut shell, mut app) = editing(mode);
        let before = hidden(&app).len();
        let line = app
            .body_buffer()
            .lines()
            .position(|l| l.starts_with("** Child one"))
            .expect("child one");
        app.body_click(line, 0);
        app.on_key(&mut shell, "tab", false, true, None);

        assert!(
            hidden(&app).len() > before,
            "{mode:?} could not fold: {:?}",
            hidden(&app)
        );
    }
}

#[test]
fn the_chord_is_bound_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "toggle-fold").is_some(),
            "{mode:?}"
        );
    }
}

#[test]
fn typing_into_a_folded_buffer_drops_the_folds() {
    // A fold is a range of lines; once the lines move the range is a
    // guess, and a fold that hides the wrong text is worse than none.
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    assert!(!hidden(&app).is_empty());
    app.on_key(&mut shell, "x", false, false, Some('x'));
    assert!(hidden(&app).is_empty(), "{:?}", hidden(&app));
}
