//! Keyword-based highlighter: a dependency-free default that produces
//! contiguous, gap-free highlight spans for common languages.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_tree_sitter::{HighlightKind, Highlighter, KeywordHighlighter};

fn covers(spans: &[closure_tree_sitter::Highlight], len: usize) {
    assert_eq!(spans.first().map(|s| s.start), Some(0).filter(|_| len > 0));
    let mut prev = 0;
    for s in spans {
        assert_eq!(s.start, prev, "no gaps");
        assert!(s.end > s.start, "non-empty");
        prev = s.end;
    }
    assert_eq!(prev, len, "covers the whole input");
}

#[test]
fn rust_keywords_and_strings_classified() {
    let h = KeywordHighlighter::rust();
    let src = "fn main() { let x = \"hi\"; }";
    let spans = h.highlight(src);
    covers(&spans, src.len());
    let kw = spans.iter().find(|s| &src[s.start..s.end] == "fn").unwrap();
    assert_eq!(kw.kind, HighlightKind::Keyword);
    assert!(
        spans
            .iter()
            .any(|s| s.kind == HighlightKind::Literal && src[s.start..s.end].contains("hi"))
    );
}

#[test]
fn shell_comment_is_comment_to_end_of_line() {
    let h = KeywordHighlighter::shell();
    let src = "echo hi # a comment\nls";
    let spans = h.highlight(src);
    covers(&spans, src.len());
    assert!(
        spans
            .iter()
            .any(|s| s.kind == HighlightKind::Comment && src[s.start..s.end].contains("comment"))
    );
}

#[test]
fn python_keywords_classified() {
    let h = KeywordHighlighter::python();
    let src = "def f(): return 1";
    let spans = h.highlight(src);
    covers(&spans, src.len());
    assert!(
        spans
            .iter()
            .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "def")
    );
    assert!(
        spans
            .iter()
            .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "return")
    );
}

#[test]
fn plain_text_is_one_plain_span() {
    let h = KeywordHighlighter::rust();
    let src = "justwords here";
    let spans = h.highlight(src);
    covers(&spans, src.len());
    assert!(
        spans
            .iter()
            .all(|s| s.kind == HighlightKind::Plain || s.kind == HighlightKind::Identifier)
    );
}

#[test]
fn empty_input_yields_no_spans() {
    assert!(KeywordHighlighter::rust().highlight("").is_empty());
}

#[test]
fn for_language_selects_or_falls_back() {
    assert_eq!(KeywordHighlighter::for_language("rust").language(), "rust");
    assert_eq!(
        KeywordHighlighter::for_language("python").language(),
        "python"
    );
    assert_eq!(KeywordHighlighter::for_language("sh").language(), "shell");
    assert_eq!(
        KeywordHighlighter::for_language("brainfuck").language(),
        "plain"
    );
}

/// Additional contract test written as part of this TDD cycle (before impl).
/// Exercises multi-line content similar to fixtures/org/code-block-simple.org
/// and the core "covers full source without gaps" invariant required by the
/// Highlighter trait.
#[test]
fn rust_multiline_block_and_fixture_style() {
    let h = KeywordHighlighter::rust();
    // Inner content from fixtures/org/code-block-simple.org (verbatim between fences)
    let src = "fn main() {\n    println!(\"hi\");\n}";
    let spans = h.highlight(src);
    covers(&spans, src.len());
    // At least the 'fn' keyword classified
    assert!(
        spans
            .iter()
            .any(|s| { &src[s.start..s.end] == "fn" && s.kind == HighlightKind::Keyword })
    );
    // String literal captured
    assert!(
        spans
            .iter()
            .any(|s| { s.kind == HighlightKind::Literal && src[s.start..s.end].contains("hi") })
    );
}
