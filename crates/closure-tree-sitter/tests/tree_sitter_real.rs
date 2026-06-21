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

#[test]
fn empty_source_is_one_plain_span_or_empty() {
    let h = TsHighlighter::for_language("bash").expect("bash");
    let spans = h.highlight("");
    assert_full_coverage(&spans, 0);
}
