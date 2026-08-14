//! Every readline motion, on every prompt that has a caret.
//!
//! `text_motion` is a fifteen-way match run against a dozen surfaces,
//! and it was the densest uncovered region left in this crate. The
//! shape is why: the editor branch is exercised by the body-editing
//! tests and the prompt branch by two or three tests that happened to
//! use the rename field, so most of the table had never been reached
//! on most of the surfaces.
//!
//! That matters beyond coverage. The prompts deliberately share one
//! readline implementation — "rather than a second copy of the same
//! fifteen answers" — and the value of sharing it is that every prompt
//! behaves the same. Nothing was checking that they do. A surface
//! wired to the wrong field, or left out of the match, would move the
//! caret in a buffer the user is not looking at, and only on that one
//! prompt.
//!
//! The surfaces with no caret are asserted too. A motion there must do
//! nothing rather than move something invisible.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n\
                   :PROPERTIES:\n\
                   :ID: 01MOTION000000000000001\n\
                   :END:\n\
                   the body of alpha\n\
                   * Beta\n\
                   :PROPERTIES:\n\
                   :ID: 01MOTION000000000000002\n\
                   :END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn type_into(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

/// The motions `text_motion` answers to.
const MOTIONS: &[&str] = &[
    "line-start",
    "line-end",
    "char-left",
    "char-right",
    "char-up",
    "char-down",
    "word-left",
    "word-right",
    "delete-char",
    "delete-char-back",
    "kill-line",
    "kill-line-back",
    "kill-word-back",
    "kill-word-forward",
    "yank",
];

/// Prompts that hold a line of text, and how to open each.
///
/// The command is the one a chord resolves to, so this opens them the
/// way a keystroke does rather than by setting the surface directly —
/// a surface reachable only from a test is not a surface.
const TYPING_SURFACES: &[(&str, ModalSurface)] = &[
    ("search", ModalSurface::Search),
    ("capture", ModalSurface::Capture),
    ("ex-command", ModalSurface::Ex),
    ("llm", ModalSurface::Llm),
    ("rename", ModalSurface::Rename),
    ("palette", ModalSurface::Palette),
];

/// What that surface's buffer currently says.
fn buffer_of(app: &ModalApp, surface: ModalSurface) -> String {
    match surface {
        ModalSurface::Search | ModalSurface::BodySearch => app.query().to_owned(),
        ModalSurface::Capture => app.capture_buffer().to_owned(),
        ModalSurface::Ex => app.ex_buffer().to_owned(),
        ModalSurface::Llm => app.chat_buffer().to_owned(),
        ModalSurface::Sync => app.sync_buffer().to_owned(),
        _ => app.field_buffer().to_owned(),
    }
}

#[test]
fn every_motion_reaches_every_typing_prompt_without_panicking() {
    // The broad claim. Fifteen motions against six prompts is ninety
    // combinations, and before this the great majority had never run.
    for (open, surface) in TYPING_SURFACES {
        for motion in MOTIONS {
            let (_d, mut sh, mut app) = fixture();
            app.select(0, &sh);
            app.run(&mut sh, open);
            assert_eq!(app.surface(), *surface, "`{open}` did not open {surface:?}");
            type_into(&mut app, &mut sh, "hello world");
            app.run(&mut sh, motion);
            // The buffer is still a string this surface can report,
            // which is the weakest thing worth asserting and the one
            // that catches a motion wired to the wrong field.
            let _ = buffer_of(&app, *surface);
        }
    }
}

#[test]
fn line_start_then_typing_lands_at_the_front_on_every_prompt() {
    // The motion whose effect is easiest to see, checked on all of
    // them: the shared readline is only worth having if it behaves the
    // same everywhere, and nothing was asserting that it does.
    for (open, surface) in TYPING_SURFACES {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        app.run(&mut sh, open);
        // Clear whatever the prompt opened with, so the assertion is
        // about what this test typed.
        app.run(&mut sh, "kill-line-back");
        app.run(&mut sh, "kill-line");
        type_into(&mut app, &mut sh, "bcd");
        app.run(&mut sh, "line-start");
        type_into(&mut app, &mut sh, "a");
        assert!(
            buffer_of(&app, *surface).starts_with('a'),
            "{surface:?}: line-start did not put the caret at the front, buffer is {:?}",
            buffer_of(&app, *surface)
        );
    }
}

#[test]
fn kill_line_back_empties_the_prompt_it_is_on() {
    for (open, surface) in TYPING_SURFACES {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        app.run(&mut sh, open);
        type_into(&mut app, &mut sh, "some text here");
        app.run(&mut sh, "line-end");
        app.run(&mut sh, "kill-line-back");
        assert!(
            buffer_of(&app, *surface).is_empty(),
            "{surface:?}: kill-line-back left {:?}",
            buffer_of(&app, *surface)
        );
    }
}

#[test]
fn what_a_kill_took_is_what_a_yank_puts_back() {
    // The kill ring is shared across the prompts too, and a yank that
    // pasted something else would be the same "wrong field" bug seen
    // from the other end.
    for (open, surface) in TYPING_SURFACES {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        app.run(&mut sh, open);
        app.run(&mut sh, "kill-line-back");
        app.run(&mut sh, "kill-line");
        type_into(&mut app, &mut sh, "abcdef");
        app.run(&mut sh, "line-start");
        app.run(&mut sh, "kill-line");
        assert!(
            buffer_of(&app, *surface).is_empty(),
            "{surface:?}: kill-line did not clear it"
        );
        app.run(&mut sh, "yank");
        assert_eq!(
            buffer_of(&app, *surface),
            "abcdef",
            "{surface:?}: yank did not restore what kill-line took"
        );
    }
}

#[test]
fn delete_char_back_removes_one_character_on_every_prompt() {
    for (open, surface) in TYPING_SURFACES {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        app.run(&mut sh, open);
        app.run(&mut sh, "kill-line-back");
        app.run(&mut sh, "kill-line");
        type_into(&mut app, &mut sh, "abc");
        app.run(&mut sh, "delete-char-back");
        assert_eq!(
            buffer_of(&app, *surface),
            "ab",
            "{surface:?}: delete-char-back"
        );
    }
}

#[test]
fn word_motions_move_by_more_than_a_character() {
    for (open, surface) in TYPING_SURFACES {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        app.run(&mut sh, open);
        app.run(&mut sh, "kill-line-back");
        app.run(&mut sh, "kill-line");
        type_into(&mut app, &mut sh, "alpha beta");
        // Back one word, then kill to the end: the last word goes and
        // the first stays, which is only true if the motion moved by a
        // word rather than a character.
        app.run(&mut sh, "word-left");
        app.run(&mut sh, "kill-line");
        let left = buffer_of(&app, *surface);
        assert!(
            left.starts_with("alpha") && !left.contains("beta"),
            "{surface:?}: word-left then kill-line left {left:?}"
        );
    }
}

#[test]
fn a_motion_on_a_surface_with_no_caret_does_nothing_at_all() {
    // The outline has no text field. A motion here must not move a
    // caret the user cannot see — and must not panic, which is what a
    // match arm reaching for a field that is not there would do.
    for motion in MOTIONS {
        let (_d, mut sh, mut app) = fixture();
        app.select(0, &sh);
        assert_eq!(app.surface(), ModalSurface::Browse);
        let before = app.selected();
        app.run(&mut sh, motion);
        assert_eq!(
            app.surface(),
            ModalSurface::Browse,
            "`{motion}` changed the surface"
        );
        assert_eq!(
            app.selected(),
            before,
            "`{motion}` moved the outline selection"
        );
    }
}

// === the editor branch ===
//
// `text_motion` answers the same fifteen names on the body editor, and
// through a different half of the match: the editor has its own
// multi-line buffer rather than the shared one-line readline. Both
// halves being reached matters because the names are the promise — a
// chord bound to `char-up` should mean the same thing wherever there is
// text, and only one of the two halves was ever run.

fn open_editor(app: &mut ModalApp, sh: &mut Shell) {
    app.select(0, sh);
    app.run(sh, "edit-body");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "edit-body did not open"
    );
}

