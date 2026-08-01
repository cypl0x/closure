//! Zoom is a command, not a chord the editor happens to know.
//!
//! The buffer's text scale was reachable only from inside a body
//! buffer, by a `ctrl` match hardcoded in the editor's key path: no
//! command name, no keymap entry, nothing in M-x. So the outline could
//! not zoom, which-key could not advertise it, and the keymap — the
//! single source of truth for chords (I4) — did not know it existed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, command_palette};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQZOOM000000000000000001
:END:
body text
";

const MODES: [InputMode; 5] = [
    InputMode::Doom,
    InputMode::Vim,
    InputMode::Emacs,
    InputMode::Helix,
    InputMode::Notion,
];

fn fixture(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

#[test]
fn the_outline_zooms_too() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    assert_eq!(app.surface(), ModalSurface::Browse);
    let plain = app.zoom();
    app.on_key(&mut shell, "=", true, false, None);
    assert!(app.zoom() > plain, "C-= zooms in from the outline");
    app.on_key(&mut shell, "-", true, false, None);
    assert!(
        (app.zoom() - plain).abs() < 0.001,
        "C-- undoes it: {}",
        app.zoom()
    );
    app.on_key(&mut shell, "=", true, false, None);
    app.on_key(&mut shell, "0", true, false, None);
    assert!((app.zoom() - 1.0).abs() < f32::EPSILON, "C-0 resets");
}

#[test]
fn the_chords_are_the_keymap_s_in_every_mode() {
    // Not a hardcoded `ctrl` match: the chord comes from the mode's
    // keymap, so which-key and the palette can show it (I4).
    for mode in MODES {
        for command in ["zoom-in", "zoom-out", "zoom-reset"] {
            let chord = closure_input::chord_for_command(mode, command);
            assert!(chord.is_some(), "{mode:?} binds {command}");
        }
        let (_d, mut shell, mut app) = fixture(mode);
        app.on_key(&mut shell, "=", true, false, None);
        assert!(app.zoom() > 1.0, "{mode:?} zooms on its own chord");
    }
}

#[test]
fn the_buffer_still_zooms_from_inside_the_editor() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    assert!(app.surface().is_editor());
    app.on_key(&mut shell, "=", true, false, None);
    assert!(app.zoom() > 1.0, "the chord reaches the buffer's own path");
    app.on_key(&mut shell, "0", true, false, None);
    assert!((app.zoom() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn zooming_says_what_it_did() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "zoom-in");
    assert!(app.status().contains("110%"), "{}", app.status());
    app.run(&mut shell, "zoom-reset");
    assert!(app.status().contains("100%"), "{}", app.status());
}

#[test]
fn m_x_offers_them() {
    let sections = command_palette("zoom", InputMode::Doom);
    let labels: Vec<&str> = sections
        .iter()
        .flat_map(|s| s.items.iter())
        .map(|e| e.label.as_str())
        .collect();
    for label in ["zoom-in", "zoom-out", "zoom-reset"] {
        assert!(labels.contains(&label), "{label} in M-x: {labels:?}");
    }
    for e in sections.iter().flat_map(|s| s.items.iter()) {
        assert!(!e.action.chord().is_empty(), "{} shows its chord", e.label);
    }
}
