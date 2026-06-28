//! P4: the Tauri webview hosts the LIVE editor surface (the served root
//! page with its capture form), not the read-only export snapshot. The
//! interactive page round-trips edits to the registry (I8) via the web
//! shell's `respond`; the windowed `run` serves it and loads the URL.
//! Hermetic — the page content is verifiable without a window or socket.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_tauri::{interactive_page, page};

#[test]
fn hosts_the_interactive_capture_surface() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* A\n").unwrap();
    let live = interactive_page(dir.path()).unwrap();
    assert!(live.contains("/capture"), "live page exposes the capture form: {live}");
    assert!(live.contains("<form"), "interactive, not a read-only snapshot");
}

#[test]
fn interactive_surface_differs_from_the_static_export() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* A\n").unwrap();
    let live = interactive_page(dir.path()).unwrap();
    let static_export = page(dir.path()).unwrap();
    assert_ne!(live, static_export, "live editor != read-only export");
}
