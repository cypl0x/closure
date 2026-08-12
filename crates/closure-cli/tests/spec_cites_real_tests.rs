//! The spec names the tests that enforce it, and those tests exist.
//!
//! `docs/spec.md` says which file holds each invariant to account —
//! "enforced by `closure-query/tests/composition_is_a_view.rs`", and a
//! dozen more like it. That is the difference between an invariant and
//! an intention, and it is only worth anything while the names are
//! true.
//!
//! A citation that has gone stale is worse than none. It reads as
//! evidence, it survives review because nobody re-checks a path, and
//! the first person to notice is whoever went looking for the test
//! that was supposed to make a rule real.
//!
//! The same reasoning as `command_source.rs`, one layer up: a pointer
//! into a file nobody has is not an answer.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Every `<crate>/tests/<name>.rs` or `<crate>/src/<name>.rs` the spec
/// mentions.
fn cited(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == '`' || c == '(' || c == ')') {
        let word = word.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '*' | '_'));
        if word.starts_with("closure-")
            && word.ends_with(".rs")
            && (word.contains("/tests/") || word.contains("/src/"))
        {
            out.push(word.to_owned());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn every_test_the_spec_names_is_a_file_that_exists() {
    let root = repo_root();
    let spec = std::fs::read_to_string(root.join("docs/spec.md")).expect("docs/spec.md");
    let names = cited(&spec);
    assert!(
        names.len() >= 5,
        "the spec has stopped naming its tests: {names:?}"
    );
    for name in names {
        assert!(
            root.join("crates").join(&name).exists(),
            "docs/spec.md cites `{name}`, which does not exist"
        );
    }
}

#[test]
fn the_work_queue_names_files_that_exist_too() {
    // Same rule for `docs/kernel-gpui.org`: every item carries an
    // acceptance line naming a test, and an acceptance nobody can run
    // is an item nobody can close.
    let root = repo_root();
    let queue = root.join("docs/kernel-gpui.org");
    let Ok(text) = std::fs::read_to_string(&queue) else {
        // The queue is a working document and may be finished and
        // removed; its absence is not a failure of the spec.
        return;
    };
    for name in cited(&text) {
        assert!(
            root.join("crates").join(&name).exists(),
            "docs/kernel-gpui.org cites `{name}`, which does not exist"
        );
    }
}
