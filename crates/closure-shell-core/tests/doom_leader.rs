//! Doom's `SPC` leader, in the buffer as well as the outline.
//!
//! The Doom keymap here was Doom-*flavoured* — `g`-prefixed chords and
//! single letters — but it had no leader, and the leader is the thing a
//! Doom user's hands actually know: `SPC f s` saves, `SPC :` is M-x,
//! `SPC q q` quits, and pressing `SPC` alone brings up which-key.
//!
//! It has to work inside the editor too, which is where a Doom user
//! spends the session: in NORMAL, `SPC` is the leader (evil's own
//! forward-char binding is exactly what Doom takes away from it), and in
//! INSERT it is a space, because there it is prose.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "* One\n:PROPERTIES:\n:ID: 01HQLEAD00000000000000001\n:END:\nbody one\n";

fn fixture(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

/// Feed a chord written the way the keymap writes it (`SPC f s`).
fn chord(app: &mut ModalApp, shell: &mut Shell, chord: &str) {
    for stroke in chord.split(' ') {
        match stroke {
            "SPC" => app.on_key(shell, "space", false, false, Some(' ')),
            "RET" => app.on_key(shell, "enter", false, false, None),
            s => {
                let c = s.chars().next().expect("a stroke");
                app.on_key(shell, &c.to_string(), false, false, Some(c));
            }
        }
    }
}

// === the leader exists, and only where it belongs ===

#[test]
fn doom_binds_a_space_leader() {
    let bound: Vec<&str> = closure_input::mode_keymap(InputMode::Doom)
        .iter()
        .filter(|(c, _)| c.starts_with("SPC "))
        .map(|(_, cmd)| *cmd)
        .collect();
    assert!(bound.len() >= 6, "a leader map, not a token one: {bound:?}");
    for expected in ["palette", "quit", "save-buffer", "toggle-view"] {
        assert!(bound.contains(&expected), "missing {expected}: {bound:?}");
    }
}

#[test]
fn the_other_modes_keep_their_own_keys() {
    // The leader is Doom's. Vim has no `SPC` map, and Notion's space is
    // a space.
    for mode in [
        InputMode::Vim,
        InputMode::Helix,
        InputMode::Notion,
        InputMode::Emacs,
    ] {
        let leader: Vec<&str> = closure_input::mode_keymap(mode)
            .iter()
            .filter(|(c, _)| c.starts_with("SPC "))
            .map(|(c, _)| *c)
            .collect();
        assert!(leader.is_empty(), "{mode:?} grew a leader: {leader:?}");
    }
}

// === which-key sees it ===

#[test]
fn space_alone_offers_the_leader_map() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    chord(&mut app, &mut shell, "SPC");
    assert_eq!(app.pending_chord(), "SPC", "the chord is in flight");
    let offered = app.completions();
    assert!(!offered.is_empty(), "which-key has something to show");
}

// === the leader in the outline ===

#[test]
fn the_leader_runs_commands_from_the_outline() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    chord(&mut app, &mut shell, "SPC :");
    assert_eq!(app.surface(), ModalSurface::Palette, "SPC : is M-x");
}

#[test]
fn the_leader_toggles_the_view() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    chord(&mut app, &mut shell, "SPC t v");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    chord(&mut app, &mut shell, "SPC t v");
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === the leader in the buffer ===

#[test]
fn the_leader_works_in_normal_inside_a_buffer() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    assert_eq!(app.body_mode(), EditorMode::Normal);
    chord(&mut app, &mut shell, "SPC");
    assert_eq!(app.pending_chord(), "SPC", "the buffer answers the leader");
    assert_eq!(
        app.body_buffer(),
        "body one\n",
        "and nothing was typed into it"
    );
}

#[test]
fn space_in_insert_is_a_space() {
    // The one place the leader must keep its hands off: prose.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    chord(&mut app, &mut shell, "i");
    app.on_key(&mut shell, "space", false, false, Some(' '));
    assert_eq!(app.body_buffer(), " body one\n");
    assert_eq!(app.pending_chord(), "", "no chord was started");
}

#[test]
fn a_vim_user_keeps_space_in_the_buffer() {
    // Vim has no leader here, so `SPC` must not swallow a keystroke and
    // must not open a chord that cannot resolve.
    let (_d, mut shell, mut app) = fixture(InputMode::Vim);
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "space", false, false, Some(' '));
    assert_eq!(app.pending_chord(), "", "no leader in vim mode");
    assert_eq!(app.body_buffer(), "body one\n", "and no edit");
}

// === save-buffer ===

#[test]
fn spc_f_s_saves_the_body_buffer() {
    let (dir, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    chord(&mut app, &mut shell, "A");
    app.on_key(&mut shell, "!", false, false, Some('!'));
    // Esc first: in INSERT the leader keeps its hands off, because
    // there a space is a space.
    app.on_key(&mut shell, "escape", false, false, None);
    chord(&mut app, &mut shell, "SPC f s");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("body one!"), "written: {on_disk}");
}

#[test]
fn spc_f_s_saves_the_file_buffer_and_stays_in_it() {
    let (dir, mut shell, mut app) = fixture(InputMode::Doom);
    chord(&mut app, &mut shell, "SPC t v");
    chord(&mut app, &mut shell, "A");
    app.on_key(&mut shell, "!", false, false, Some('!'));
    app.on_key(&mut shell, "escape", false, false, None);
    chord(&mut app, &mut shell, "SPC f s");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.starts_with("* One!"), "written: {on_disk}");
    assert_eq!(app.surface(), ModalSurface::EditFile, "`:w`, not `:wq`");
}

#[test]
fn save_buffer_outside_a_buffer_says_the_vault_is_already_written() {
    // Every bound chord has to do something honest, including this one
    // from the outline, where there is no buffer to save.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "save-buffer");
    assert!(!app.status().is_empty(), "it answers");
    assert_eq!(app.surface(), ModalSurface::Browse);
}
