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
    let mut cursor = 0usize;
    for line in src.split_inclusive('\n') {
        let start = cursor;
        let end = cursor + line.len();
        cursor = end;
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.trim().is_empty() {
            if let Some((ps, pe)) = paragraph.take() {
                blocks.push(Block {
                    source: Arc::clone(&source),
                    kind: BlockKind::Paragraph,
                    start: ps,
                    end: pe,
                    heading_level: None,
                });
            }
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
            if let Some((ps, pe)) = paragraph.take() {
                blocks.push(Block {
                    source: Arc::clone(&source),
                    kind: BlockKind::Paragraph,
                    start: ps,
                    end: pe,
                    heading_level: None,
                });
            }
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
    Ok(MdDoc { source, blocks })
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
