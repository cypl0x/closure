//! `CommonMark` + GFM parser (Layer 1).
//!
//! Source-preserving parser for markdown, mirroring the closure-org
//! architecture. Byte-exact roundtrip (I1) holds on arbitrary input by
//! storing the source once and referencing each node by span.
//!
//! Current scope: ATX headings (`# Title` / `## Sub`) and paragraphs.
//! Lists, tables, fenced code, links, and inline markup land in
//! successive cycles.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::sync::Arc;

use thiserror::Error;

/// Parsed markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdDoc {
    source: Arc<str>,
    blocks: Vec<Block>,
}

impl MdDoc {
    /// Full verbatim source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Top-level blocks in document order.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// A markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    source: Arc<str>,
    kind: BlockKind,
    start: usize,
    end: usize,
    heading_level: Option<u8>,
}

impl Block {
    /// Classification of this block.
    #[must_use]
    pub const fn kind(&self) -> BlockKind {
        self.kind
    }

    /// Verbatim source slice.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source[self.start..self.end]
    }

    /// ATX heading level (1 for `#`, 2 for `##`, …). `None` when not
    /// a heading.
    #[must_use]
    pub const fn heading_level(&self) -> Option<u8> {
        self.heading_level
    }
}

/// Coarse block classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// A `#`-prefixed ATX heading line.
    Heading,
    /// A run of non-heading, non-blank lines.
    Paragraph,
    /// A whitespace-only line.
    BlankLine,
    /// A list item line (`-`, `*`, `+`, or `N.` marker).
    ListItem,
    /// A fenced code block (```` ``` ```` … ```` ``` ````), inclusive
    /// of both fence lines.
    CodeFence,
    /// A blockquote line (`>` prefix).
    Blockquote,
    /// A GFM table line (pipe-delimited, e.g. `| a | b |` or the
    /// `|---|---|` delimiter row).
    Table,
    /// A thematic break: a line of three or more `-`, `*`, or `_`
    /// (optionally space-separated), e.g. `---`, `***`, `- - -`.
    ThematicBreak,
}

/// Inline markup classification (Q4-M1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    /// Unmarked text.
    Plain,
    /// `*em*` / `_em_`.
    Emphasis,
    /// `**strong**` / `__strong__`.
    Strong,
    /// `` `code` `` span.
    Code,
    /// `[text](target)` inline link.
    Link,
}

