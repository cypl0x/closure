//! X1a hermetic core: the webview's HTML payload renders the vault.
//! Runs in the default build (no webview toolchain) — the window itself
//! is display-bound and exercised under `nix develop .#webview`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

fn vault(files: &[(&str, &str)]) -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(d.path().join(name), body).expect("write");
    }
    d
}

#[test]
fn page_renders_vault_headlines_as_html() {
    let td = vault(&[("notes.org", "* Ship the native shell\n* Second note\n")]);
    let html = closure_shell_tauri::page(td.path()).expect("page");
    assert!(html.contains("<html"), "is an HTML document");
    assert!(html.contains("Ship the native shell"), "renders headline");
    assert!(html.contains("Second note"));
}

#[test]
fn page_on_empty_vault_is_valid_html() {
    let td = vault(&[]);
    let html = closure_shell_tauri::page(td.path()).expect("page");
    assert!(html.contains("<html"));
}
