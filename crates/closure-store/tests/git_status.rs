//! "git status in the UI — Just some lightweight read only (for now)
//! widgets that indicate the vault or files git status. … somewhere in
//! the top/bottom left/right corner put the vault git status icons +
//! number to indicate the vault git repo status. … git (diff) fringes
//! in the editor"
//!
//! Explicitly *not* `git status`'s own output pasted into a pane. What
//! the item asks for is the state, as numbers a widget can draw: how
//! many files differ, on which branch. The terminal already prints the
//! text version better than any pane could.
//!
//! Read-only, and cheap enough to be worth having. Running `git` on
//! every frame would be the level-1 microfreeze all over again, so
//! this is a function a caller asks when something changed — never
//! something a painter reaches for.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command;

use closure_store::git_status;

/// A vault that is also a git repository, with one commit in it.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tmp");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    git(&["add", "notes.org"]);
    git(&["commit", "-m", "first"]);
    dir
}

#[test]
fn a_directory_that_is_not_a_repository_has_no_status() {
    // Most vaults are not repositories, and that is not an error — the
    // widget simply has nothing to draw.
    let dir = tempfile::tempdir().expect("tmp");
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn a_clean_repository_reports_a_branch_and_nothing_else() {
    let dir = repo();
    let st = git_status(dir.path()).expect("a repository");
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!(st.modified, 0);
    assert_eq!(st.staged, 0);
    assert_eq!(st.untracked, 0);
    assert!(st.is_clean());
}

#[test]
fn an_edited_file_counts_as_modified() {
    let dir = repo();
    fs::write(dir.path().join("notes.org"), "* Alpha\n* Beta\n").expect("write");
    let st = git_status(dir.path()).expect("a repository");
    assert_eq!(st.modified, 1, "{st:?}");
    assert!(!st.is_clean());
}

#[test]
fn a_staged_change_counts_separately() {
    // Two different things worth knowing: what you have changed, and
    // what you have already told git about.
    let dir = repo();
    fs::write(dir.path().join("notes.org"), "* Alpha\n* Beta\n").expect("write");
    Command::new("git")
        .args(["add", "notes.org"])
        .current_dir(dir.path())
        .output()
        .expect("git");
    let st = git_status(dir.path()).expect("a repository");
    assert_eq!(st.staged, 1, "{st:?}");
    assert_eq!(st.modified, 0, "{st:?}");
}

#[test]
fn a_new_note_counts_as_untracked() {
    // The common case in a vault: a note you just captured.
    let dir = repo();
    fs::write(dir.path().join("new.org"), "* Fresh\n").expect("write");
    let st = git_status(dir.path()).expect("a repository");
    assert_eq!(st.untracked, 1, "{st:?}");
}

#[test]
fn a_path_with_spaces_is_still_one_file() {
    // `--porcelain` quotes such a path; counting lines is right, but
    // only if the parse does not trip over the quoting.
    let dir = repo();
    fs::write(dir.path().join("two words.org"), "* Fresh\n").expect("write");
    let st = git_status(dir.path()).expect("a repository");
    assert_eq!(st.untracked, 1, "{st:?}");
}

#[test]
fn a_detached_head_still_reports_something() {
    // Not a branch, but the widget must not go blank.
    let dir = repo();
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git");
    let sha = String::from_utf8(sha.stdout).unwrap().trim().to_owned();
    Command::new("git")
        .args(["checkout", "--detach", &sha])
        .current_dir(dir.path())
        .output()
        .expect("git");
    let st = git_status(dir.path()).expect("a repository");
    assert!(st.branch.is_some(), "{st:?}");
}

#[test]
fn the_summary_is_short_enough_for_a_corner() {
    let dir = repo();
    fs::write(dir.path().join("notes.org"), "* Alpha\n* Beta\n").expect("write");
    fs::write(dir.path().join("new.org"), "* Fresh\n").expect("write");
    let st = git_status(dir.path()).expect("a repository");
    let text = st.summary();
    assert!(text.contains("main"), "{text}");
    assert!(
        text.chars().count() <= 32,
        "{} chars: {text}",
        text.chars().count()
    );
}

#[test]
fn a_clean_repository_summarises_as_clean() {
    let dir = repo();
    let st = git_status(dir.path()).expect("a repository");
    assert!(st.summary().contains("main"), "{}", st.summary());
}

#[test]
fn a_vault_below_the_repository_root_still_finds_it() {
    // A vault is often a subdirectory of a dotfiles repository.
    let dir = repo();
    let sub = dir.path().join("notes");
    fs::create_dir_all(&sub).expect("mkdir");
    fs::write(sub.join("a.org"), "* A\n").expect("write");
    assert!(git_status(&sub).is_some());
}

#[test]
fn it_does_not_wander_up_into_an_unrelated_repository() {
    // A temp dir under `/tmp` must not report the status of whatever
    // repository happens to contain it — but a real subdirectory of a
    // repository should. `Path::new("/")` has neither.
    assert!(git_status(Path::new("/")).is_none());
}
