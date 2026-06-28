//! P2: the GTK shell is an interactive editor, not a static snapshot. The
//! window feeds each key as a `KeyEvent` to `next_frame`, which dispatches
//! it (P1) and returns the GTK widget descriptor to repaint. Driving the
//! capture chord through `next_frame` edits the vault + the rebuilt widget
//! tree — hermetic, the same step the windowed `run` calls per keypress.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{App, KeyEvent, Shell};
use closure_shell_gtk::next_frame;
use closure_store::Vault;

#[test]
fn key_events_edit_the_vault_and_rebuild_the_widget_tree() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("inbox.org"), "* Existing\n").unwrap();
    let mut sh = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = App::new();

    // The exact KeyEvents the GTK key controller will produce.
    let _ = next_frame(&mut app, &mut sh, &KeyEvent::ctrl("c"));
    for c in "From GTK".chars() {
        let _ = next_frame(&mut app, &mut sh, &KeyEvent::char(c));
    }
    let frame = next_frame(&mut app, &mut sh, &KeyEvent::key("enter"));

    assert!(frame.contains("GtkListBox"), "frame is a widget descriptor: {frame}");
    assert!(frame.contains("From GTK"), "captured headline is rendered: {frame}");
    let on_disk = fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(on_disk.contains("From GTK"), "persisted through the registry (I8)");
}

#[test]
fn next_frame_tracks_navigation() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("n.org"), "* A\n* B\n* C\n").unwrap();
    let mut sh = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = App::new();
    // Down moves the selection — the rebuilt frame marks a different row.
    let f0 = next_frame(&mut app, &mut sh, &KeyEvent::key("escape"));
    let f1 = next_frame(&mut app, &mut sh, &KeyEvent::key("down"));
    assert_ne!(f0, f1, "navigation changes the rendered frame");
}
