#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_tree_sitter::{HighlightKind, Highlighter, NoOpHighlighter};

#[test]
fn noop_classifies_whole_input_as_plain() {
    let h = NoOpHighlighter;
    assert_eq!(h.language(), "plain");
    let highlights = h.highlight("fn main() {}");
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 0);
    assert_eq!(highlights[0].end, 12);
    assert_eq!(highlights[0].kind, HighlightKind::Plain);
}

#[test]
fn empty_input_produces_zero_length_span() {
    let highlights = NoOpHighlighter.highlight("");
    assert_eq!(highlights[0].start, 0);
    assert_eq!(highlights[0].end, 0);
}
