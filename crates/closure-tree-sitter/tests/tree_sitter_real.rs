//! V6a: real tree-sitter highlighting behind the `tree-sitter` feature.
//! Default builds never compile this (the dep-free `KeywordHighlighter`
//! is the hermetic default); `just tree-sitter` runs it. Verifies a real
//! grammar parse classifies comments/strings and still satisfies the
//! `Highlighter` coverage invariant (gap-free, non-overlapping, [0,len)).

#![cfg(feature = "tree-sitter")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_tree_sitter::{Highlight, HighlightKind, Highlighter, TsHighlighter};

fn assert_full_coverage(spans: &[Highlight], len: usize) {
    let mut pos = 0;
    for s in spans {
        assert_eq!(s.start, pos, "no gap / overlap before {s:?}");
        assert!(s.end >= s.start);
        pos = s.end;
    }
    assert_eq!(pos, len, "spans cover the whole source");
}

#[test]
fn bash_highlighter_classifies_comment_and_string() {
    let h = TsHighlighter::for_language("bash").expect("bash grammar available");
    let src = "# a comment\necho \"hello\"\n";
    let spans = h.highlight(src);
    assert_full_coverage(&spans, src.len());

    let kind_at = |needle: &str| {
        let at = src.find(needle).unwrap();
        spans
            .iter()
            .find(|s| s.start <= at && at < s.end)
            .map(|s| s.kind)
    };
    assert_eq!(kind_at("# a comment"), Some(HighlightKind::Comment));
    assert_eq!(kind_at("\"hello\""), Some(HighlightKind::Literal));
}

#[test]
fn language_reports_bash() {
    let h = TsHighlighter::for_language("bash").expect("bash");
    assert_eq!(h.language(), "bash");
}

#[test]
fn unsupported_language_is_none() {
    assert!(TsHighlighter::for_language("brainfuck").is_none());
}

/// Helper: the highlight kind covering the first byte of `needle`.
fn kind_at(spans: &[Highlight], src: &str, needle: &str) -> Option<HighlightKind> {
    let at = src.find(needle).unwrap();
    spans
        .iter()
        .find(|s| s.start <= at && at < s.end)
        .map(|s| s.kind)
}

#[test]
fn rust_grammar_classifies_comment_and_string() {
    // D5: a real Rust grammar — `line_comment` → Comment, `string_literal`
    // → Literal — and still gap-free.
    let h = TsHighlighter::for_language("rust").expect("rust grammar available");
    assert_eq!(h.language(), "rust");
    let src = "// note\nlet s = \"hi\";\n";
    let spans = h.highlight(src);
    assert_full_coverage(&spans, src.len());
    assert_eq!(
        kind_at(&spans, src, "// note"),
        Some(HighlightKind::Comment)
    );
    assert_eq!(kind_at(&spans, src, "\"hi\""), Some(HighlightKind::Literal));
}

#[test]
fn python_grammar_classifies_comment_and_string() {
    let h = TsHighlighter::for_language("python").expect("python grammar available");
    assert_eq!(h.language(), "python");
    let src = "# note\nx = \"hi\"\n";
    let spans = h.highlight(src);
    assert_full_coverage(&spans, src.len());
    assert_eq!(kind_at(&spans, src, "# note"), Some(HighlightKind::Comment));
    assert_eq!(kind_at(&spans, src, "\"hi\""), Some(HighlightKind::Literal));
}

#[test]
fn json_grammar_classifies_string_and_number() {
    // JSON has no comments; assert string + number literals instead.
    let h = TsHighlighter::for_language("json").expect("json grammar available");
    assert_eq!(h.language(), "json");
    let src = "{\"k\": 42}\n";
    let spans = h.highlight(src);
    assert_full_coverage(&spans, src.len());
    assert_eq!(kind_at(&spans, src, "\"k\""), Some(HighlightKind::Literal));
    assert_eq!(kind_at(&spans, src, "42"), Some(HighlightKind::Literal));
}

#[test]
fn empty_source_is_one_plain_span_or_empty() {
    let h = TsHighlighter::for_language("bash").expect("bash");
    let spans = h.highlight("");
    assert_full_coverage(&spans, 0);
}
