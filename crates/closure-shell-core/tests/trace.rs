//! "[Update]: The freeze / lag is still there. So limiting the preview
//! lines hasn't worked. How can I get logs in order to help with
//! debugging?"
//!
//! A fair question and the only useful answer, because the first fix
//! was aimed at a cause that turned out not to be the cause. The
//! microfreeze does not reproduce here — on a synthetic vault shaped
//! like the report, every step of the selection costs the same ~3ms
//! whether it lands on a level-1 headline or a subheading — so
//! guessing again would be worth less than measuring on the machine
//! where it happens.
//!
//! `toggle-trace` turns on a stopwatch around every keypress. Anything
//! slower than a frame is written to the message log with what it was
//! doing: the surface, the level of the row it landed on, and whether
//! the detail had to be derived again. `g M` reads it back, and the
//! log is plain text that can be pasted into an item.
//!
//! Off by default and free when off — a timing that costs something to
//! collect would change the thing it is measuring.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Top
:PROPERTIES:
:ID: 01HQTRACE0000000000001
:END:
body
** Sub one
more body
** Sub two
more body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQTRACE0000000000001"));
    (dir, shell, app)
}

#[test]
fn tracing_is_off_until_it_is_asked_for() {
    let (_d, _sh, app) = fixture();
    assert!(!app.tracing());
}

#[test]
fn the_command_turns_it_on_and_off() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-trace");
    assert!(app.tracing());
    app.run(&mut shell, "toggle-trace");
    assert!(!app.tracing());
}

#[test]
fn turning_it_on_says_where_the_readings_go() {
    // A stopwatch nobody can find the dial of is not an instrument.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-trace");
    let status = app.status().to_lowercase();
    assert!(
        status.contains("messages") || status.contains("g m"),
        "{status}"
    );
}

#[test]
fn a_slow_step_is_recorded_with_what_it_was_doing() {
    // The reading has to name the thing under suspicion, or it is a
    // number with nowhere to go. `note_slow_key` is the seam the shell
    // calls with a real measurement.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-trace");
    app.note_slow_key("k", std::time::Duration::from_millis(950), &shell);
    let log = app.messages().join("\n");
    assert!(log.contains("950"), "the time: {log}");
    assert!(log.contains('k'), "the key: {log}");
    assert!(
        log.contains("level") || log.contains("Browse"),
        "what it landed on: {log}"
    );
}

#[test]
fn a_fast_step_is_not_recorded_at_all() {
    // Otherwise the log is a wall of noise and the one slow frame is
    // invisible in it.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-trace");
    let before = app.messages().len();
    app.note_slow_key("k", std::time::Duration::from_millis(1), &shell);
    assert_eq!(app.messages().len(), before);
}

#[test]
fn nothing_is_recorded_while_tracing_is_off() {
    let (_d, _sh, mut app) = fixture();
    let shell = fixture().1;
    let before = app.messages().len();
    app.note_slow_key("k", std::time::Duration::from_millis(950), &shell);
    assert_eq!(app.messages().len(), before, "off means off");
}

#[test]
fn the_reading_says_whether_the_detail_was_rebuilt() {
    // The suspect this exists to convict or clear: the detail is
    // memoised per selection, and a memo that never hits would make
    // every frame pay for the whole subtree.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-trace");
    app.note_slow_key("k", std::time::Duration::from_millis(950), &shell);
    let log = app.messages().join("\n");
    assert!(log.contains("detail"), "{log}");
}

#[test]
fn every_mode_can_reach_it() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "toggle-trace").is_some(),
            "{mode:?} cannot turn tracing on"
        );
    }
}

// === what this binary was built from ===
//
// "Create a function/command that returns these values or
// alternatively prints them to the stdout/*MESSAGES* buffer."

#[test]
fn the_version_command_names_the_build() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "build-info");
    let log = app.messages().join("\n");
    assert!(
        log.contains(&closure_core::build_info().describe()),
        "the build is not in the log: {log}"
    );
}

#[test]
fn it_names_closure_too_so_the_line_stands_alone() {
    // A bare hash in a message log says nothing about what it is the
    // hash of.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "build-info");
    assert!(app.messages().join("\n").contains("closure"));
}

#[test]
fn it_is_reachable_from_the_palette() {
    // No chord, deliberately: this is something you ask for once while
    // filing a bug, not a key worth spending. `M-x build-info` finds
    // it, and `version` — the word people actually type — resolves to
    // the same command.
    let names = closure_shell_core::palette_command_names();
    assert!(names.contains(&"build-info"), "not in the palette");
    assert_eq!(
        closure_shell_core::canonical_command("version"),
        "build-info"
    );
}
