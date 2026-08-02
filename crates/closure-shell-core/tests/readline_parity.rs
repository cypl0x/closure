//! The same readline chords, in the editor and in every prompt.
//!
//! Reported 2026-08-02: "my naive expectation is that in everything
//! that is some input field and we are in the INSERT mode … there is
//! the usual kinda readline Emacsish keybindings available. That's what
//! I use in all of the other applications (terminal, browser, rofi, …).
//! Do we really have to implement this Emacs readline behavior by
//! ourself for every keybinding? … Especially the discrepancy between
//! the editor and the prompt makes it feel unsatisfying."
//!
//! The discrepancy was real: the body editor had word motions
//! (`M-b`-shaped, on ctrl/alt+arrows) and `M-d`, and the one-line
//! fields had neither. This pins them as one set — every chord below
//! does the same thing to a prompt as it does to a buffer in INSERT.
//!
//! (The other half of the question is answered in the vault: there is
//! no host-OS readline to borrow. gpui draws its own text and owns its
//! own key path, which is why every application on that list —
//! terminal, browser, rofi — ships its own copy of this table.)

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn shell() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQRLP0000000000000001\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

/// One chord, as `on_key` takes it.
type Chord = (&'static str, bool, bool);

/// Type `text`, then press `chords`, in the body editor's INSERT.
fn in_the_editor(text: &str, chords: &[Chord]) -> String {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in text.chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    for (key, ctrl, alt) in chords {
        app.on_key(&mut sh, key, *ctrl, *alt, None);
    }
    app.body_buffer().to_owned()
}

/// The same, in the rename prompt.
fn in_a_prompt(text: &str, chords: &[Chord]) -> String {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "rename");
    assert_eq!(app.surface(), ModalSurface::Rename);
    app.on_key(&mut sh, "u", true, false, None); // clear the prefill
    for c in text.chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    for (key, ctrl, alt) in chords {
        app.on_key(&mut sh, key, *ctrl, *alt, None);
    }
    app.field_buffer().to_owned()
}

/// Every chord that must mean the same thing in both, with the text it
/// is applied to and what it should leave behind.
const TABLE: &[(&str, &[Chord], &str, &str)] = &[
    ("C-a then a letter", &[("a", true, false)], "hello", "hello"),
    (
        "C-w kills the word before",
        &[("w", true, false)],
        "keep this",
        "keep ",
    ),
    (
        "Alt+Backspace kills it too",
        &[("backspace", false, true)],
        "keep this",
        "keep ",
    ),
    (
        "C-u kills to the line start",
        &[("u", true, false)],
        "gone",
        "",
    ),
    (
        "C-k from the start kills the line",
        &[("a", true, false), ("k", true, false)],
        "gone",
        "",
    ),
    (
        "M-d kills the word after",
        &[("a", true, false), ("d", false, true)],
        "one two",
        " two",
    ),
    // `M-b` lands on the start of the last word, so the kill that
    // follows takes the one before it — readline's own composition,
    // and the reason these two chords are worth having together.
    (
        "alt+left steps back a word, then kills the one before",
        &[("left", false, true), ("backspace", false, true)],
        "alpha beta",
        "beta",
    ),
    (
        "ctrl+left is the same step",
        &[("left", true, false), ("backspace", false, true)],
        "alpha beta",
        "beta",
    ),
    (
        "alt+right comes back",
        &[
            ("a", true, false),
            ("right", false, true),
            ("k", true, false),
        ],
        "alpha beta",
        "alpha",
    ),
    (
        "C-b then C-d deletes under the cursor",
        &[("b", true, false), ("d", true, false)],
        "abc",
        "ab",
    ),
    (
        "C-e goes back to the end",
        &[("a", true, false), ("e", true, false), ("k", true, false)],
        "unchanged",
        "unchanged",
    ),
];

#[test]
fn every_readline_chord_means_the_same_in_both() {
    for (what, chords, text, want) in TABLE {
        assert_eq!(&in_the_editor(text, chords), want, "editor: {what}");
        assert_eq!(&in_a_prompt(text, chords), want, "prompt: {what}");
    }
}

#[test]
fn the_capture_prompt_is_a_prompt_like_the_others() {
    // It has its own key handler for the capture history, which is
    // exactly the sort of place a chord goes missing.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture");
    for c in "alpha beta".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "left", false, true, None); // to the start of "beta"
    app.on_key(&mut sh, "backspace", false, true, None); // kills "alpha "
    assert_eq!(app.capture_buffer(), "beta");
}
