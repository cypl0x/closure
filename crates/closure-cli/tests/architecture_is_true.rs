//! The layer diagram names the crates that exist, and all of them.
//!
//! `docs/architecture.md` is one page and it is the page somebody reads
//! first. It listed `closure-spec`, `closure-util` and
//! `closure-flutter`, none of which exist, and left out
//! `closure-shell-core`, which is the largest crate in the workspace
//! and the thing every shell is built from.
//!
//! That is the failure I7 was restated for in 2026-08: a document
//! making a claim the manifests refute on the line below. A diagram
//! that is wrong in both directions at once — naming what is not there,
//! omitting what is — is worse than no diagram, because it is the
//! thing a newcomer trusts before they know enough to check.
//!
//! So the diagram is checked against `crates/` rather than maintained
//! by hand and hoped over.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Every crate directory under `crates/`.
fn workspace_crates() -> BTreeSet<String> {
    std::fs::read_dir(repo_root().join("crates"))
        .expect("crates/")
        .filter_map(Result::ok)
        .filter(|e| e.path().join("Cargo.toml").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Every `closure-…` name the diagram mentions.
///
/// Full names only. The diagram used to abbreviate a run as
/// `closure-lsp  -acp`, which reads as `closure-acp` on one line and
/// as `closure-plugin-sniffer` on the next — ambiguous to a parser and
/// to a person. Spelling them out is the fix; a cleverer reader would
/// only have hidden the ambiguity from the one reader who matters.
fn named_in_diagram(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| c == '\u{2502}' || c == ',');
        if let Some(rest) = word.strip_prefix("closure-")
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !rest.is_empty()
        {
            out.insert(format!("closure-{rest}"));
        }
    }
    out
}

#[test]
fn the_diagram_names_no_crate_that_does_not_exist() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("docs/architecture.md")).expect("architecture.md");
    let have = workspace_crates();
    let named = named_in_diagram(&text);
    let ghosts: Vec<&String> = named.iter().filter(|c| !have.contains(*c)).collect();
    assert!(
        ghosts.is_empty(),
        "the layer diagram names crates that do not exist: {ghosts:?}"
    );
}

#[test]
fn every_crate_appears_somewhere_in_the_diagram() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("docs/architecture.md")).expect("architecture.md");
    let named = named_in_diagram(&text);
    let missing: Vec<String> = workspace_crates()
        .into_iter()
        .filter(|c| !named.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "these crates exist and the layer diagram does not place them: {missing:?}"
    );
}
