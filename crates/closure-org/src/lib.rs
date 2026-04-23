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
    todo_span: Option<Span>,
    priority_span: Option<Span>,
    tag_spans: Vec<Span>,
    properties: Option<Properties>,
    level: u8,
    body: Vec<Node>,
    children: Vec<Self>,
}

/// Parsed `:PROPERTIES:` ... `:END:` drawer attached to a [`Headline`].
/// Entry order is preserved from the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    source: Arc<str>,
    drawer_span: Span,
    entries: Vec<PropertyEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PropertyEntry {
    key_span: Span,
    value_span: Span,
}

impl Properties {
    /// Lookup a property value by key name (case-sensitive). Returns the
    /// raw value slice without surrounding whitespace.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| &self.source[e.key_span.start..e.key_span.end] == key)
            .map(|e| &self.source[e.value_span.start..e.value_span.end])
    }

    /// Shorthand for `get("ID")`. Invariant I2 pins stable ULID block IDs
    /// into this property.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.get("ID")
    }

    /// Number of entries in the drawer.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the drawer has zero entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate `(key, value)` pairs in source order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(move |e| {
            (
                &self.source[e.key_span.start..e.key_span.end],
                &self.source[e.value_span.start..e.value_span.end],
            )
        })
    }
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

    /// TODO keyword if present (e.g. `"TODO"`, `"DONE"`).
    #[must_use]
    pub fn todo(&self) -> Option<&str> {
        self.todo_span.map(|s| &self.source[s.start..s.end])
    }

    /// Priority letter if present. For `[#A]` returns `Some('A')`.
    #[must_use]
    pub fn priority(&self) -> Option<char> {
        let span = self.priority_span?;
        let slice = &self.source[span.start..span.end];
        slice.strip_prefix("[#")?.chars().next()
    }

    /// Trailing headline tags in source order.
    #[must_use]
    pub fn tags(&self) -> Vec<&str> {
        self.tag_spans
            .iter()
            .map(|s| &self.source[s.start..s.end])
            .collect()
    }

    /// The headline's `:PROPERTIES:` drawer if one was parsed.
    #[must_use]
    pub const fn properties(&self) -> Option<&Properties> {
        self.properties.as_ref()
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
#[allow(clippy::must_use_candidate, clippy::too_many_lines)]
pub fn parse(src: &str) -> Result<OrgDoc, ParseError> {
    let source: Arc<str> = Arc::from(src);
    let mut preamble: Vec<Node> = Vec::new();
    let mut roots: Vec<Headline> = Vec::new();
    let mut paragraph: Option<Span> = None;

    let lines: Vec<(&str, Span)> = {
        let mut v = Vec::new();
        let mut cursor = 0usize;
        for line in src.split_inclusive('\n') {
            let s = Span {
                start: cursor,
                end: cursor + line.len(),
            };
            cursor += line.len();
            v.push((line, s));
        }
        v
    };

    let mut i = 0;
    while i < lines.len() {
        let (line, span) = lines[i];

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
                todo_span: head.todo_span,
                priority_span: head.priority_span,
                tag_spans: head.tag_spans,
                properties: None,
                level: head.level,
                body: Vec::new(),
                children: Vec::new(),
            });
            i += 1;
            continue;
        }

        // Property drawer: immediately after a heading (body empty, no
        // drawer yet), a `:PROPERTIES:` / `:END:` block with `:KEY: value`
        // entries between is captured structurally and excluded from the
        // headline's body.
        if paragraph.is_none()
            && let Some(h) = roots.last()
            && h.body.is_empty()
            && h.properties.is_none()
            && trimmed_line(line) == ":PROPERTIES:"
            && let Some((drawer, next_i)) = scan_property_drawer(&lines, i, &source)
        {
            if let Some(h_mut) = roots.last_mut() {
                h_mut.properties = Some(drawer);
            }
            i = next_i;
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
        i += 1;
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

fn trimmed_line(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line).trim()
}

