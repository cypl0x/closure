//! "messages enter should copy the selected line to system clipboard
//! an internal 'clipboard' (kill-ring?)"
//!
//! The message log is where a trace reading, an error or a saved-file
//! line ends up — all things you want to paste somewhere else, which
//! is the entire reason to open it. `RET` on a row did nothing at all:
//! the picker's accept had no case for this surface, so the one
//! gesture that means "take this" was silent.
//!
//! It goes to the same register the editor's `y`/`d`/`C-k` use, which
//! is the seam the system-clipboard mirror already watches — so one
//! assignment puts it in both places rather than growing a second
//! clipboard beside the first.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01MSGYANK00000000000000AA\n:END:\nbody\n";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

#[test]
fn enter_on_a_message_takes_it() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "build-info");
    let expected = app.messages().first().cloned().expect("a message");
    app.run(&mut shell, "messages");
    assert_eq!(app.surface(), ModalSurface::Messages);

    app.on_key(&mut shell, "enter", false, false, None);

    assert_eq!(
        app.register_text(),
        expected,
        "the selected line did not reach the register"
    );
}

#[test]
fn taking_a_message_bumps_the_generation_the_clipboard_watches() {
    // The mirror only pushes when the generation moves; setting the
    // text without bumping it would copy internally and silently not
    // reach the system clipboard, which is half the request.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "build-info");
    app.run(&mut shell, "messages");
    let before = app.register_generation();
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.register_generation() > before,
        "the system clipboard was never told"
    );
}

#[test]
fn taking_a_message_says_so() {
    // Silence after a copy is indistinguishable from a key that did
    // nothing — which is what the report was about.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "build-info");
    app.run(&mut shell, "messages");
    let before = app.messages().len();
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.messages().len() > before, "no confirmation");
}

#[test]
fn enter_on_an_empty_log_does_nothing_bad() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "messages");
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.register_text().is_empty());
}
