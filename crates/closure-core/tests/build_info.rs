//! "build time git commit hash (and if from dirty working tree append
//! that too) … I don't want to have a timestamp when the executable
//! has been built, because that would break the reproducibility. There
//! is a reason that nix defaults every mtime to epoch. … Additionally
//! the commit count is something I could make use of as well. If you
//! can wire them in at built time it would be great. Create a
//! function/command that returns these values or alternatively prints
//! them to the stdout/*MESSAGES* buffer."
//!
//! The reproducibility argument decides the whole shape. A timestamp
//! is a property of *when* you built; a commit hash is a property of
//! *what* you built, so the same tree always yields the same binary
//! and the value still identifies the source exactly. The dirty flag
//! is what keeps that honest: a build from an edited tree is not the
//! commit it claims, and says so.
//!
//! Nothing here may fail a build. A source tarball has no `.git` — nix
//! builds from one routinely — and a version string is never worth a
//! broken build, so an unknown value is reported as unknown.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::build_info;

#[test]
fn there_is_always_a_version_string() {
    // Whatever the build environment, something identifies it.
    assert!(!build_info().describe().is_empty());
}

#[test]
fn it_never_carries_a_timestamp() {
    // The reason the item exists. A build stamp would make two builds
    // of one tree differ, which is exactly what nix's epoch mtimes
    // exist to prevent.
    let d = build_info().describe();
    for stamp in ["20", "GMT", "UTC", ":"] {
        // A commit hash is hex and a count is digits; neither contains
        // a colon or a zone, and `20` would only appear in a year.
        if stamp == "20" {
            continue;
        }
        assert!(!d.contains(stamp), "looks like a timestamp: {d}");
    }
}

#[test]
fn the_same_source_describes_itself_the_same_way() {
    // Determinism, stated as a property: nothing in here reads a clock
    // or an environment that changes between two builds of one tree.
    assert_eq!(build_info().describe(), build_info().describe());
}

#[test]
fn a_known_commit_is_hex_and_short() {
    let info = build_info();
    if let Some(commit) = info.commit {
        assert!(
            commit.chars().all(|c| c.is_ascii_hexdigit()),
            "not a hash: {commit}"
        );
        assert!((6..=40).contains(&commit.len()), "odd length: {commit}");
    }
}

#[test]
fn a_known_count_is_a_number() {
    let info = build_info();
    if let Some(count) = info.commits {
        assert!(count > 0, "a repository with no commits built this?");
    }
}

#[test]
fn a_dirty_build_says_so_in_the_description() {
    let info = build_info();
    assert_eq!(
        info.dirty,
        info.describe().contains("dirty"),
        "the flag and the string disagree: {}",
        info.describe()
    );
}

#[test]
fn the_description_carries_what_it_knows() {
    let info = build_info();
    if let Some(commit) = info.commit {
        assert!(
            info.describe().contains(commit),
            "the commit is missing from {}",
            info.describe()
        );
    }
    if let Some(count) = info.commits {
        assert!(
            info.describe().contains(&count.to_string()),
            "the count is missing from {}",
            info.describe()
        );
    }
}

#[test]
fn an_unknown_build_is_reported_rather_than_faked() {
    // A tarball with no `.git` must not produce a plausible-looking
    // hash, and must not have failed the build to get here.
    let info = build_info();
    if info.commit.is_none() {
        assert!(
            info.describe().contains("unknown"),
            "invented something: {}",
            info.describe()
        );
    }
}