/// Scan `lines[start..]` for a well-formed `:PROPERTIES:` / `:END:`
/// drawer. Returns the drawer and the index of the line after `:END:`,
/// or `None` if the structure isn't a valid drawer.
fn scan_property_drawer(
    lines: &[(&str, Span)],
    start: usize,
    source: &Arc<str>,
) -> Option<(Properties, usize)> {
    let (_, start_span) = lines[start];
    let mut entries: Vec<PropertyEntry> = Vec::new();
    let mut j = start + 1;
    while j < lines.len() {
        let (ln, sp) = lines[j];
        let trim = trimmed_line(ln);
        if trim == ":END:" {
            let drawer_span = Span {
                start: start_span.start,
                end: sp.end,
            };
            return Some((
                Properties {
                    source: Arc::clone(source),
                    drawer_span,
                    entries,
                },
                j + 1,
            ));
        }
        if let Some(entry) = parse_property_entry(ln, sp) {
            entries.push(entry);
            j += 1;
            continue;
        }
        return None; // malformed
    }
    None
}

fn parse_property_entry(line: &str, span: Span) -> Option<PropertyEntry> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let rest = body.strip_prefix(':')?;
    let colon_pos = rest.find(':')?;
    let key_str = &rest[..colon_pos];
    if key_str.is_empty()
        || !key_str
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let key_span = Span {
        start: span.start + 1,
        end: span.start + 1 + colon_pos,
    };
    let after_colon_pos = 1 + colon_pos + 1;
    let after_colon = &body[after_colon_pos..];
    let leading_ws = after_colon.len() - after_colon.trim_start_matches([' ', '\t']).len();
    let value_content = after_colon.trim_matches([' ', '\t']);
    let value_span = Span {
        start: span.start + after_colon_pos + leading_ws,
        end: span.start + after_colon_pos + leading_ws + value_content.len(),
    };
    Some(PropertyEntry {
        key_span,
        value_span,
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
    if let Some(p) = &h.properties {
        out.push_str(doc.source_of(p.drawer_span));
    }
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
    todo_span: Option<Span>,
    priority_span: Option<Span>,
    tag_spans: Vec<Span>,
    level: u8,
}

const TODO_KEYWORDS: &[&str] = &["TODO", "DONE"];

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
/// follows). If a title is present, the content between the stars and
/// the end of the line (before the newline) is further split into:
/// optional leading TODO keyword, optional `[#X]` priority, title, and
/// optional trailing `:tag:tag:` list.
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

    let ws_skip = after
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    // Content region: between the stars+space and the end of body.
    let content_start = span.start + stars + ws_skip;
    let content_end = span.start + stars + after.len();
    let content = &body[stars + ws_skip..stars + after.len()];

    // Strip trailing tags first (right-to-left).
    let (content_before_tags_end, tag_spans) =
        strip_trailing_tags(content, content_start, content_end);

    // After trimming trailing whitespace before tags, the TODO+priority+title
    // live in [content_start, title_trim_end).
    let pre_tags = &content[..(content_before_tags_end - content_start)];
    let title_trim_end = content_start + pre_tags.trim_end_matches([' ', '\t']).len();
    let working = &content[..(title_trim_end - content_start)];

    // Parse TODO keyword from left.
    let (todo_span, after_todo_str, after_todo_pos) = strip_leading_todo(working, content_start);

    // Parse priority `[#X]` from left.
    let (priority_span, after_prio_str, after_prio_pos) =
        strip_leading_priority(after_todo_str, after_todo_pos);

    // Remainder is the title; trim leading whitespace.
    let title_leading_ws =
        after_prio_str.len() - after_prio_str.trim_start_matches([' ', '\t']).len();
    let title_start = after_prio_pos + title_leading_ws;
    let title_end = title_trim_end;
    let title_span = Span {
        start: title_start,
        end: title_end,
    };

    Some(HeadInfo {
        header_span: span,
        title_span,
        todo_span,
        priority_span,
        tag_spans,
        level,
    })
}

