//! "do we need an alt leader like in Doom Emacs? Doom Emacs maps for
//! example ctlr+space to alt leader. This is useful if you are in Doom
//! keymap mode and in INSERT mode and still wants to run =recent-files=
//! (usually SPC f f). Then you are to run this via C-SPC f f"
//!
//! Yes. Forty-two of Doom's chords start with `SPC`, and in INSERT
//! `SPC` is a space — which is the whole point of INSERT — so the
//! leader half of the keymap simply went away for as long as you were
//! typing. Getting to it meant `Esc`, the chord, and `i` again, and
//! after `Esc` the cursor is not where you left it.
//!
//! So the leader gets a second key that a buffer does not want: `C-SPC`
//! as asked, and `M-SPC` because that is what Doom itself calls
//! `doom-leader-alt-key`. Both open exactly the leader `SPC` opens —
//! the same pending chord, the same which-key panel, the same
//! continuation strokes — because a second door into one room is worth
//! having only if the room is the same one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQLEAD000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(mode);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQLEAD000000000000001"));
    (dir, shell, app)
}

/// A body buffer, in INSERT, with the caret in the text.
fn inserting(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture(mode);
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    (dir, shell, app)
}

/// One stroke of a chord.
fn key(app: &mut ModalApp, shell: &mut Shell, k: &str, ctrl: bool, alt: bool) {
    let text = (k.len() == 1 && !ctrl && !alt).then(|| k.chars().next().expect("one char"));
    app.on_key(shell, k, ctrl, alt, text);
}

#[test]
fn the_alt_leader_reaches_a_leader_chord_from_insert() {
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    let before = app.body_buffer().to_owned();

    key(&mut app, &mut shell, "space", true, false);
    key(&mut app, &mut shell, "f", false, false);
    key(&mut app, &mut shell, "f", false, false);

    assert_eq!(
        app.surface(),
        ModalSurface::Files,
        "SPC f f is recent-files"
    );
    assert_eq!(app.body_buffer(), before, "and nothing was typed");
}

#[test]
fn dooms_own_alt_leader_key_works_too() {
    // `doom-leader-alt-key` is `M-SPC`; the item asks for `C-SPC`. Both,
    // because both are muscle memory for somebody.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    key(&mut app, &mut shell, "space", false, true);
    key(&mut app, &mut shell, "f", false, false);
    key(&mut app, &mut shell, "f", false, false);
    assert_eq!(app.surface(), ModalSurface::Files);
}

#[test]
fn it_opens_the_same_which_key_panel_the_leader_does() {
    // The reason the leader is usable at all: you press it and it tells
    // you what comes next. An alt leader that opened a silent prefix
    // would be a different, worse leader.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    key(&mut app, &mut shell, "space", true, false);
    assert_eq!(app.pending_chord(), "SPC", "the same pending chord");
    assert_eq!(
        app.which_key_pending(),
        "SPC",
        "and the panel knows to open on it"
    );
    assert!(
        app.which_key_groups()
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .any(|(chord, _)| chord.starts_with("SPC f")),
        "with the leader's own entries under it"
    );
}

#[test]
fn a_plain_space_is_still_a_space() {
    // The alt leader exists precisely so that this stays true.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    let before = app.body_buffer().to_owned();
    app.on_key(&mut shell, "space", false, false, Some(' '));
    assert_eq!(
        app.body_buffer().len(),
        before.len() + 1,
        "a space was typed"
    );
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "and no leader opened"
    );
}

#[test]
fn escape_abandons_it_and_leaves_the_text_alone() {
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    let before = app.body_buffer().to_owned();
    key(&mut app, &mut shell, "space", true, false);
    app.on_key(&mut shell, "escape", false, false, None);
    assert!(app.pending_chord().is_empty(), "the chord was dropped");
    assert_eq!(app.body_buffer(), before);
}

#[test]
fn it_works_in_a_file_buffer_too() {
    // Same keymap, same buffer, same reason.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    app.on_key(&mut shell, "i", false, false, Some('i'));
    key(&mut app, &mut shell, "space", true, false);
    key(&mut app, &mut shell, "f", false, false);
    key(&mut app, &mut shell, "f", false, false);
    assert_eq!(app.surface(), ModalSurface::Files);
}

#[test]
fn the_modes_without_a_leader_do_not_grow_one() {
    // Doom is the only keymap with `SPC` chords in it, and `C-SPC` in
    // Emacs is `set-mark` — a mode that has no leader must not have an
    // alt leader either.
    for mode in [InputMode::Emacs, InputMode::Vim, InputMode::Helix] {
        let (_d, mut shell, mut app) = inserting(mode);
        key(&mut app, &mut shell, "space", true, false);
        assert!(
            app.pending_chord().is_empty(),
            "{mode:?} opened a leader it does not have"
        );
    }
}

#[test]
fn a_half_typed_alt_leader_does_not_eat_the_next_letter() {
    // `C-SPC` then something unbound must leave the buffer as it was,
    // not swallow a keystroke into a chord that went nowhere.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    let before = app.body_buffer().to_owned();
    key(&mut app, &mut shell, "space", true, false);
    key(&mut app, &mut shell, "z", false, false);
    assert_eq!(app.body_buffer(), before, "nothing typed");
    assert!(app.pending_chord().is_empty(), "and the chord is over");
}