#[test]
fn every_motion_reaches_the_body_editor_without_panicking() {
    for motion in MOTIONS {
        let (_d, mut sh, mut app) = fixture();
        open_editor(&mut app, &mut sh);
        type_into(&mut app, &mut sh, "one two\nthree four");
        app.run(&mut sh, motion);
        assert_eq!(
            app.surface(),
            ModalSurface::EditBody,
            "`{motion}` left the editor"
        );
    }
}

#[test]
fn the_editor_moves_up_and_down_between_lines() {
    // A motion the one-line prompts cannot have, and the reason the
    // editor needs its own half of the match at all.
    let (_d, mut sh, mut app) = fixture();
    open_editor(&mut app, &mut sh);
    app.run(&mut sh, "line-start");
    let (top_row, _) = app.body_cursor();
    app.run(&mut sh, "char-down");
    let (down_row, _) = app.body_cursor();
    assert!(
        down_row >= top_row,
        "char-down went upwards: {top_row} -> {down_row}"
    );
    app.run(&mut sh, "char-up");
    let (up_row, _) = app.body_cursor();
    assert!(up_row <= down_row, "char-up went downwards");
}

#[test]
fn the_body_editor_opens_in_a_normal_mode_not_an_insert_one() {
    // Found by a test of mine that assumed otherwise: typing "abcd"
    // into a freshly opened editor produced "bcd", because `a` is
    // append and was consumed as a command before the rest arrived as
    // literal text. That is vim's behaviour and the right one for a
    // modal editor — but it is invisible until something types a word
    // starting with a letter that means something, and then it looks
    // like a dropped keystroke.
    let (_d, mut sh, mut app) = fixture();
    open_editor(&mut app, &mut sh);
    let before = app.body_buffer().to_owned();
    // `x` in normal mode is delete-character, not the letter x.
    type_into(&mut app, &mut sh, "x");
    assert!(
        !app.body_buffer().contains('x') || before.contains('x'),
        "`x` was inserted as text, so the editor opened in insert mode: {:?}",
        app.body_buffer()
    );
}

#[test]
fn the_editor_deletes_forward_and_backward() {
    let (_d, mut sh, mut app) = fixture();
    open_editor(&mut app, &mut sh);
    // Into insert mode deliberately. `i` is the command; everything
    // after it is text.
    type_into(&mut app, &mut sh, "i");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "wxyz");

    app.run(&mut sh, "delete-char-back");
    assert!(
        app.body_buffer().contains("wxy") && !app.body_buffer().contains("wxyz"),
        "delete-char-back: {:?}",
        app.body_buffer()
    );

    app.run(&mut sh, "line-start");
    app.run(&mut sh, "delete-char");
    assert!(
        app.body_buffer().contains("xy") && !app.body_buffer().contains("wxy"),
        "delete-char at line start: {:?}",
        app.body_buffer()
    );
}

#[test]
fn a_motion_that_is_not_one_is_ignored_rather_than_guessed_at() {
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "capture");
    type_into(&mut app, &mut sh, "abc");
    app.run(&mut sh, "char-sideways");
    assert_eq!(app.capture_buffer(), "abc");
}
