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

    /// The document's full source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    fn source_of(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }
}

/// Failure mode for structure-preserving rewrites.
#[derive(Debug, Error)]
pub enum RewriteError {
    /// No headline exists at the given path.
    #[error("no headline at the given path")]
    NotFound,
    /// Rewriting produced source the parser could not accept.
    #[error("rewrite produced unparsable source")]
    Parse,
}

/// Rewrite a headline's title in-place and return a freshly parsed
/// [`OrgDoc`]. Byte-exact roundtrip holds on the unchanged portion of
/// the source; only the `title_span` is replaced.
pub fn rewrite_headline_title(
    doc: &OrgDoc,
    path: &[usize],
    new_title: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    src.replace_range(target.title_span.start..target.title_span.end, new_title);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set or clear the TODO keyword on a headline.
///
/// `keyword: Some("TODO")` sets the keyword (replaces the existing one
/// if present); `keyword: None` removes it. The rest of the header
/// line (priority, title, tags) is preserved verbatim.
pub fn rewrite_headline_set_todo(
    doc: &OrgDoc,
    path: &[usize],
    keyword: Option<&str>,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    // Determine the current "header content" region: between stars+space
    // and the trailing `\n`. Replace the leading TODO+space (if any) and
    // re-emit with the new keyword.
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);
    let stars = body.chars().take_while(|&c| c == '*').count();
    let after_stars = &body[stars..];
    let ws_skip = after_stars
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let content = &after_stars[ws_skip..];

    let stripped = target.todo_span.map_or(content, |span| {
        let kw_len = span.end - span.start;
        let after_kw = &content[kw_len..];
        after_kw.strip_prefix(' ').unwrap_or(after_kw)
    });

    let stars_str = "*".repeat(stars);
    let new_header = keyword.map_or_else(
        || format!("{stars_str} {stripped}\n"),
        |k| format!("{stars_str} {k} {stripped}\n"),
    );

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set or clear the `[#X]` priority on a headline.
///
/// `Some('A')` inserts a priority cookie immediately after the optional
/// TODO keyword; `None` removes any existing cookie. Title and tags
/// are preserved verbatim.
pub fn rewrite_headline_set_priority(
    doc: &OrgDoc,
    path: &[usize],
    priority: Option<char>,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);
    let stars = body.chars().take_while(|&c| c == '*').count();
    let after_stars = &body[stars..];
    let ws = after_stars
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let content = &body[stars + ws..];

    let mut prefix = String::new();
    let mut rest: &str = target.todo_span.map_or(content, |span| {
        let kw_len = span.end - span.start;
        prefix.push_str(&content[..kw_len]);
        prefix.push(' ');
        let after = &content[kw_len..];
        after.strip_prefix(' ').unwrap_or(after)
    });

    // Strip any existing `[#X]` priority cookie from `rest`.
    if rest.starts_with("[#") && rest.len() >= 4 && rest.as_bytes()[3] == b']' {
        let after = &rest[4..];
        rest = after.strip_prefix(' ').unwrap_or(after);
    }

    if let Some(p) = priority {
        use std::fmt::Write as _;
        let _ = write!(prefix, "[#{p}] ");
    }

    let stars_str = "*".repeat(stars);
    let new_header = format!("{stars_str} {prefix}{rest}\n");

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set the trailing tag list on a headline.
///
/// `tags: &[]` clears the tag block. Otherwise the trailing
/// `:tag1:tag2:` block is replaced wholesale (or appended if absent).
/// Title, TODO, and priority are preserved.
pub fn rewrite_headline_set_tags(
    doc: &OrgDoc,
    path: &[usize],
    tags: &[&str],
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);

    // Determine where the trailing tag block sits in `body`.
    let trim_end = target.tag_spans.first().map_or_else(
        || body.trim_end_matches([' ', '\t']).len(),
        |first_tag| {
            let start_in_body = first_tag.start - target.header_span.start;
            // The opening `:` lives at start_in_body - 1; trim
            // whitespace before it to drop the separator.
            body[..start_in_body - 1]
                .trim_end_matches([' ', '\t'])
                .len()
        },
    );

    let title_part = &body[..trim_end];

    let new_header = if tags.is_empty() {
        format!("{title_part}\n")
    } else {
        let block: String = tags.iter().fold(String::from(":"), |mut s, t| {
            s.push_str(t);
            s.push(':');
            s
        });
        format!("{title_part} {block}\n")
    };

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Ensure the headline at `path` has an `:ID:` property.
///
/// If the drawer is absent, a fresh one is inserted immediately after
/// the headline's header line. If the drawer exists without an `:ID:`
/// entry, one is inserted at the top of the drawer. If the drawer
/// already has a different `:ID:`, this is a no-op — existing ids are
/// never replaced (I2).
pub fn rewrite_headline_ensure_id(
    doc: &OrgDoc,
    path: &[usize],
    id: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    if let Some(p) = &target.properties {
        if p.entries
            .iter()
            .any(|e| &src[e.key_span.start..e.key_span.end] == "ID")
        {
            return Ok(doc.clone());
        }
        let after_open = find_line_end(&src, p.drawer_span.start).ok_or(RewriteError::Parse)?;
        let insert = format!(":ID: {id}\n");
        src.insert_str(after_open, &insert);
    } else {
        let after_header = target.header_span.end;
        let insert = format!(":PROPERTIES:\n:ID: {id}\n:END:\n");
        src.insert_str(after_header, &insert);
    }
    parse(&src).map_err(|_| RewriteError::Parse)
}

fn find_line_end(src: &str, from: usize) -> Option<usize> {
    let rest = src.get(from..)?;
    let nl = rest.find('\n')?;
    Some(from + nl + 1)
}

fn navigate_headline<'a>(doc: &'a OrgDoc, path: &[usize]) -> Option<&'a Headline> {
    let first = *path.first()?;
    let mut cur = doc.roots.get(first)?;
    for &i in &path[1..] {
        cur = cur.children.get(i)?;
    }
    Some(cur)
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
    meta: NodeMeta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeMeta {
    None,
    CodeBlock {
        lang_span: Option<Span>,
        args_span: Option<Span>,
        content_span: Span,
    },
    ListItem {
        indent: usize,
        marker: ListMarker,
        checkbox: Option<Checkbox>,
        content_span: Span,
    },
}