/// Right-to-left strip a trailing `:tag:tag:` list from the content line.
/// Returns `(end_position_before_tags, tag_spans)` where `tag_spans`
/// cover the individual tag names (not the colons) in source order. The
/// tag list requires at least one whitespace character before its
/// opening `:` for validity.
fn strip_trailing_tags(
    content: &str,
    content_start: usize,
    content_end: usize,
) -> (usize, Vec<Span>) {
    let trimmed = content.trim_end_matches([' ', '\t']);
    if !trimmed.ends_with(':') || trimmed.len() < 2 {
        return (content_end, Vec::new());
    }
    let bytes = trimmed.as_bytes();
    let mut cursor = trimmed.len() - 1; // position of trailing ':'
    let mut tags_rev: Vec<(usize, usize)> = Vec::new();

    loop {
        // cursor points at a ':'. Walk leftward over tag-chars for the name.
        let name_end = cursor;
        let mut name_start = cursor;
        while name_start > 0 && is_tag_char(bytes[name_start - 1] as char) {
            name_start -= 1;
        }
        if name_start == name_end {
            // Empty name: the ':' at cursor has no tag chars before it.
            // Either the tag list is degenerate (abort and treat as title)
            // or we have already collected tags and hit the structure
            // boundary. If nothing collected, abort.
            if tags_rev.is_empty() {
                return (content_end, Vec::new());
            }
            break;
        }
        tags_rev.push((name_start, name_end));

        if name_start == 0 {
            // Tag list starts at content start — no preceding whitespace,
            // so this isn't a well-formed trailing tag block.
            return (content_end, Vec::new());
        }
        match bytes[name_start - 1] {
            b':' => {
                cursor = name_start - 1;
            }
            b' ' | b'\t' => break,
            _ => return (content_end, Vec::new()),
        }
    }

    if tags_rev.is_empty() {
        return (content_end, Vec::new());
    }

    // Earliest name_start is the last element of tags_rev.
    let first_name_start = tags_rev.last().map_or(content.len(), |t| t.0);
    // The opening ':' of the tag list sits at first_name_start - 1.
    let first_colon_pos = first_name_start.saturating_sub(1);
    let before_tags_trim = content[..first_colon_pos]
        .trim_end_matches([' ', '\t'])
        .len();

    tags_rev.reverse();
    let tag_spans = tags_rev
        .into_iter()
        .map(|(s, e)| Span {
            start: content_start + s,
            end: content_start + e,
        })
        .collect();
    (content_start + before_tags_trim, tag_spans)
}

const fn is_tag_char(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '@' | '#' | '%')
}

/// Try to strip a leading TODO keyword. Returns the matched span (if any)
/// plus the remaining substring and its absolute start position.
fn strip_leading_todo(working: &str, base: usize) -> (Option<Span>, &str, usize) {
    for &kw in TODO_KEYWORDS {
        if let Some(rest) = working.strip_prefix(kw)
            && (rest.is_empty() || rest.starts_with([' ', '\t']))
        {
            return (
                Some(Span {
                    start: base,
                    end: base + kw.len(),
                }),
                rest,
                base + kw.len(),
            );
        }
    }
    (None, working, base)
}

/// Try to strip a leading priority `[#X]` (after optional whitespace).
fn strip_leading_priority(working: &str, base: usize) -> (Option<Span>, &str, usize) {
    let ws = working.len() - working.trim_start_matches([' ', '\t']).len();
    let after_ws = &working[ws..];
    let prio_start = base + ws;
    if after_ws.len() >= 4 && after_ws.starts_with("[#") && after_ws.as_bytes()[3] == b']' {
        let inside = after_ws.as_bytes()[2];
        if inside.is_ascii_alphabetic() {
            return (
                Some(Span {
                    start: prio_start,
                    end: prio_start + 4,
                }),
                &after_ws[4..],
                prio_start + 4,
            );
        }
    }
    (None, working, base)
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
