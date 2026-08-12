//! Every chord in every mode, against three shapes of vault.
//!
//! `closure-tui` held 1,279 unexecuted lines of 3,316 — the largest
//! hole in a crate this run freezes, and the freeze permits tests
//! because a test is not a feature.
//!
//! The bulk of it is `apply_buffer_command`, a flat one-arm-per-command
//! dispatch reached through `handle_stroke`. Most of those arms had
//! never run. This drives every chord the keymap defines, in every
//! mode, and asserts the one thing worth asserting broadly: it does not
//! panic. What a command *did* belongs in a test per command; that it
//! can be pressed at all is the claim nothing was making.
//!
//! The empty vault is the half that finds things — an arm that indexes
//! the selected headline is fine until there is not one. So is pressing
//! every chord twice: the second press lands in whatever surface the
//! first opened, which is where a mode nobody tested gets its only
//! visitor.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::PathBuf;

use closure_tui::App;

const MODES: &[closure_config::InputMode] = &[
    closure_config::InputMode::Doom,
    closure_config::InputMode::Emacs,
    closure_config::InputMode::Vim,
    closure_config::InputMode::Helix,
    closure_config::InputMode::Notion,
];

fn vault_with(content: &str) -> (tempfile::TempDir, Vec<PathBuf>) {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("notes.org");
    std::fs::write(&f, content).expect("write");
    (d, vec![f])
}

const FULL: &str = "\
* TODO Ship it :work:
SCHEDULED: <2026-06-20 Sat>
:PROPERTIES:
:ID: 01TUICHORD0000000001
:END:
a body line
** A child
:PROPERTIES:
:ID: 01TUICHORD0000000002
:END:
";

/// Every chord the mode's keymap defines, one press each.
fn press_every_chord(mode: closure_config::InputMode, paths: &[PathBuf]) {
    for (chord, _cmd) in closure_tui::mode_bindings(mode) {
        let mut app = App::with_mode(paths.to_vec(), mode);
        // Chords are written as space-separated strokes (`g a`), and
        // `handle_stroke` takes one stroke at a time.
        for stroke in chord.split_whitespace() {
            app.handle_stroke(stroke);
        }
    }
}

#[test]
fn every_chord_in_every_mode_on_a_full_vault() {
    let (_d, paths) = vault_with(FULL);
    for mode in MODES {
        press_every_chord(*mode, &paths);
    }
}

#[test]
fn every_chord_in_every_mode_on_an_empty_vault() {
    // Where "the selected headline" is not one.
    let (_d, paths) = vault_with("");
    for mode in MODES {
        press_every_chord(*mode, &paths);
    }
}

#[test]
fn every_chord_in_every_mode_with_no_files_at_all() {
    // `App::new(Vec::new())` is a shipped case — there is a test for
    // it — so every chord has to survive it too.
    for mode in MODES {
        press_every_chord(*mode, &[]);
    }
}

#[test]
fn pressing_every_chord_twice_lands_in_whatever_it_opened() {
    // The second press goes to the surface the first one opened, which
    // for most of these is the only visitor that mode will ever get.
    let (_d, paths) = vault_with(FULL);
    for (chord, _cmd) in closure_tui::mode_bindings(closure_config::InputMode::Doom) {
        let mut app = App::with_mode(paths.clone(), closure_config::InputMode::Doom);
        for _ in 0..2 {
            for stroke in chord.split_whitespace() {
                app.handle_stroke(stroke);
            }
        }
    }
}

#[test]
fn escape_gets_back_out_of_every_surface() {
    // The property that makes a modal shell usable: whatever a chord
    // opened, Escape leaves. A surface with no way out is a hang the
    // user has to kill the terminal to escape.
    let (_d, paths) = vault_with(FULL);
    for (chord, _cmd) in closure_tui::mode_bindings(closure_config::InputMode::Doom) {
        let mut app = App::with_mode(paths.clone(), closure_config::InputMode::Doom);
        for stroke in chord.split_whitespace() {
            app.handle_stroke(stroke);
        }
        // `ESC`, not `escape`: this shell's stroke vocabulary is the
        // terminal's (`RET`, `ESC`, `C-c`) and the gpui window's is
        // gpui's (`enter`, `escape`). Worth stating in a test, because
        // a chord table shared between them is the thing that makes
        // the difference easy to forget.
        //
        // Twice: some surfaces are two deep (a prompt inside a pane).
        app.handle_stroke("ESC");
        app.handle_stroke("ESC");
        assert_eq!(
            app.mode(),
            closure_tui::AppMode::Browse,
            "`{chord}` opened something Escape does not leave"
        );
    }
}