/// List item marker kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMarker {
    /// `-` bullet.
    Dash,
    /// `+` bullet.
    Plus,
    /// `N.` ordered marker.
    OrderedDot,
    /// `N)` ordered marker.
    OrderedParen,
}

/// Checkbox state on a list item (`[ ]`, `[X]`, `[-]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkbox {
    /// Unchecked `[ ]`.
    Unchecked,
    /// Checked `[X]` or `[x]`.
    Checked,
    /// Partial `[-]`.
    Partial,
}

/// Structural view of a [`NodeKind::ListItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemView<'a> {
    /// Leading indent width in columns (space/tab counted as 1 each).
    pub indent: usize,
    /// Bullet / ordered marker.
    pub marker: ListMarker,
    /// Checkbox if one is present after the marker.
    pub checkbox: Option<Checkbox>,
    /// Item content following the marker (and checkbox), with trailing
    /// newline stripped.
    pub content: &'a str,
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
    /// A `#+BEGIN_SRC` / `#+END_SRC` fenced block.
    CodeBlock,
    /// A single list item line (`-`, `+`, `1.`, `1)`, optionally with
    /// leading indent and `[ ]` / `[X]` / `[-]` checkbox).
    ListItem,
    /// A `| a | b |` table row (including the `|---|` separator rows).
    TableRow,
}

/// Structural view of a [`Node`] classified as [`NodeKind::CodeBlock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeBlockView<'a> {
    /// Language identifier if one was specified on the begin line.
    pub language: Option<&'a str>,
    /// Raw header arguments after the language (e.g. `:results output`).
    pub args: Option<&'a str>,
    /// Verbatim content between begin and end lines, including trailing
    /// newline on each line.
    pub content: &'a str,
}

