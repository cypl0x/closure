//! P1: the unified input→state→view dispatch. `App::dispatch(shell, event)`
//! is the ONE step every shell's window delegates to — apply a typed
//! `KeyEvent` (key + ctrl + typed char) via the mode-aware `on_key`, then
//! return the fresh `ViewTree`. A shell using only `dispatch` can edit the
//! vault; no per-shell key logic. Hermetic — no window.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{App, KeyEvent, Shell, serialize_view};
use closure_store::Vault;

fn shell(dir: &std::path::Path) -> Shell {
    Shell::new(Vault::open(dir).expect("open"))
}

#[test]
fn key_event_constructors_set_the_right_fields() {
    let c = KeyEvent::ctrl("c");
    assert!(c.ctrl && c.key == "c" && c.text.is_none());
    let ch = KeyEvent::char('x');
    assert!(!ch.ctrl && ch.text == Some('x') && ch.key == "x");
    let k = KeyEvent::key("enter");
    assert!(!k.ctrl && k.key == "enter" && k.text.is_none());
}

#[test]
fn dispatch_drives_a_full_capture_through_one_path_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* Existing\n").unwrap();
    let mut sh = shell(dir.path());
    let mut app = App::new();

    // Drive C-c, type "Hello", Enter — entirely through dispatch.
    app.dispatch(&mut sh, &KeyEvent::ctrl("c"));
    for c in "Hello parity".chars() {
        app.dispatch(&mut sh, &KeyEvent::char(c));
    }
    let view = app.dispatch(&mut sh, &KeyEvent::key("enter"));

    // The returned ViewTree reflects the edit…
    assert!(
        serialize_view(&view).contains("Hello parity"),
        "captured headline is in the returned ViewTree"
    );
    // …and it persisted to disk through the registry (I8).
    let on_disk: String = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(on_disk.contains("Hello parity"), "capture hit the vault");
}

#[test]
fn dispatch_returns_the_view_and_reflects_quit() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("n.org"), "* A\n").unwrap();
    let mut sh = shell(dir.path());
    let mut app = App::new();
    let view = app.dispatch(&mut sh, &KeyEvent::ctrl("q"));
    assert!(app.should_quit(), "C-q quits via the same path");
    assert!(
        !serialize_view(&view).is_empty(),
        "still returns a ViewTree"
    );
}

#[test]
fn dispatch_never_panics_on_escape_from_any_surface() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("n.org"), "* A\n").unwrap();
    let mut sh = shell(dir.path());
    // Enter each editing surface, then Escape — all through dispatch.
    for enter in [
        KeyEvent::ctrl("c"), // capture
        KeyEvent::ctrl("a"), // add-sibling
        KeyEvent::ctrl("r"), // rename
        KeyEvent::char('/'), // palette
    ] {
        let mut app = App::new();
        app.dispatch(&mut sh, &enter);
        let _ = app.dispatch(&mut sh, &KeyEvent::key("escape"));
    }
}
