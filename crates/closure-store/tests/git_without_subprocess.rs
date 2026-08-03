//! "Since shelling out to git was quite expensive for just reading the
//! porcelain values. Can you swap ALL of the git stuff with the
//! gitoxide lib? Not the gix executable."
//!
//! Measured before this: the git widget cost 6.3ms per edit, all of it
//! process spawn and pipe. Rate-limiting it to once every two seconds
//! made the shell usable; it did not make the reading cheap, and it
//! left a `git` binary as a runtime dependency of a program that is
//! otherwise self-contained.
//!
//! These pin the *behaviour* rather than the implementation, so they
//! were written against the shelling-out version and pass against
//! either — which is the only way to swap an engine and know nothing
//! moved.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::{LineChange, file_diff, git_status};

/// A repository with one committed file and one commit.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git is available to *build* the fixture");
        assert!(out.status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(dir.path().join("a.org"), "* One\nbody\n* Two\nbody\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    dir
}

#[test]
fn a_clean_repository_reports_its_branch() {
    let dir = repo();
    let status = git_status(dir.path()).expect("a repository");
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.modified, 0, "nothing was touched");
    assert_eq!(status.untracked, 0);
}

#[test]
fn a_modified_file_is_counted() {
    let dir = repo();
    std::fs::write(dir.path().join("a.org"), "* One\nchanged\n* Two\nbody\n").unwrap();
    let status = git_status(dir.path()).expect("a repository");
    assert_eq!(status.modified, 1, "the edit was not seen");
}

#[test]
fn an_untracked_file_is_counted_separately() {
    let dir = repo();
    std::fs::write(dir.path().join("new.org"), "* New\n").unwrap();
    let status = git_status(dir.path()).expect("a repository");
    assert_eq!(status.untracked, 1);
    assert_eq!(status.modified, 0, "a new file is not a modification");
}

#[test]
fn a_directory_that_is_not_a_repository_answers_none() {
    // The ordinary case: most vaults are a folder of org files. It has
    // to be cheap and it has to be `None`, not an error.
    let dir = tempfile::tempdir().unwrap();
    assert!(git_status(dir.path()).is_none());
}

#[test]
fn the_diff_marks_the_lines_that_changed() {
    // Zero-based, deliberately: git counts hunks from one and every
    // painter that consumes these counts from zero, so the conversion
    // happens here rather than in each shell. I wrote this test
    // expecting line 2 and it reported line 1 — the code was right and
    // the expectation was mine.
    let dir = repo();
    std::fs::write(dir.path().join("a.org"), "* One\nchanged\n* Two\nbody\n").unwrap();
    let marks = file_diff(dir.path(), std::path::Path::new("a.org"));
    assert!(
        marks
            .iter()
            .any(|(line, kind)| *line == 1 && *kind == LineChange::Changed),
        "the second line changed and the fringe does not say so: {marks:?}"
    );
}

#[test]
fn an_added_line_is_marked_as_added() {
    let dir = repo();
    std::fs::write(
        dir.path().join("a.org"),
        "* One\nbody\nextra\n* Two\nbody\n",
    )
    .unwrap();
    let marks = file_diff(dir.path(), std::path::Path::new("a.org"));
    assert!(
        marks.iter().any(|(_, kind)| *kind == LineChange::Added),
        "an inserted line is not marked: {marks:?}"
    );
}

#[test]
fn every_line_of_an_untracked_file_is_new() {
    // Also written the wrong way round first ("nothing to diff
    // against, so no marks"). A file git has never seen is not
    // unchanged — it is entirely new, and the fringe says so for every
    // line of it, which is what a freshly captured note should look
    // like.
    let dir = repo();
    std::fs::write(dir.path().join("new.org"), "* New\nbody\n").unwrap();
    let marks = file_diff(dir.path(), std::path::Path::new("new.org"));
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert!(marks.iter().all(|(_, kind)| *kind == LineChange::Added));
}

#[test]
fn a_non_repository_has_no_diff() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    assert!(file_diff(dir.path(), std::path::Path::new("a.org")).is_empty());
}

#[test]
fn an_untracked_directory_counts_once_the_way_git_counts_it() {
    // Caught by comparing against `git status --porcelain` on a real
    // vault: git collapses a directory it has never seen into a single
    // entry, and emitting one per file inside counted an assets folder
    // forty times over — 13 against 50.
    let dir = repo();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    for i in 0..5 {
        std::fs::write(assets.join(format!("f{i}.png")), b"x").unwrap();
    }
    let status = git_status(dir.path()).expect("a repository");
    assert_eq!(
        status.untracked, 1,
        "an untracked directory should count once, not once per file"
    );
}