/// Classify inline markup inside one block's text (Q4-M1).
///
/// Flat, single-pass, span-preserving: each span keeps its markers
/// verbatim and the concatenation of the span texts reproduces the
/// input byte-exactly (the `Highlighter` gap-free rule — I1 is never
/// touched because this only *reads*). Unbalanced markers fall back to
/// `Plain`; nesting is not modelled (a later increment).
#[must_use]
pub fn inline_spans(text: &str) -> Vec<(InlineKind, &str)> {
    fn flush_plain<'a>(
        text: &'a str,
        spans: &mut Vec<(InlineKind, &'a str)>,
        from: usize,
        to: usize,
    ) {
        if from < to {
            spans.push((InlineKind::Plain, &text[from..to]));
        }
    }
    /// A delimited span `<mark>…<mark>` at the head of `rest`: its
    /// total length including both markers, `None` when unclosed/empty.
    fn delimited(rest: &str, mark: &str) -> Option<usize> {
        rest.strip_prefix(mark)?
            .find(mark)
            .filter(|&n| n > 0)
            .map(|n| mark.len() + n + mark.len())
    }
    let mut spans: Vec<(InlineKind, &str)> = Vec::new();
    let mut plain_start = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        let matched: Option<(InlineKind, usize)> =
            if rest.starts_with("**") || rest.starts_with("__") {
                delimited(rest, &rest[..2]).map(|n| (InlineKind::Strong, n))
            } else if rest.starts_with('*') || rest.starts_with('_') {
                delimited(rest, &rest[..1]).map(|n| (InlineKind::Emphasis, n))
            } else if rest.starts_with('`') {
                delimited(rest, "`").map(|n| (InlineKind::Code, n))
            } else if rest.starts_with('[') {
                rest.find("](").and_then(|mid| {
                    rest[mid + 2..]
                        .find(')')
                        .map(|close| (InlineKind::Link, mid + 2 + close + 1))
                })
            } else {
                None
            };
        if let Some((kind, len)) = matched {
            flush_plain(text, &mut spans, plain_start, i);
            spans.push((kind, &text[i..i + len]));
            i += len;
            plain_start = i;
        } else {
            // Advance one char (not byte — stay on a boundary).
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    flush_plain(text, &mut spans, plain_start, text.len());
    spans
}

/// Every link target in `md`, document order (Q4-M3).
///
/// Inline `[text](target)` targets and wiki `[[target]]` names, taken
/// from paragraph / heading / list / quote / table blocks — code
/// fences are skipped (a link inside code is text, not a link). This
/// is the md identity substrate: markdown has no `:ID:`, so identity
/// is the path/slug the link names (Decision, Q4).
#[must_use]
pub fn link_targets(md: &str) -> Vec<String> {
    let Ok(doc) = parse(md);
    let mut out = Vec::new();
    for block in doc.blocks() {
        if block.kind() == BlockKind::CodeFence {
            continue;
        }
        let text = block.source();
        let mut i = 0usize;
        while i < text.len() {
            let rest = &text[i..];
            if let Some(inner) = rest.strip_prefix("[[") {
                if let Some(close) = inner.find("]]") {
                    out.push(inner[..close].to_owned());
                    i += 2 + close + 2;
                    continue;
                }
            } else if rest.starts_with('[')
                && let Some(mid) = rest.find("](")
                && let Some(close) = rest[mid + 2..].find(')')
            {
                out.push(rest[mid + 2..mid + 2 + close].to_owned());
                i += mid + 2 + close + 1;
                continue;
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Parse failure.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Parse markdown into an [`MdDoc`]. The parser is total; every byte
/// of the input ends up inside exactly one block's span (I1 by
/// construction).
#[allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::too_many_lines
)]
pub fn parse(src: &str) -> Result<MdDoc, ParseError> {
    let source: Arc<str> = Arc::from(src);
    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph: Option<(usize, usize)> = None;
    // Open code fence: (block-start, marker string) while inside.
    let mut fence: Option<(usize, String)> = None;
    let mut cursor = 0usize;
    let flush_para = |paragraph: &mut Option<(usize, usize)>, blocks: &mut Vec<Block>| {
        if let Some((ps, pe)) = paragraph.take() {
            blocks.push(Block {
                source: Arc::clone(&source),
                kind: BlockKind::Paragraph,
                start: ps,
                end: pe,
                heading_level: None,
            });
        }
    };
    for line in src.split_inclusive('\n') {
        let start = cursor;
        let end = cursor + line.len();
        cursor = end;
        let body = line.strip_suffix('\n').unwrap_or(line);
        // Inside a fence: consume lines until the closing marker.
        if let Some((fstart, marker)) = &fence {
            if body.trim_start().starts_with(marker.as_str()) {
                let fstart = *fstart;
                fence = None;
                blocks.push(Block {
                    source: Arc::clone(&source),
                    kind: BlockKind::CodeFence,
                    start: fstart,
                    end,
                    heading_level: None,
                });
            }
            continue;
        }
        // Opening fence (``` or ~~~).
        if let Some(marker) = fence_marker(body) {
            flush_para(&mut paragraph, &mut blocks);
            fence = Some((start, marker));
            continue;
        }
        // Q4-M2: a setext underline directly under a pending paragraph
        // turns the WHOLE run into one heading (CommonMark rule; `---`
        // only stays a thematic break with no paragraph attached).
        if paragraph.is_some()
            && let Some(level) = setext_level(body)
        {
            if let Some((ps, _)) = paragraph.take() {
                blocks.push(Block {
                    source: Arc::clone(&source),
                    kind: BlockKind::Heading,
                    start: ps,
                    end,
                    heading_level: Some(level),
                });
            }
            continue;
        }
        // GFM / CommonMark line blocks (D1): thematic break first so a
        // `- - -` rule is not mistaken for a list item.
        if let Some(kind) = classify_line_block(body) {
            flush_para(&mut paragraph, &mut blocks);
            blocks.push(Block {
                source: Arc::clone(&source),
                kind,
                start,
                end,
                heading_level: None,
            });
            continue;
        }
        if is_list_item(body) {
            flush_para(&mut paragraph, &mut blocks);
            blocks.push(Block {
                source: Arc::clone(&source),
                kind: BlockKind::ListItem,
                start,
                end,
                heading_level: None,
            });
            continue;
        }
        if body.trim().is_empty() {
            flush_para(&mut paragraph, &mut blocks);
            blocks.push(Block {
                source: Arc::clone(&source),
                kind: BlockKind::BlankLine,
                start,
                end,
                heading_level: None,
            });
            continue;
        }
        if let Some(level) = classify_atx_heading(body) {
            flush_para(&mut paragraph, &mut blocks);
            blocks.push(Block {
                source: Arc::clone(&source),
                kind: BlockKind::Heading,
                start,
                end,
                heading_level: Some(level),
            });
            continue;
        }
        match &mut paragraph {
            Some(p) => p.1 = end,
            None => paragraph = Some((start, end)),
        }
    }
    if let Some((ps, pe)) = paragraph {
        blocks.push(Block {
            source: Arc::clone(&source),
            kind: BlockKind::Paragraph,
            start: ps,
            end: pe,
            heading_level: None,
        });
    }
    // An unterminated fence runs to EOF.
    if let Some((fstart, _)) = fence {
        blocks.push(Block {
            source: Arc::clone(&source),
            kind: BlockKind::CodeFence,
            start: fstart,
            end: cursor,
            heading_level: None,
        });
    }
    Ok(MdDoc { source, blocks })
}

/// Setext underline level: a line of only `=` (level 1) or only `-`
/// (level 2), at least one char, trailing whitespace allowed (Q4-M2).
fn setext_level(body: &str) -> Option<u8> {
    let t = body.trim_end();
    if !t.is_empty() && t.bytes().all(|b| b == b'=') {
        Some(1)
    } else if !t.is_empty() && t.bytes().all(|b| b == b'-') {
        Some(2)
    } else {
        None
    }
}

/// The fence marker (```` ``` ```` or `~~~`) opening `body`, if any.
fn fence_marker(body: &str) -> Option<String> {
    let t = body.trim_start();
    for m in ["```", "~~~"] {
        if t.starts_with(m) {
            return Some(m.to_owned());
        }
    }
    None
}

/// Classify a single line as a GFM/CommonMark line block (thematic break,
/// blockquote, or table), or `None` to fall through to list/paragraph.
/// Per-line so the byte-exact roundtrip (I1) is preserved by construction.
fn classify_line_block(body: &str) -> Option<BlockKind> {
    if is_thematic_break(body) {
        return Some(BlockKind::ThematicBreak);
    }
    let t = body.trim_start();
    if t.starts_with('>') {
        return Some(BlockKind::Blockquote);
    }
    if t.starts_with('|') {
        return Some(BlockKind::Table);
    }
    None
}

/// A thematic break: ignoring spaces, three or more of a single `-`, `*`,
/// or `_` and nothing else.
fn is_thematic_break(body: &str) -> bool {
    let mut marker: Option<char> = None;
    let mut count = 0usize;
    for c in body.chars() {
        match c {
            ' ' | '\t' => {}
            '-' | '*' | '_' if marker.is_none_or(|m| m == c) => {
                marker = Some(c);
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

/// Whether `body` is a markdown list item (`-`, `*`, `+`, or `N.`).
fn is_list_item(body: &str) -> bool {
    let t = body.trim_start();
    if let Some(rest) = t.strip_prefix(['-', '*', '+']) {
        return rest.starts_with(' ');
    }
    // Ordered: digits then `. ` or `) `.
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && t[digits..].starts_with('.') && t[digits + 1..].starts_with(' ')
}

/// Serialise back to markdown. Concatenates block spans verbatim — I1
/// holds for unedited documents by construction.
#[must_use]
pub fn print(doc: &MdDoc) -> String {
    let mut out = String::with_capacity(doc.source.len());
    for b in &doc.blocks {
        out.push_str(&b.source[b.start..b.end]);
    }
    out
}

fn classify_atx_heading(body: &str) -> Option<u8> {
    let hashes = body.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &body[hashes..];
    if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
        Some(u8::try_from(hashes).unwrap_or(u8::MAX))
    } else {
        None
    }
}

/// Convert org source to markdown (line-level subset). Returns the
/// markdown plus warnings for lossy parts (drawers, planning lines,
/// TODO keywords) that are dropped.
#[must_use]
pub fn from_org(org: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(org.len());
    let mut warnings = Vec::new();
    let mut in_drawer = false;
    for line in org.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = body.trim();
        if in_drawer {
            if trimmed.eq_ignore_ascii_case(":END:") {
                in_drawer = false;
            }
            continue;
        }
        if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 2 {
            in_drawer = true;
            warnings.push(format!("dropped drawer `{trimmed}`"));
            continue;
        }
        if trimmed.starts_with("SCHEDULED:")
            || trimmed.starts_with("DEADLINE:")
            || trimmed.starts_with("CLOSED:")
        {
            warnings.push(format!("dropped planning `{trimmed}`"));
            continue;
        }
        if let Some(lang) = trimmed.strip_prefix("#+BEGIN_SRC") {
            let _ = writeln!(out, "```{}", lang.trim());
            continue;
        }
        if trimmed.eq_ignore_ascii_case("#+END_SRC") {
            out.push_str("```\n");
            continue;
        }
        let stars = body.bytes().take_while(|&b| b == b'*').count();
        if stars > 0 && body.as_bytes().get(stars) == Some(&b' ') {
            let _ = writeln!(out, "{} {}", "#".repeat(stars), &body[stars + 1..]);
            continue;
        }
        out.push_str(line);
    }
    (out, warnings)
}

/// Convert markdown source to org (line-level subset).
#[must_use]
pub fn to_org(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_fence = false;
    for line in md.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = body.trim_start();
        if let Some(rest) = fence_marker(body).and_then(|m| trimmed.strip_prefix(m.as_str())) {
            if in_fence {
                out.push_str("#+END_SRC\n");
                in_fence = false;
            } else {
                let _ = writeln!(out, "#+BEGIN_SRC{}", {
                    let lang = rest.trim();
                    if lang.is_empty() {
                        String::new()
                    } else {
                        format!(" {lang}")
                    }
                });
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let hashes = body.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && body.as_bytes().get(hashes) == Some(&b' ') {
            let _ = writeln!(out, "{} {}", "*".repeat(hashes), &body[hashes + 1..]);
            continue;
        }
        out.push_str(line);
    }
    out
}
