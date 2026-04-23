//! Lossless Emacs org-mode parser and printer.
//!
//! Invariant I1 (byte-exact roundtrip on the golden corpus) is the core
//! guarantee of this crate: for every `s` under `fixtures/org/`,
//! `print(parse(s)?) == s` byte-for-byte.
//!
//! The parser is a hand-written line-cursor classifier that captures each
//! region's source span into a shared `Arc<str>` held by the document.
//! Printing slices back into that source, so unedited documents roundtrip
//! by construction. Later cycles add structured editing where mutated
//! nodes set a dirty flag and re-serialize from structured fields.
//!
//! `Span` is deliberately `pub(crate)`. Nothing outside `closure-org` ever
//! sees a byte offset — that's the firewall that keeps CRDT, shells, and
//! adapters clean (spec invariants I7, I8).

#![forbid(unsafe_code)]

use std::sync::Arc;
use thiserror::Error;

/// A parsed org document.
///
/// Owns its source once (via [`Arc<str>`]) and every [`Node`] or
/// [`Headline`] references a slice of it by span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgDoc {
    source: Arc<str>,
    preamble: Vec<Node>,
    roots: Vec<Headline>,
}

impl OrgDoc {
    /// Nodes preceding the first headline.
    #[must_use]
    pub fn preamble(&self) -> &[Node] {
        &self.preamble
    }

    /// Top-level headlines. In the current cycle they are siblings; the
    /// nesting cycle restructures into a tree.
    #[must_use]
    pub fn roots(&self) -> &[Headline] {
        &self.roots
    }

    fn source_of(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }
}

/// Byte range into [`OrgDoc::source`]. Crate-internal by design: exposing
/// byte offsets to the kernel or shells would break the span firewall
/// described in `docs/architecture.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A line-level construct. [`Node::kind`] exposes the classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    source: Arc<str>,
    kind: NodeKind,
    span: Span,
}

/// Coarse classification of a [`Node`]. Structured fields per kind arrive
/// in later cycles.
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
    /// Classification of this node.
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    /// The verbatim source slice that produced this node.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source[self.span.start..self.span.end]
    }
}

/// A parsed headline with its title, level, body, and (post-nesting)
/// children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    source: Arc<str>,
    header_span: Span,
    title_span: Span,
    level: u8,
    body: Vec<Node>,
    children: Vec<Self>,
}

impl Headline {
    /// Nesting level (number of leading `*`).
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// The title text, excluding leading stars and the separating space,
    /// and the trailing newline.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.source[self.title_span.start..self.title_span.end]
    }

    /// Verbatim source of the header line (including trailing newline if
    /// any).
    #[must_use]
    pub fn header(&self) -> &str {
        &self.source[self.header_span.start..self.header_span.end]
    }

    /// Nodes in this headline's body (after header, before children or
    /// next sibling).
    #[must_use]
    pub fn body(&self) -> &[Node] {
        &self.body
    }

    /// Child headlines (populated once nesting lands).
    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
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
    let mut roots: Vec<Headline> = Vec::new();
    let mut paragraph: Option<Span> = None;
    let mut cursor: usize = 0;

    for line in src.split_inclusive('\n') {
        let span = Span {
            start: cursor,
            end: cursor + line.len(),
        };
        cursor += line.len();

        if let Some(head) = classify_heading(line, span) {
            flush_paragraph(
                &source,
                &mut paragraph,
                target_nodes(&mut roots, &mut preamble),
            );
            roots.push(Headline {
                source: Arc::clone(&source),
                header_span: head.header_span,
                title_span: head.title_span,
                level: head.level,
                body: Vec::new(),
                children: Vec::new(),
            });
            continue;
        }

        match classify_line(line) {
            LineKind::Paragraph => match &mut paragraph {
                Some(p) => p.end = span.end,
                None => paragraph = Some(span),
            },
            LineKind::Blank => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::BlankLine,
                        span,
                    },
                );
            }
            LineKind::Comment => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::Comment,
                        span,
                    },
                );
            }
            LineKind::Keyword => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::Keyword,
                        span,
                    },
                );
            }
        }
    }
    flush_paragraph(
        &source,
        &mut paragraph,
        target_nodes(&mut roots, &mut preamble),
    );

    Ok(OrgDoc {
        source,
        preamble,
        roots: nest(roots),
    })
}

/// Restructure a flat list of headlines into a tree by level. A heading
/// with level N becomes a child of the nearest preceding heading with
/// level < N; otherwise it is a root. Level jumps (e.g. 1 → 3) are
/// accepted and the deeper heading is attached to the nearest
/// lower-level ancestor.
fn nest(flat: Vec<Headline>) -> Vec<Headline> {
    let mut roots: Vec<Headline> = Vec::new();
    for h in flat {
        attach(&mut roots, h);
    }
    roots
}

