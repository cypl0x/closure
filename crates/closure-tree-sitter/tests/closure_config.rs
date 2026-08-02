//! "config.org syntax — It's just a src block. Which kind of syntax is
//! this? Do we have a treesitter grammar for that?"
//!
//! It is a key/value language of closure's own, and until now nothing
//! knew that: the block was painted as prose, so a key, its value and
//! the `=` between them were one undifferentiated run. The honest
//! answer to "which kind of syntax" is that closure should be able to
//! show you it knows — which means a highlighter for it, in the same
//! place every other block language has one.
//!
//! No new mechanism: `KeywordHighlighter` already serves the `src`
//! blocks, so `closure-config` becomes a language it knows rather than
//! a fifth way of colouring text.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_tree_sitter::{HighlightKind, Highlighter, KeywordHighlighter};

fn kinds(source: &str) -> Vec<(HighlightKind, String)> {
    let h = KeywordHighlighter::for_language("closure-config");
    h.highlight(source)
        .into_iter()
        .map(|s| (s.kind, source[s.start..s.end].to_owned()))
        .collect()
}

#[test]
fn the_language_is_recognised() {
    assert_eq!(
        KeywordHighlighter::for_language("closure-config").language(),
        "closure-config"
    );
}

#[test]
fn a_key_is_a_keyword() {
    let spans = kinds("theme = dark\n");
    assert!(
        spans
            .iter()
            .any(|(k, t)| *k == HighlightKind::Keyword && t == "theme"),
        "{spans:?}"
    );
}

#[test]
fn the_value_is_a_literal() {
    let spans = kinds("theme = dark\n");
    assert!(
        spans
            .iter()
            .any(|(k, t)| *k == HighlightKind::Literal && t.contains("dark")),
        "{spans:?}"
    );
}

#[test]
fn a_comment_is_a_comment() {
    // Every line of the generated config is commented; they are how
    // the file documents itself.
    let spans = kinds("# How you type.\ntheme = dark\n");
    assert!(
        spans
            .iter()
            .any(|(k, t)| *k == HighlightKind::Comment && t.contains("How you type")),
        "{spans:?}"
    );
}

#[test]
fn a_commented_out_setting_is_still_a_comment() {
    // The generated file comments out every key that has no default,
    // so this is most of it.
    let spans = kinds("# assets_dir = assets\n");
    assert!(
        spans
            .iter()
            .all(|(k, _)| *k == HighlightKind::Comment || *k == HighlightKind::Plain),
        "a commented key was highlighted as live config: {spans:?}"
    );
}

#[test]
fn the_spans_cover_the_source_without_gaps() {
    // The trait's contract, and what lets a shell fold them straight
    // into a renderer.
    let source = "# a comment\ntheme = dark\nwrap = true\n";
    let spans = KeywordHighlighter::for_language("closure-config").highlight(source);
    let mut at = 0usize;
    for span in &spans {
        assert_eq!(span.start, at, "gap or overlap: {spans:?}");
        at = span.end;
    }
    assert_eq!(at, source.len());
}

#[test]
fn an_unknown_language_is_still_plain() {
    // Nothing else changes shape because this was added.
    assert_eq!(
        KeywordHighlighter::for_language("brainfuck").language(),
        "plain"
    );
}
