//! Optional tree-sitter integration for syntax highlighting inside
//! `#+BEGIN_SRC` code blocks.
//!
//! The full tree-sitter C grammar pulls unsafe code and a complicated
//! build; the crate currently exposes the abstract API contract so
//! shells and the kernel can already integrate against it. A real
//! grammar loader (bundled vs. feature-flagged per language) lands
//! once the policy is picked.

#![forbid(unsafe_code)]

/// Coarse highlight kind. Concrete grammars map their tokens to one
/// of these so shells can render with a small, stable palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// Identifiers, function names, type names.
    Identifier,
    /// Reserved keywords.
    Keyword,
    /// String, number, bool, char literals.
    Literal,
    /// Comments.
    Comment,
    /// Operators and punctuation.
    Punctuation,
    /// Plain text (default fallback).
    Plain,
}

/// One highlight span: a byte range plus a highlight kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    /// Inclusive byte start.
    pub start: usize,
    /// Exclusive byte end.
    pub end: usize,
    /// Classification.
    pub kind: HighlightKind,
}

/// Highlighter implementation contract.
pub trait Highlighter {
    /// Language identifier this highlighter supports.
    fn language(&self) -> &str;
    /// Compute highlights for `source`. The returned spans must be
    /// non-overlapping and cover `[0, source.len())` without gaps so
    /// shells can fold them into a string-buffer renderer without
    /// re-scanning.
    fn highlight(&self, source: &str) -> Vec<Highlight>;
}

/// Default no-op highlighter: classifies the whole input as `Plain`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpHighlighter;

impl Highlighter for NoOpHighlighter {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "plain"
    }

    fn highlight(&self, source: &str) -> Vec<Highlight> {
        vec![Highlight {
            start: 0,
            end: source.len(),
            kind: HighlightKind::Plain,
        }]
    }
}
