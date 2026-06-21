//! V6b: the TUI file view picks the real tree-sitter highlighter when the
//! `tree-sitter` feature is on, falling back to the dep-free
//! `KeywordHighlighter` otherwise. The distinguishing proof: real parsing
//! understands *string context*, so keyword-looking words inside a string
//! are NOT keyword-highlighted (the keyword highlighter would mis-mark
//! them).

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(feature = "tree-sitter")]
use closure_tree_sitter::HighlightKind;
use closure_tui::pick_highlighter;

#[test]
fn fallback_highlighter_still_highlights_when_feature_off() {
    // Always compiled: the picked highlighter reports the language and
    // produces gap-free spans for a shell snippet.
    let h = pick_highlighter("bash");
    let spans = h.highlight("echo hi\n");
    assert!(!spans.is_empty());
    let mut pos = 0;
    for s in &spans {
        assert_eq!(s.start, pos);
        pos = s.end;
    }
    assert_eq!(pos, "echo hi\n".len(), "gap-free coverage");
}

#[cfg(feature = "tree-sitter")]
#[test]
fn real_grammar_respects_string_context() {
    let h = pick_highlighter("bash");
    let src = "x=\"if then fi\"\n";
    let str_start = src.find('"').unwrap();
    let str_end = src.rfind('"').unwrap() + 1;
    let spans = h.highlight(src);
    // No keyword highlight inside the quoted string — real parsing knows
    // `if`/`then`/`fi` are string content here, not shell keywords.
    let kw_in_string = spans
        .iter()
        .any(|s| s.kind == HighlightKind::Keyword && s.start >= str_start && s.end <= str_end);
    assert!(
        !kw_in_string,
        "tree-sitter must not keyword-highlight string content: {spans:?}"
    );
}
