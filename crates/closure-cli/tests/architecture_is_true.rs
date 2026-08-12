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

// === the phase table gates on invariants that exist (2026-08-12) ===
//
// `docs/phases.md` names, per milestone, which invariants must be green
// at its boundary. The spec grew I11 and I12 and the table did not
// learn about them, which is the same drift as the layer diagram: a
// planning document that describes a system one release ago.
//
// The check is deliberately one-directional. Every invariant the table
// names must exist — a phase gated on a rule nobody wrote is a gate
// that passes for the wrong reason. The reverse is not required: an
// invariant may be older than every phase boundary, or hold
// continuously rather than at one.

#[test]
fn every_invariant_a_phase_gates_on_exists() {
    let root = repo_root();
    let phases = std::fs::read_to_string(root.join("docs/phases.md")).expect("phases.md");
    let spec = std::fs::read_to_string(root.join("docs/spec.md")).expect("spec.md");
    let mut named: Vec<String> = Vec::new();
    for word in phases.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(n) = word.strip_prefix('I')
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            named.push(format!("I{n}"));
        }
    }
    named.sort();
    named.dedup();
    assert!(named.len() >= 5, "the phase table names no invariants");
    for i in named {
        assert!(
            spec.contains(&format!("### {i} ")),
            "docs/phases.md gates a milestone on `{i}`, which docs/spec.md does not define"
        );
    }
}

#[test]
fn the_newest_invariants_are_gated_somewhere() {
    // The other direction, for the two this session added only. An
    // invariant nobody's boundary checks is one that can rot between
    // releases without anybody noticing.
    let root = repo_root();
    let phases = std::fs::read_to_string(root.join("docs/phases.md")).expect("phases.md");
    for i in ["I11", "I12"] {
        assert!(
            phases.contains(i),
            "no phase boundary checks `{i}` — it can rot without anybody noticing"
        );
    }
}
