//! P2: fold the keyword highlighter's byte ranges into owned
//! `(text, kind)` segments the GUI can colour. Pure + hermetic (the
//! dependency-free `KeywordHighlighter`, no tree-sitter C grammar).

#![allow(clippy::unwrap_used)]

use closure_shell_core::highlight_spans;
use closure_tree_sitter::HighlightKind;

#[test]
fn spans_cover_source_and_concatenate_back() {
    let src = "fn main() {}";
    let spans = highlight_spans(src, "rust");
    let joined: String = spans.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(joined, src, "spans must reconstruct the source exactly");
}

#[test]
fn rust_keyword_is_classified() {
    let spans = highlight_spans("fn x", "rust");
    assert!(
        spans
            .iter()
            .any(|(t, k)| t == "fn" && *k == HighlightKind::Keyword),
        "fn -> Keyword: {spans:?}"
    );
}

#[test]
fn plain_language_has_no_keyword_spans_and_roundtrips() {
    // "plain" has no keyword table, so prose words are Identifier, never
    // Keyword — the window maps Identifier to the default text colour so
    // prose reads normally; only real code-block langs colour keywords.
    let src = "just prose here";
    let spans = highlight_spans(src, "plain");
    assert!(spans.iter().all(|(_, k)| *k != HighlightKind::Keyword));
    let joined: String = spans.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(joined, src);
}

#[test]
fn unknown_language_has_no_keyword_spans() {
    let spans = highlight_spans("anything goes", "brainfuck");
    assert!(spans.iter().all(|(_, k)| *k != HighlightKind::Keyword));
}

#[test]
fn empty_source_yields_no_spans() {
    assert!(highlight_spans("", "rust").is_empty());
}
