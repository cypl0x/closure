//! Lossless Emacs org-mode parser and printer.
//!
//! Invariant I1 (byte-exact roundtrip on the golden corpus) is the core
//! guarantee of this crate: for every `s` under `fixtures/org/`,
//! `print(parse(s)?) == s` byte-for-byte.
//!
//! The parser is a hand-written line-cursor classifier that captures each
//! region's source span into a shared `Arc<str>` held by the document.
//! Printing slices back into that source, so unedited documents roundtrip
//! by construction. As structured editing lands in later M1 cycles,
//! mutated nodes will set a dirty flag and re-serialize from structured
//! fields; unedited nodes continue to roundtrip byte-exactly.
//!
//! `Span` is deliberately `pub(crate)`. Nothing outside `closure-org` ever
//! sees a byte offset — that's the firewall that keeps CRDT, shells, and
//! adapters clean (spec invariants I7, I8).

#![forbid(unsafe_code)]

use std::sync::Arc;
use thiserror::Error;

/// A parsed org document.
///
/// The document owns its source text once (via [`Arc<str>`]) and every
/// `Node` references a slice of it by `Span`. Cycle 3 will add a `roots`
/// field for parsed headlines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgDoc {
    source: Arc<str>,
    preamble: Vec<Node>,
}

impl OrgDoc {
    /// Nodes preceding any headline. In the current M1 cycle every node
    /// lives here; headline parsing arrives in a later cycle.
    #[must_use]
    pub fn preamble(&self) -> &[Node] {
        &self.preamble
    }

    fn source_of(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }
}

/// Byte range into [`OrgDoc::source`]. Crate-internal by design: exposing
/// byte offsets to the kernel or shells would break the span firewall
/// described in `docs/architecture.md`. The type is `pub(crate)` and no
/// public API accepts or returns one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A top-level construct in an org file. [`Node::kind`] exposes the
/// classification; the source bytes are retrieved through [`OrgDoc`] (not
/// yet exposed — will land with structured editing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    kind: NodeKind,
    span: Span,
}

/// Coarse classification of a [`Node`]. Structured fields per kind arrive
/// in later M1 cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A whitespace-only line (possibly empty).
    BlankLine,
    /// A `#` comment line starting at column 0, followed by space or EOL.
    Comment,
    /// A `#+KEY: value` metadata line starting at column 0.
    Keyword,
    /// One or more adjacent non-blank, non-comment, non-keyword lines.
    Paragraph,
}

impl Node {
    /// The coarse classification of this node.
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }
}

/// Failure mode while parsing an org document.
///
/// The current parser is total — any byte sequence is valid org from this
/// crate's point of view — but the `Result` return keeps the public
/// signature stable as richer parsing lands.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Parse an org document. See [`OrgDoc`] for the tree shape.
#[allow(clippy::must_use_candidate)]
pub fn parse(src: &str) -> Result<OrgDoc, ParseError> {
    let source: Arc<str> = Arc::from(src);
    let mut preamble: Vec<Node> = Vec::new();
    let mut paragraph: Option<Span> = None;
    let mut cursor: usize = 0;

    for line in src.split_inclusive('\n') {
        let span = Span {
            start: cursor,
            end: cursor + line.len(),
        };
        cursor += line.len();

        match classify(line) {
            LineKind::Paragraph => match &mut paragraph {
                Some(p) => p.end = span.end,
                None => paragraph = Some(span),
            },
            LineKind::Blank => {
                flush_paragraph(&mut paragraph, &mut preamble);
                preamble.push(Node {
                    kind: NodeKind::BlankLine,
                    span,
                });
            }
            LineKind::Comment => {
                flush_paragraph(&mut paragraph, &mut preamble);
                preamble.push(Node {
                    kind: NodeKind::Comment,
                    span,
                });
            }
            LineKind::Keyword => {
                flush_paragraph(&mut paragraph, &mut preamble);
                preamble.push(Node {
                    kind: NodeKind::Keyword,
                    span,
                });
            }
        }
    }
    flush_paragraph(&mut paragraph, &mut preamble);

    Ok(OrgDoc { source, preamble })
}

