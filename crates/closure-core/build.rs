//! Capture what was built, at build time.
//!
//! "build time git commit hash (and if from dirty working tree append
//! that too) … I don't want to have a timestamp when the executable
//! has been built, because that would break the reproducibility."
//!
//! So: no clock is read here. A commit hash and a commit count are
//! properties of the *source*, which means two builds of one tree
//! produce one binary — the thing nix's epoch mtimes exist to protect
//! — while still identifying exactly what was built. The dirty flag is
//! what keeps that claim honest: a build from an edited tree is not
//! the commit it names.
//!
//! Nothing in here may fail a build. Building from a source tarball
//! with no `.git` is ordinary (nix does it), and a version string is
//! never worth a broken build: what is not known is reported unknown.

use std::process::Command;

include!("src/gitwatch.rs");

fn main() {
    // Re-run when the checked-out commit moves. Without this the
    // values freeze at whatever the first build saw — which is what
    // made `build-info` report `89cf823` from a tree that was on
    // `6403e51`, two days and forty commits later.
    //
    // Watching `HEAD` alone was the bug. On a branch it holds
    // `ref: refs/heads/main` and does not change when a commit lands
    // or a pull fast-forwards; only the ref file does. In this
    // repository `HEAD` was last written two days before the ref it
    // points at.
    //
    // The rule lives in `src/gitwatch.rs` and is `include!`d rather
    // than copied: a build script cannot depend on its own crate, and
    // a second copy of this parser is exactly how the two would drift.
    for git_dir in [".git", "../../.git"] {
        let head = std::path::Path::new(git_dir).join("HEAD");
        let Ok(contents) = std::fs::read_to_string(&head) else {
            continue;
        };
        for path in watch_paths(git_dir, &contents) {
            println!("cargo::rerun-if-changed={path}");
        }
    }

    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8(out.stdout).ok()?.trim().to_owned();
        (!text.is_empty()).then_some(text)
    };

    // A nix build has no `.git` to ask, but the flake knows the
    // revision and passes it in. Whatever is already set wins: it came
    // from something that could see the repository, and this could
    // not.
    if std::env::var_os("CLOSURE_GIT_COMMIT").is_none()
        && let Some(commit) = git(&["rev-parse", "--short=12", "HEAD"])
    {
        println!("cargo::rustc-env=CLOSURE_GIT_COMMIT={commit}");
    }
    println!("cargo::rerun-if-env-changed=CLOSURE_GIT_COMMIT");
    if let Some(count) = git(&["rev-list", "--count", "HEAD"]) {
        println!("cargo::rustc-env=CLOSURE_GIT_COMMITS={count}");
    }
    // Tracked changes only. Untracked files are not what was compiled,
    // and calling a build dirty because of an editor swap file beside
    // it would make the flag mean nothing.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.trim().is_empty());
    if dirty {
        println!("cargo::rustc-env=CLOSURE_GIT_DIRTY=1");
    }

    // The feature list is *not* captured here, though it looks like it
    // belongs beside the hash. Cargo features are per crate, and
    // `closure-core` has none of its own — asking it what the binary
    // was built with can only ever answer "nothing". It is captured in
    // `closure-cli`, which owns the flags that vary.
}
