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
}

/// Parse failure.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Parse markdown into an [`MdDoc`]. The parser is total; every byte
/// of the input ends up inside exactly one block's span (I1 by
/// construction).
#[allow(clippy::must_use_candidate, clippy::missing_errors_doc)]
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

/// Whether `body` is a markdown list item (`-`, `*`, `+`, or `N.`).
fn is_list_item(body: &str) -> bool {
    let t = body.trim_start();
    if let Some(rest) = t.strip_prefix(['-', '*', '+']) {
        return rest.starts_with(' ');
    }
    // Ordered: digits then `. ` or `) `.
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && t[digits..].starts_with('.')
        && t[digits + 1..].starts_with(' ')
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
        if trimmed.starts_with("SCHEDULED:") || trimmed.starts_with("DEADLINE:")
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