/// Serialise an org document back to its source text. Concatenates each
/// node's source slice, which guarantees I1 (byte-exact roundtrip) for
/// unedited documents by construction.
#[must_use]
pub fn print(doc: &OrgDoc) -> String {
    let total: usize = doc
        .preamble
        .iter()
        .map(|n| n.span().end - n.span().start)
        .sum();
    let mut out = String::with_capacity(total);
    for n in &doc.preamble {
        out.push_str(doc.source_of(n.span()));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Blank,
    Comment,
    Keyword,
    Paragraph,
}

fn flush_paragraph(paragraph: &mut Option<Span>, out: &mut Vec<Node>) {
    if let Some(span) = paragraph.take() {
        out.push(Node {
            kind: NodeKind::Paragraph,
            span,
        });
    }
}

fn classify(line: &str) -> LineKind {
    // Strip terminator newline for classification only; callers keep the
    // full original bytes including the terminator in the span.
    let body = line.strip_suffix('\n').unwrap_or(line);

    if body.trim().is_empty() {
        return LineKind::Blank;
    }
    if let Some(rest) = body.strip_prefix("#+")
        && is_keyword_header(rest)
    {
        return LineKind::Keyword;
    }
    if let Some(rest) = body.strip_prefix('#')
        && (rest.is_empty() || rest.starts_with([' ', '\t']))
    {
        return LineKind::Comment;
    }
    LineKind::Paragraph
}

/// Recognise the body of a `#+KEY:` header (the text following `#+`).
/// Returns true when the text matches `NAME:...` where `NAME` is one or
/// more ASCII alphanumerics, underscores, or hyphens and begins with a
/// letter or underscore.
fn is_keyword_header(rest: &str) -> bool {
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    for ch in chars {
        if ch == ':' {
            return true;
        }
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            return false;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn roundtrip(src: &str) {
        let parsed = parse(src).expect("parse is infallible in M1 cycle 1");
        assert_eq!(print(&parsed), src, "roundtrip mismatch for {src:?}");
    }

    #[test]
    fn empty_input_parses_to_empty_doc() {
        let doc = parse("").expect("parse");
        assert!(doc.preamble().is_empty());
        roundtrip("");
    }

    #[test]
    fn single_newline_is_one_blank_line() {
        let doc = parse("\n").expect("parse");
        assert_eq!(doc.preamble().len(), 1);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::BlankLine);
        roundtrip("\n");
    }

    #[test]
    fn paragraph_coalesces_adjacent_lines() {
        let src = "a\nb\nc\n";
        let doc = parse(src).expect("parse");
        assert_eq!(doc.preamble().len(), 1);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
        roundtrip(src);
    }

    #[test]
    fn blank_between_paragraphs_splits_them() {
        let doc = parse("a\n\nb\n").expect("parse");
        assert_eq!(doc.preamble().len(), 3);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
        assert_eq!(doc.preamble()[1].kind(), NodeKind::BlankLine);
        assert_eq!(doc.preamble()[2].kind(), NodeKind::Paragraph);
    }

    #[test]
    fn comment_requires_column_zero() {
        assert_eq!(classify("# hi\n"), LineKind::Comment);
        assert_eq!(classify("#\n"), LineKind::Comment);
        assert_eq!(classify("#\ttab-comment\n"), LineKind::Comment);
        assert_eq!(classify("  # hi\n"), LineKind::Paragraph);
        assert_eq!(classify("#foo\n"), LineKind::Paragraph);
    }

    #[test]
    fn keyword_requires_column_zero_and_colon() {
        assert_eq!(classify("#+TITLE: x\n"), LineKind::Keyword);
        assert_eq!(classify("#+FILETAGS: :t:\n"), LineKind::Keyword);
        assert_eq!(classify("#+AUTHOR:\n"), LineKind::Keyword);
        assert_eq!(classify("#+TITLE x\n"), LineKind::Paragraph);
        assert_eq!(classify("  #+TITLE: x\n"), LineKind::Paragraph);
    }

    #[test]
    fn file_without_trailing_newline_roundtrips() {
        roundtrip("no trailing newline");
        roundtrip("#+TITLE: x");
        roundtrip("");
    }

    #[test]
    fn span_covers_full_line_including_newline() {
        let doc = parse("   \n").expect("parse");
        assert_eq!(doc.preamble().len(), 1);
        let n = &doc.preamble()[0];
        assert_eq!(n.kind(), NodeKind::BlankLine);
        assert_eq!(doc.source_of(n.span()), "   \n");
    }
}
