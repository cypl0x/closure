//! I7 said one thing and the manifests said another.
//!
//! "**I7 — 'shells consume closure-core only; spans pub(crate)
//! firewall.'** The span half is true (`pub(crate) struct Span`,
//! verified). The first half is false for every shell … Either drop I7
//! or enforce it with `cargo-deny`. Right now it's a claim the manifest
//! refutes on the line below it." — the Opus 5 review, 2026-08-04.
//!
//! It is enforced here instead of dropped, because the part that
//! matters is enforceable and is what the invariant was *for*: a shell
//! must never address content by byte offset, so the byte offsets must
//! not be reachable from one. Restated to what the code actually
//! guarantees, and pinned so it cannot drift again:
//!
//! * no shell names `Span`, `span`, `byte offset` or a line index into
//!   source — the parser's coordinates stay inside the parser crates;
//! * no shell mutates a `Document` except through a registered command
//!   (I8), which is a separate invariant and separately enforced;
//! * a shell may *read* org's public view types, because rendering org
//!   means knowing what a headline and a markup run are.
//!
//! The last clause is the honest change. `closure-shell-gpui` reads
//! `Headline`, `MarkupKind`, `markup_spans`, `block_delimiter_of` and
//! `parse`, and there is no version of "render org" that does not.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::{Path, PathBuf};

/// Every shell crate in the workspace.
fn shells() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
    let mut out: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("read crates/")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("closure-shell-") || n == "closure-tui")
        })
        .collect();
    out.sort();
    assert!(out.len() >= 6, "found only {} shells", out.len());
    out
}

fn sources(crate_dir: &Path) -> Vec<(PathBuf, String)> {
    let src = crate_dir.join("src");
    let Ok(entries) = std::fs::read_dir(&src) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .collect()
}

#[test]
fn no_shell_reaches_for_the_parsers_coordinates() {
    // The half of I7 that was always true, and the half worth keeping:
    // spans are `pub(crate)` inside the parser crates, so a shell
    // cannot address content by byte offset even by accident.
    for shell in shells() {
        for (path, text) in sources(&shell) {
            for needle in ["closure_org::Span", "closure_core::Span", "::span("] {
                assert!(
                    !text.contains(needle),
                    "{} names {needle} — I7's firewall",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn a_shell_only_reads_the_org_types_it_renders() {
    // What a shell may take from `closure-org`: the view types, the
    // read-only scanners, and `parse`. Anything that *edits* has to go
    // through a command (I8), so a rewrite called from a shell is the
    // thing this catches.
    for shell in shells() {
        for (path, text) in sources(&shell) {
            // A call, not a doc link: `[\`closure_org::rewrite_…\`]`
            // in a comment explains where the edit happens, which is
            // the opposite of hiding it.
            let calls: Vec<&str> = text
                .lines()
                .map(str::trim_start)
                .filter(|l| !l.starts_with("//"))
                .filter(|l| l.contains("closure_org::rewrite_"))
                .collect();
            assert!(
                calls.is_empty(),
                "{} calls a rewrite directly — edits go through commands (I8): {calls:?}",
                path.display()
            );
        }
    }
}

#[test]
fn no_manifest_claims_something_its_dependencies_refute() {
    // The finding itself: two crates described themselves as
    // "Consumes closure-core only (I7)" with `closure-store`,
    // `closure-query`, `closure-config` and `closure-shell-core`
    // listed on the lines below.
    for shell in shells() {
        let manifest = std::fs::read_to_string(shell.join("Cargo.toml")).expect("Cargo.toml");
        let (head, deps) = manifest
            .split_once("[dependencies]")
            .unwrap_or((manifest.as_str(), ""));
        if !head.contains("closure-core only") {
            continue;
        }
        let others: Vec<&str> = deps
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(name, _)| name.trim())
            .filter(|n| n.starts_with("closure-") && *n != "closure-core")
            .collect();
        assert!(
            others.is_empty(),
            "{} says it consumes closure-core only, and depends on {others:?}",
            shell.display()
        );
    }
}
