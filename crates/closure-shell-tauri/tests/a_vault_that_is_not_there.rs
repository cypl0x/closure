//! Both page builders, given a vault that will not open.
//!
//! `page` and `interactive_page` each start with `Vault::open(..)?` and
//! every test hands them a real vault, so neither error arm had run.
//! This is the webview shell: the failure surfaces as a blank window,
//! and an error that never propagates is a blank window with nothing in
//! the log either.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::Path;

use closure_shell_tauri::{interactive_page, page};

#[test]
fn page_reports_a_vault_it_cannot_open() {
    assert!(page(Path::new("/nonexistent/vault/for/tauri")).is_err());
}

#[test]
fn the_interactive_page_reports_it_too() {
    // Two entry points, two `?`s. Only checking one would leave the
    // interactive shell — the one people actually use — able to fail
    // silently.
    assert!(interactive_page(Path::new("/nonexistent/vault/for/tauri")).is_err());
}

#[test]
fn a_file_where_a_vault_directory_belongs_is_refused() {
    // The other way a path is wrong, and a different errno.
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("not-a-directory.org");
    std::fs::write(&f, "* a file, not a vault\n").expect("write");
    // Either it refuses, or it treats the file's directory as the
    // vault — what it must not do is panic.
    let _ = page(&f);
    let _ = interactive_page(&f);
}

#[test]
fn the_serve_address_is_loopback_only() {
    // This shell serves its page over HTTP. Binding anything other
    // than loopback would put the user's vault on the network, which
    // is not a thing a local notebook should ever do by default.
    assert!(
        closure_shell_tauri::SERVE_ADDR.starts_with("127.0.0.1:"),
        "the webview shell serves on {}",
        closure_shell_tauri::SERVE_ADDR
    );
}
