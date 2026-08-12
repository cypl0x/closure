//! The window highlights through `pick_highlighter`, not past it.
//!
//! `pick_highlighter` exists precisely for this, and its own doc says
//! so: "this function existed twice: `closure-tui` had it and the gpui
//! shell — the one the user looks at all day — reached straight for
//! `KeywordHighlighter`, so twenty grammars would have been compiled
//! and never consulted."
//!
//! It was still reaching past it. `highlight_spans` named
//! `KeywordHighlighter` directly, so building the gpui shell with
//! `--features tree-sitter` compiled twenty C grammars and changed
//! nothing on screen. The fix had been written and not applied at the
//! one call site it was written for.
//!
//! These tests hold for both builds on purpose. What must be true
//! without the feature is that highlighting still works; what must be
//! true with it is that the same call goes somewhere better. A test
//! that only ran under the opt-in would be a test nobody runs.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{HighlightKind, highlight_spans};

#[test]
fn the_segments_reassemble_into_the_source() {
    // The contract every highlighter owes, whichever one answered.
    let src = "fn main() { /* hi */ let x = 1; }";
    let joined: String = highlight_spans(src, "rust")
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(joined, src);
}

#[test]
fn a_comment_is_marked_as_one() {
    // True of the keyword highlighter and of a real grammar, which is
    // what makes it a fair assertion across both builds.
    let spans = highlight_spans("# a comment\nx = 1\n", "python");
    assert!(
        spans
            .iter()
            .any(|(text, kind)| *kind == HighlightKind::Comment && text.contains("comment")),
        "{spans:?}"
    );
}

#[test]
fn empty_source_yields_nothing() {
    assert!(highlight_spans("", "rust").is_empty());
}

#[test]
fn a_language_nobody_has_a_grammar_for_still_highlights() {
    // The fallback has to survive the dispatch. A language with no
    // tree-sitter grammar must not come back unhighlighted just
    // because the feature is on.
    let spans = highlight_spans("-- a comment\nx = 1\n", "haskell");
    assert!(!spans.is_empty());
    let joined: String = spans.into_iter().map(|(t, _)| t).collect();
    assert_eq!(joined, "-- a comment\nx = 1\n");
}

#[test]
fn the_shell_says_which_highlighter_is_running() {
    // The same courtesy the software-rasteriser notice pays. A reader
    // whose code blocks look plain should be able to find out whether
    // this build has the grammars, rather than concluding highlighting
    // is broken.
    let name = closure_shell_core::highlighter_name();
    assert!(
        name == "keyword" || name == "tree-sitter",
        "unexpected highlighter name: {name}"
    );
}
