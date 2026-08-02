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

fn main() {
    // Re-run when the checked-out commit moves. Without this the
    // values freeze at whatever the first build saw. Both paths are
    // needed: `HEAD` for the ref, and the ref file itself for a commit
    // on the same branch.
    for path in [".git/HEAD", "../../.git/HEAD"] {
        if std::path::Path::new(path).exists() {
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

    if let Some(commit) = git(&["rev-parse", "--short=12", "HEAD"]) {
        println!("cargo::rustc-env=CLOSURE_GIT_COMMIT={commit}");
    }
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
}
