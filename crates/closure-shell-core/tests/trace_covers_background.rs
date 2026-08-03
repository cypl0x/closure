//! "SPC t T doesn't do anything other than its activation/deactivation
//! message."
//!
//! Right, and the reason is the instrument's own shape. `note_slow_key`
//! times *keystrokes*: the shell records when a key came in, when it
//! finished, and logs the gap if it crossed a frame. The stall the user
//! is chasing does not happen inside a keystroke. It happens on the
//! reload timer, where the live session dials its peers — so the
//! tracer was watching the one place the problem is not, and honestly
//! reported nothing.
//!
//! A tracer that only sees the work you already suspect is not an
//! instrument, it is a confirmation. It now times any named step,
//! keystrokes among them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01TRACE0000000000000000AA\n:END:\nbody\n";

fn app_with_tracing(shell: &mut Shell) -> ModalApp {
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.run(shell, "toggle-trace");
    assert!(app.tracing(), "the toggle did not arm it");
    app
}

#[test]
fn a_slow_background_step_is_traced() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = app_with_tracing(&mut shell);
    let before = app.messages().len();

    app.note_slow_step("session", std::time::Duration::from_millis(80), &shell);

    let after = app.messages();
    assert!(
        after.len() > before,
        "a step that took 80ms was not reported at all"
    );
    assert!(
        after
            .iter()
            .any(|m| m.contains("session") && m.contains("80")),
        "the trace does not name the step or its cost: {after:?}"
    );
}

#[test]
fn a_fast_background_step_is_not_traced() {
    // The log must stay readable: one line per *problem*, not per tick.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = app_with_tracing(&mut shell);
    let before = app.messages().len();
    for _ in 0..50 {
        app.note_slow_step("session", std::time::Duration::from_micros(20), &shell);
    }
    assert_eq!(app.messages().len(), before, "the log filled with noise");
}

#[test]
fn nothing_is_traced_while_the_tracer_is_off() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    assert!(!app.tracing());
    let before = app.messages().len();
    app.note_slow_step("session", std::time::Duration::from_secs(2), &shell);
    assert_eq!(app.messages().len(), before, "traced while switched off");
}

#[test]
fn the_keystroke_path_still_reports() {
    // The old behaviour has to survive: keys were the one thing it did
    // see, and this must not trade one blind spot for another.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = app_with_tracing(&mut shell);
    let before = app.messages().len();
    app.note_slow_key("j", std::time::Duration::from_millis(90), &shell);
    assert!(
        app.messages().len() > before,
        "keystrokes stopped reporting"
    );
}
