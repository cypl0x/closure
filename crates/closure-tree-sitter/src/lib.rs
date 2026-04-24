//! Optional tree-sitter integration for syntax highlighting inside
//! `#+BEGIN_SRC` code blocks.
//!
//! The full tree-sitter C grammar pulls unsafe code and a complicated
//! build; the crate is a placeholder until a language-loader policy is
//! picked (bundled grammars vs. per-language feature flags).

#![forbid(unsafe_code)]

/// Highlight span produced by a grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight<'a> {
    /// The language identifier, e.g. `"rust"`.
    pub language: &'a str,
    /// Source slice to highlight.
    pub source: &'a str,
}

/// Placeholder highlighter that returns a single span covering the
/// whole source with an unknown highlight name. Real grammar loading
/// lands in a later milestone.
#[must_use]
pub fn highlight<'a>(language: &'a str, source: &'a str) -> Vec<Highlight<'a>> {
    vec![Highlight { language, source }]
}