/// Borrowed view of an inline link `[[target][description]]` or `[[target]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkView<'a> {
    /// Link target (URL, id, file path, etc.) before the optional `][`.
    pub target: &'a str,
    /// Optional human-readable description.
    pub description: Option<&'a str>,
}

/// Borrowed view of a timestamp `<YYYY-MM-DD ...>` (active) or
/// `[YYYY-MM-DD ...]` (inactive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampView<'a> {
    /// Whether this timestamp is active (angle brackets).
    pub active: bool,
    /// The content between the delimiters (e.g. `2026-05-01 Fri 14:30`).
    pub content: &'a str,
}

/// Scan `text` for `<YYYY-MM-DD ...>` (active) and `[YYYY-MM-DD ...]`
/// (inactive) timestamps. Returns them in source order. Malformed
/// brackets without a leading date are skipped.
#[must_use]
pub fn find_timestamps(text: &str) -> Vec<TimestampView<'_>> {
    let mut out: Vec<TimestampView<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'<' || b == b'[' {
            let close = if b == b'<' { b'>' } else { b']' };
            if let Some(end_rel) = text[i + 1..].find(close as char) {
                let inner = &text[i + 1..i + 1 + end_rel];
                if is_timestamp_content(inner) {
                    out.push(TimestampView {
                        active: b == b'<',
                        content: inner,
                    });
                    i = i + 1 + end_rel + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn is_timestamp_content(s: &str) -> bool {
    // Require leading `YYYY-MM-DD`.
    if s.len() < 10 {
        return false;
    }
    let b = s.as_bytes();
    b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

/// Inline markup kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    /// `*bold*`
    Bold,
    /// `/italic/`
    Italic,
    /// `=code=`
    Code,
    /// `~verbatim~`
    Verbatim,
    /// `+strike+`
    Strikethrough,
    /// `_under_`
    Underline,
}

/// Borrowed view of an inline markup run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkupView<'a> {
    /// Kind of markup.
    pub kind: MarkupKind,
    /// Content between the markers (markers excluded).
    pub content: &'a str,
}

/// Scan `text` for inline markup runs.
///
/// A run is `MARKER NON-WS ... NON-WS MARKER` where MARKER is one of
/// `*/=~+_` and the neighbouring characters (outside the markers) must
/// be non-alphanumeric, whitespace, or string boundaries so the marker
/// isn't inside a word.
#[must_use]
pub fn find_markup(text: &str) -> Vec<MarkupView<'_>> {
    const MARKERS: &[(u8, MarkupKind)] = &[
        (b'*', MarkupKind::Bold),
        (b'/', MarkupKind::Italic),
        (b'=', MarkupKind::Code),
        (b'~', MarkupKind::Verbatim),
        (b'+', MarkupKind::Strikethrough),
        (b'_', MarkupKind::Underline),
    ];
    let bytes = text.as_bytes();
    let mut out: Vec<MarkupView<'_>> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let Some(&(_, kind)) = MARKERS.iter().find(|(m, _)| *m == b) else {
            i += 1;
            continue;
        };
        // Left boundary: start of string, or preceding char is non-word.
        let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        if !left_ok {
            i += 1;
            continue;
        }
        // Require at least one non-ws char after marker.
        if i + 1 >= bytes.len() {
            break;
        }
        let after = bytes[i + 1];
        if after == b' ' || after == b'\t' || after == b'\n' || after == b {
            i += 1;
            continue;
        }
        // Find closing marker on same line.
        let mut j = i + 1;
        let mut found: Option<usize> = None;
        while j < bytes.len() {
            let c = bytes[j];
            if c == b'\n' {
                break;
            }
            if c == b && bytes[j - 1] != b' ' && bytes[j - 1] != b'\t' {
                // Right boundary: next char is non-word (or end).
                let right_ok = j + 1 >= bytes.len() || !is_word_byte(bytes[j + 1]);
                if right_ok {
                    found = Some(j);
                    break;
                }
            }
            j += 1;
        }
        if let Some(end) = found {
            out.push(MarkupView {
                kind,
                content: &text[i + 1..end],
            });
            i = end + 1;
            continue;
        }
        i += 1;
    }
    out
}

