//! "Because I do have to resize the outline headlines tree view EVERY
//! time I open closure." And: "remember window configuration. For
//! example how wide my outline headline tree view should be."
//!
//! The pane has always been resizable — there is a drag handle and a
//! width behind it — and the width was never written down, so every
//! session started at the default and every session began with the
//! same drag.
//!
//! "Can or should we persist the view state in some org-mode native
//! syntax? Attributes?" It goes where the rest of the session state
//! already goes: the `closure-config` block in config.org, beside
//! `last_place` and `recent_files`, written when the window closes.
//! That is org, it is the file you already edit by hand, and it means
//! there is one place to look rather than a new dotfile with its own
//! rules.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01PANEWIDTH0000000000AA\n:END:\nbody\n";

fn vault() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\n# how you type\ninput_mode = doom\n#+END_SRC\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell)
}

#[test]
fn a_resized_pane_is_written_down() {
    let (dir, shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.set_outline_width(Some(640));
    app.save_last_place(&shell);

    let source = std::fs::read_to_string(dir.path().join("config.org")).unwrap();
    let cfg = closure_config::Config::from_org_source(&source).expect("still parses");
    assert_eq!(
        cfg.outline_width,
        Some(640),
        "the width was not remembered:\n{source}"
    );
}

#[test]
fn the_remembered_width_comes_back() {
    let (dir, shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.set_outline_width(Some(640));
    app.save_last_place(&shell);

    // A fresh session over the same vault.
    let shell2 = Shell::new(Vault::open(dir.path()).unwrap());
    let mut next = ModalApp::new(closure_config::InputMode::Doom);
    next.restore_last_place(&shell2);
    assert_eq!(next.outline_width(), Some(640));
}

#[test]
fn a_session_that_never_resized_writes_nothing() {
    // A key that appears the first time you close the window, holding
    // the default, is noise in a file the user reads.
    let (dir, shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.save_last_place(&shell);
    let source = std::fs::read_to_string(dir.path().join("config.org")).unwrap();
    assert!(
        !source.contains("outline_width"),
        "an untouched pane wrote a width:\n{source}"
    );
}

#[test]
fn the_users_own_comments_survive_the_write() {
    let (dir, shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.set_outline_width(Some(500));
    app.save_last_place(&shell);
    let source = std::fs::read_to_string(dir.path().join("config.org")).unwrap();
    assert!(source.contains("# how you type"), "{source}");
    assert!(source.contains("input_mode = doom"), "{source}");
}

#[test]
fn a_nonsense_width_in_the_file_is_ignored() {
    // config.org is hand-edited. A width of zero, or a negative one,
    // would paint a pane you cannot see and cannot grab to fix.
    let (dir, shell) = vault();
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\noutline_width = -20\n#+END_SRC\n",
    )
    .unwrap();
    let _ = shell;
    let shell2 = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.restore_last_place(&shell2);
    assert_eq!(app.outline_width(), None, "a negative width was accepted");
}
