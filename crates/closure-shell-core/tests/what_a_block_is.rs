//! "Decide, and write into docs/spec.md, what a block is."
//!
//! The vision asks for Notion-style composable building blocks. Org
//! gives a tree of headlines made of elements. Those are two different
//! units and the spec never said which one closure composes, so every
//! item that depends on the answer — parameters, typed inputs, slots,
//! cycle limits, the database panes — had nothing to be tested
//! against.
//!
//! The answer is three tiers, and it is in `docs/spec.md` beside the
//! invariant that makes it hold. This pins both, because a definition
//! that lives only in prose is the thing the manual exists to prevent.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{INVARIANTS, manual_org};

/// The spec, read from the repo the way `i7_is_true` reads manifests.
fn spec() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    std::fs::read_to_string(root.join("docs/spec.md")).expect("docs/spec.md")
}

#[test]
fn the_spec_says_which_unit_composes() {
    let s = spec();
    assert!(
        s.contains("## What a block is"),
        "the spec still does not define the composition unit"
    );
    let lower = s.to_lowercase();
    for tier in ["addressable block", "content block", "composable block"] {
        assert!(lower.contains(tier), "the definition is missing `{tier}`");
    }
}

#[test]
fn the_spec_forbids_writing_an_expansion_back() {
    // The half of the definition that is enforceable, and the reason
    // closure's dynamic blocks are not org's.
    let s = spec();
    assert!(
        s.contains("### I12"),
        "there is no invariant holding composition to a view"
    );
}

#[test]
fn the_invariant_list_carries_it() {
    // `closure spec` and the manual both read this list, so an
    // invariant that is only in the markdown is one the running
    // program does not claim.
    // By name, not by count: I11 is the performance budget and belongs
    // to its own item, so counting here would make two independent
    // pieces of work fail together.
    assert!(
        INVARIANTS.iter().any(|l| l.starts_with("I12")),
        "{INVARIANTS:?}"
    );
    for want in ["I1 ", "I8 ", "I12"] {
        assert!(
            INVARIANTS.iter().any(|l| l.starts_with(want)),
            "`{want}` left the list"
        );
    }
    assert!(manual_org(InputMode::Doom).contains("I12"));
}