const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan `text` for inline `[[target][desc]]` / `[[target]]` links.
/// Returns them in source order. Unterminated `[[` is skipped without
/// panicking (I5).
#[must_use]
pub fn find_links(text: &str) -> Vec<LinkView<'_>> {
    let mut out: Vec<LinkView<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let after = &text[i + 2..];
            if let Some(end_rel) = after.find("]]") {
                let inner = &after[..end_rel];
                let (target, description) = inner.find("][").map_or((inner, None), |mid| {
                    (&inner[..mid], Some(&inner[mid + 2..]))
                });
                out.push(LinkView {
                    target,
                    description,
                });
                i = i + 2 + end_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
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

    /// Structural view when this node is a [`NodeKind::CodeBlock`].
    #[must_use]
    pub fn as_code_block(&self) -> Option<CodeBlockView<'_>> {
        if let NodeMeta::CodeBlock {
            lang_span,
            args_span,
            content_span,
        } = &self.meta
        {
            Some(CodeBlockView {
                language: lang_span.map(|s| &self.source[s.start..s.end]),
                args: args_span.map(|s| &self.source[s.start..s.end]),
                content: &self.source[content_span.start..content_span.end],
            })
        } else {
            None
        }
    }

    /// Structural view when this node is a [`NodeKind::ListItem`].
    #[must_use]
    pub fn as_list_item(&self) -> Option<ListItemView<'_>> {
        if let NodeMeta::ListItem {
            indent,
            marker,
            checkbox,
            content_span,
        } = self.meta
        {
            Some(ListItemView {
                indent,
                marker,
                checkbox,
                content: &self.source[content_span.start..content_span.end],
            })
        } else {
            None
        }
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

        // Code block: `#+BEGIN_SRC [lang [args...]]` ... `#+END_SRC`. Case
        // insensitive on the directive. Content between is verbatim and
        // never classified as heading/drawer/etc. Unclosed blocks fall
        // through to the normal line classifier.
        if let Some((node, next_i)) = scan_code_block(&lines, i, &source) {
            flush_paragraph(
                &source,
                &mut paragraph,
                target_nodes(&mut roots, &mut preamble),
            );
            push_node(&mut roots, &mut preamble, node);
            i = next_i;
            continue;
        }

        if let Some((kind, meta)) = classify_special_line(line, span) {
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
                    kind,
                    span,
                    meta,
                },
            );
            i += 1;
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
                        meta: NodeMeta::None,
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
                        meta: NodeMeta::None,
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
                        meta: NodeMeta::None,
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

fn classify_special_line(line: &str, span: Span) -> Option<(NodeKind, NodeMeta)> {
    if let Some(meta) = classify_list_item(line, span) {
        return Some((NodeKind::ListItem, meta));
    }
    if classify_table_row(line) {
        return Some((NodeKind::TableRow, NodeMeta::None));
    }
    None
}

fn classify_list_item(line: &str, span: Span) -> Option<NodeMeta> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    let rest = &body[indent..];
    if rest.is_empty() {
        return None;
    }
    let bytes = rest.as_bytes();

    // Determine marker and its consumed length.
    let (marker, marker_len) =
        if bytes[0] == b'-' && bytes.get(1).is_some_and(|b| *b == b' ' || *b == b'\t') {
            (ListMarker::Dash, 1)
        } else if bytes[0] == b'+' && bytes.get(1).is_some_and(|b| *b == b' ' || *b == b'\t') {
            (ListMarker::Plus, 1)
        } else if bytes[0].is_ascii_digit() {
            // Ordered: digits followed by '.' or ')'.
            let mut j = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let delim = bytes[j];
            if delim != b'.' && delim != b')' {
                return None;
            }
            let after_delim = bytes.get(j + 1);
            if !matches!(after_delim, Some(b' ' | b'\t')) {
                return None;
            }
            let m = if delim == b'.' {
                ListMarker::OrderedDot
            } else {
                ListMarker::OrderedParen
            };
            (m, j + 1)
        } else {
            return None;
        };

    let after_marker = &rest[marker_len..];
    let ws = after_marker.len() - after_marker.trim_start_matches([' ', '\t']).len();
    let after_ws = &after_marker[ws..];

    // Optional checkbox.
    let (checkbox, after_cb_len) = if after_ws.len() >= 3
        && &after_ws[..1] == "["
        && &after_ws[2..3] == "]"
    {
        let mark = after_ws.as_bytes()[1];
        let cb = match mark {
            b' ' => Some(Checkbox::Unchecked),
            b'X' | b'x' => Some(Checkbox::Checked),
            b'-' => Some(Checkbox::Partial),
            _ => None,
        };
        match cb {
            Some(_) => {
                let after_box = &after_ws[3..];
                if after_box.is_empty() || after_box.starts_with(' ') || after_box.starts_with('\t')
                {
                    let skip_ws = after_box.len() - after_box.trim_start_matches([' ', '\t']).len();
                    (cb, 3 + skip_ws)
                } else {
                    (None, 0)
                }
            }
            None => (None, 0),
        }
    } else {
        (None, 0)
    };

    let content_rel_start = indent + marker_len + ws + after_cb_len;
    let content_end = indent + rest.len();
    let content_span = Span {
        start: span.start + content_rel_start,
        end: span.start + content_end,
    };

    Some(NodeMeta::ListItem {
        indent,
        marker,
        checkbox,
        content_span,
    })
}

