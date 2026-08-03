//! Every edit was paying for a `git status`.
//!
//! Measured on the user's own vault: a fold costs 0.35ms, and the git
//! widget behind it costs 6.3ms — because the memo is keyed on the
//! vault revision, and every mutation moves the revision. So each
//! keystroke that changes anything spawned `git rev-parse`,
//! `git status --porcelain` and friends, synchronously, on the UI
//! thread. That is a third of a 60Hz frame per keypress, before the
//! window has drawn anything.
//!
//! The revision key is right for correctness and wrong for cost. A git
//! indicator is ambient: it says roughly where the working tree stands.
//! Nobody acts on it within 200ms of a keystroke, and nothing else in
//! the shell depends on it being exact. So it is also rate-limited —
//! at most one run per interval, no matter how fast you type.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* Alpha\n:PROPERTIES:\n:ID: 01GITCOST0000000000000000\n:END:\nbody\n\
                   * Beta\n:PROPERTIES:\n:ID: 01GITCOST1111111111111111\n:END:\nbody\n";

fn repo_vault() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    // A real repository, so `git_status` has something to answer.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .ok();
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "one"]);
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell)
}

#[test]
fn typing_does_not_spawn_a_git_process_per_keystroke() {
    let (_dir, mut shell) = repo_vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    // Prime it: the first read is allowed to cost what it costs.
    let _ = app.git_state(&shell);
    let after_first = app.git_reads();

    for _ in 0..20 {
        app.run(&mut shell, "toggle-fold");
        let _ = app.git_state(&shell);
    }

    let runs = app.git_reads() - after_first;
    assert!(
        runs <= 1,
        "twenty edits ran git {runs} times — once per keystroke, on the UI thread"
    );
}

#[test]
fn the_widget_still_answers_while_it_is_rate_limited() {
    // Rate-limiting must not turn into "no answer": the status bar
    // keeps showing the last known state rather than blanking.
    let (_dir, mut shell) = repo_vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    let first = app.git_state(&shell);
    assert!(first.is_some(), "no git state in a real repository");
    app.run(&mut shell, "toggle-fold");
    assert!(
        app.git_state(&shell).is_some(),
        "the widget went blank as soon as it was rate-limited"
    );
}

#[test]
fn a_vault_that_is_not_a_repository_is_not_asked_repeatedly() {
    // The common case for most vaults, and the one where the answer is
    // "no" — which must also not cost a process per keystroke.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    let _ = app.git_state(&shell);
    let base = app.git_reads();
    for _ in 0..10 {
        app.run(&mut shell, "toggle-fold");
        let _ = app.git_state(&shell);
    }
    assert!(
        app.git_reads() - base <= 1,
        "{} runs",
        app.git_reads() - base
    );
}