fn attach(siblings: &mut Vec<Headline>, h: Headline) {
    match siblings.last_mut() {
        Some(last) if last.level < h.level => attach(&mut last.children, h),
        _ => siblings.push(h),
    }
}

/// Serialise an org document back to its source text. Concatenates each
/// node's source slice, which guarantees I1 (byte-exact roundtrip) for
/// unedited documents by construction.
#[must_use]
pub fn print(doc: &OrgDoc) -> String {
    let mut out = String::with_capacity(doc.source.len());
    for n in &doc.preamble {
        out.push_str(doc.source_of(n.span));
    }
    for h in &doc.roots {
        print_headline(doc, h, &mut out);
    }
    out
}

fn print_headline(doc: &OrgDoc, h: &Headline, out: &mut String) {
    out.push_str(doc.source_of(h.header_span));
    for n in &h.body {
        out.push_str(doc.source_of(n.span));
    }
    for c in &h.children {
        print_headline(doc, c, out);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Blank,
    Comment,
    Keyword,
    Paragraph,
}

struct HeadInfo {
    header_span: Span,
    title_span: Span,
    level: u8,
}

#[allow(clippy::ptr_arg)]
fn target_nodes<'a>(
    roots: &'a mut Vec<Headline>,
    preamble: &'a mut Vec<Node>,
) -> &'a mut Vec<Node> {
    if let Some(last) = roots.last_mut() {
        &mut last.body
    } else {
        preamble
    }
}

#[allow(clippy::ptr_arg)]
fn push_node(roots: &mut Vec<Headline>, preamble: &mut Vec<Node>, node: Node) {
    target_nodes(roots, preamble).push(node);
}

fn flush_paragraph(source: &Arc<str>, paragraph: &mut Option<Span>, out: &mut Vec<Node>) {
    if let Some(span) = paragraph.take() {
        out.push(Node {
            source: Arc::clone(source),
            kind: NodeKind::Paragraph,
            span,
        });
    }
}

/// Recognise a heading line. A heading is `*+` at column 0 followed by
/// either end-of-line (empty title) or at least one space/tab (title
/// follows).
fn classify_heading(line: &str, span: Span) -> Option<HeadInfo> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let stars = body.chars().take_while(|&c| c == '*').count();
    if stars == 0 {
        return None;
    }
    let after = &body[stars..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let level = u8::try_from(stars).unwrap_or(u8::MAX);
    let title_skip = after
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let title_start_offset = stars + title_skip;
    let title_end_offset = stars + after.len();
    let title_span = Span {
        start: span.start + title_start_offset,
        end: span.start + title_end_offset,
    };
    Some(HeadInfo {
        header_span: span,
        title_span,
        level,
    })
}

fn classify_line(line: &str) -> LineKind {
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
        let parsed = parse(src).expect("parse is infallible");
        assert_eq!(print(&parsed), src, "roundtrip mismatch for {src:?}");
    }

    #[test]
    fn empty_input_parses_to_empty_doc() {
        let doc = parse("").expect("parse");
        assert!(doc.preamble().is_empty());
        assert!(doc.roots().is_empty());
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
        assert_eq!(classify_line("# hi\n"), LineKind::Comment);
        assert_eq!(classify_line("#\n"), LineKind::Comment);
        assert_eq!(classify_line("#\ttab-comment\n"), LineKind::Comment);
        assert_eq!(classify_line("  # hi\n"), LineKind::Paragraph);
        assert_eq!(classify_line("#foo\n"), LineKind::Paragraph);
    }

    #[test]
    fn keyword_requires_column_zero_and_colon() {
        assert_eq!(classify_line("#+TITLE: x\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+FILETAGS: :t:\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+AUTHOR:\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+TITLE x\n"), LineKind::Paragraph);
        assert_eq!(classify_line("  #+TITLE: x\n"), LineKind::Paragraph);
    }

    #[test]
    fn file_without_trailing_newline_roundtrips() {
        roundtrip("no trailing newline");
        roundtrip("#+TITLE: x");
        roundtrip("");
    }

    #[test]
    fn heading_basic_roundtrips() {
        roundtrip("* Hello\n");
        roundtrip("* First\n* Second\n* Third\n");
        roundtrip("*\n");
        roundtrip("**\n");
    }

    #[test]
    fn heading_with_body_roundtrips() {
        roundtrip("* Hello\nbody\n");
    }
}