fn classify_table_row(line: &str) -> bool {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let trimmed = body.trim_start_matches([' ', '\t']);
    trimmed.starts_with('|')
}

fn scan_code_block(
    lines: &[(&str, Span)],
    start: usize,
    source: &Arc<str>,
) -> Option<(Node, usize)> {
    let (line, span) = lines[start];
    let body = line.strip_suffix('\n').unwrap_or(line);
    // Directive must start at column 0.
    let rest = body.strip_prefix("#+")?;
    // Match `begin_src` case-insensitive followed by space/EOL/colon.
    let directive_len = 9; // "begin_src" or "BEGIN_SRC"
    if rest.len() < directive_len {
        return None;
    }
    if !rest[..directive_len].eq_ignore_ascii_case("begin_src") {
        return None;
    }
    let after = &rest[directive_len..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }

    // Parse `lang` and `args` from header.
    let header_rest = after.trim_start_matches([' ', '\t']);
    let header_offset = (body.len() - header_rest.len()) + span.start;
    let (lang_span, args_span) = if header_rest.is_empty() {
        (None, None)
    } else {
        let lang_end = header_rest.find([' ', '\t']).unwrap_or(header_rest.len());
        let lang = &header_rest[..lang_end];
        let lang_span = if lang.is_empty() {
            None
        } else {
            Some(Span {
                start: header_offset,
                end: header_offset + lang.len(),
            })
        };
        let after_lang = &header_rest[lang_end..];
        let args_trim = after_lang.trim_start_matches([' ', '\t']);
        let args_offset = header_offset + (header_rest.len() - args_trim.len());
        let args_trimmed = args_trim.trim_end_matches([' ', '\t']);
        let args_span = if args_trimmed.is_empty() {
            None
        } else {
            Some(Span {
                start: args_offset,
                end: args_offset + args_trimmed.len(),
            })
        };
        (lang_span, args_span)
    };

    // Scan forward for `#+END_SRC` at column 0.
    let content_start = span.end;
    let mut j = start + 1;
    while j < lines.len() {
        let (ln, _) = lines[j];
        let ln_body = ln.strip_suffix('\n').unwrap_or(ln);
        if let Some(rest) = ln_body.strip_prefix("#+")
            && rest.eq_ignore_ascii_case("end_src")
        {
            let (_, end_span) = lines[j];
            let node_span = Span {
                start: span.start,
                end: end_span.end,
            };
            let content_span = Span {
                start: content_start,
                end: end_span.start,
            };
            return Some((
                Node {
                    source: Arc::clone(source),
                    kind: NodeKind::CodeBlock,
                    span: node_span,
                    meta: NodeMeta::CodeBlock {
                        lang_span,
                        args_span,
                        content_span,
                    },
                },
                j + 1,
            ));
        }
        j += 1;
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
            meta: NodeMeta::None,
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
