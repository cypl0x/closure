//! `build-info` reported a commit that was not the one running.
//!
//! The screenshot said `closure 89cf823c0c84 (1623 commits)` after a
//! pull and rebuild that had moved the tree to `6403e51`. Not a
//! rendering bug: the value was captured once and never recaptured.
//!
//! `build.rs` asked cargo to re-run on `.git/HEAD`. On a branch that
//! file holds `ref: refs/heads/main` and does not change when a commit
//! lands or a pull fast-forwards — only `.git/refs/heads/main` does.
//! In this repository `.git/HEAD` was last written on 2026-08-01 and
//! the ref file on 2026-08-03, which is the whole bug in two
//! timestamps.
//!
//! A version string that lies is worse than none: it is the thing you
//! reach for to find out what you are running.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::gitwatch::watch_paths;

#[test]
fn the_branch_ref_is_watched_not_only_head() {
    let paths = watch_paths(".git", "ref: refs/heads/main\n");
    assert!(
        paths.iter().any(|p| p == ".git/refs/heads/main"),
        "a commit on the current branch would go unnoticed: {paths:?}"
    );
}

#[test]
fn head_itself_is_still_watched() {
    // Switching branches *does* rewrite HEAD, and that has to be
    // noticed too.
    let paths = watch_paths(".git", "ref: refs/heads/main\n");
    assert!(paths.iter().any(|p| p == ".git/HEAD"), "{paths:?}");
}

#[test]
fn packed_refs_is_watched() {
    // `git gc` moves the loose ref into `packed-refs` and deletes it,
    // after which the loose path never changes again — and the hash
    // would freeze a second time, for a different reason.
    let paths = watch_paths(".git", "ref: refs/heads/main\n");
    assert!(paths.iter().any(|p| p == ".git/packed-refs"), "{paths:?}");
}

#[test]
fn a_detached_head_watches_only_head() {
    // Detached, `HEAD` holds the hash itself, so it is the only file
    // that moves — and there is no ref path to invent.
    let paths = watch_paths(".git", "6403e51ce9fd0000000000000000000000000000\n");
    assert_eq!(paths, vec![".git/HEAD".to_owned()]);
}

#[test]
fn a_worktree_git_dir_is_respected() {
    // `.git` is not always `.git`: in a worktree it is a file pointing
    // elsewhere, and the paths have to follow it rather than assume.
    let paths = watch_paths("/repo/.git/worktrees/wt", "ref: refs/heads/side\n");
    assert!(
        paths
            .iter()
            .any(|p| p == "/repo/.git/worktrees/wt/refs/heads/side"),
        "{paths:?}"
    );
}
