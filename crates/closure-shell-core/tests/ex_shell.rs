//! `:! pwd` — vim's shell escape, behind closure's own trust gate.
//!
//! Running arbitrary shell from an editor is exactly the capability the
//! evaluator already default-denies (C1a): a vault is a file someone
//! can send you, and `#+BEGIN_SRC shell` in it must not run. `:!` is
//! the same capability typed by hand, so it answers to the same key —
//! `eval_trust` in the vault's `config.org` — rather than inventing a
//! second, looser rule for the same thing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn vault_with_config(config: Option<&str>) -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    if let Some(body) = config {
        fs::write(
            dir.path().join("config.org"),
            format!("#+BEGIN_SRC closure-config\n{body}\n#+END_SRC\n"),
        )
        .expect("write config");
    }
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn a_shell_escape_is_refused_by_default() {
    // Default-deny, and it says which key opens it rather than just
    // failing.
    let (_d, mut sh) = vault_with_config(None);
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!echo hello");
    let status = app.status();
    assert!(status.contains("eval_trust"), "{status}");
    assert!(
        app.shell_output().is_none(),
        "nothing ran: {:?}",
        app.shell_output()
    );
}

#[test]
fn a_trusted_vault_runs_the_command_and_shows_its_output() {
    let (_d, mut sh) = vault_with_config(Some("eval_trust = shell"));
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!echo hello");
    assert!(
        app.shell_output().is_some_and(|o| o.contains("hello")),
        "stdout is shown: {:?}",
        app.shell_output()
    );
}

#[test]
fn a_failing_command_reports_its_exit_code() {
    let (_d, mut sh) = vault_with_config(Some("eval_trust = shell"));
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!exit 3");
    let status = app.status();
    assert!(status.contains('3'), "the code is in the status: {status}");
}

#[test]
fn stderr_is_shown_too() {
    // A command that only writes to stderr must not look like one that
    // did nothing.
    let (_d, mut sh) = vault_with_config(Some("eval_trust = shell"));
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!echo oops 1>&2");
    assert!(
        app.shell_output().is_some_and(|o| o.contains("oops")),
        "{:?}",
        app.shell_output()
    );
}

#[test]
fn an_empty_bang_says_what_it_wants() {
    let (_d, mut sh) = vault_with_config(Some("eval_trust = shell"));
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!");
    assert!(!app.status().is_empty());
    assert!(app.shell_output().is_none(), "nothing ran");
}

#[test]
fn the_command_runs_in_the_vault_directory() {
    // `:! ls` should list the vault, which is the only directory the
    // user is thinking about.
    let (dir, mut sh) = vault_with_config(Some("eval_trust = shell"));
    let mut app = ModalApp::new(InputMode::Vim);
    app.run_ex_line(&mut sh, "!pwd");
    let out = app.shell_output().unwrap_or_default();
    let want = dir.path().canonicalize().expect("canonical");
    assert!(
        out.contains(&want.display().to_string()),
        "ran in {out:?}, wanted {want:?}"
    );
}
