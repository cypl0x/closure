//! Kernel document model: stable block IDs, command registry, keybinding
//! trie, event bus. Sits on top of [`closure_org`] and is UI-agnostic.
//!
//! This crate defines the frontend-agnostic API surface (spec invariant
//! I7): shells and adapters consume [`Document`], [`BlockId`], and the
//! future command registry. They never reach into `closure-org`
//! directly, and they never see byte offsets / spans.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;

use closure_org::{Headline, OrgDoc, parse};
use thiserror::Error;
use ulid::Ulid;

/// Stable identifier for a headline block.
///
/// Derived from the `:ID:` property when present, otherwise a fresh ULID
/// allocated at parse time. Fresh ULIDs live in memory only until a
/// command persists them via the future `:ID:` injector.
///
/// Spec invariant I2: `BlockId` survives parse/print/CRDT merges. `Edit`
/// values reference blocks by ID, never by file position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockId(Arc<str>);

impl BlockId {
    /// ULID or custom-id string that identifies this block.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Allocate a fresh ULID-backed id.
    #[must_use]
    pub fn fresh() -> Self {
        Self(Arc::from(Ulid::new().to_string()))
    }

    /// Adopt an existing id string (e.g. from a `:ID:` property).
    #[must_use]
    pub fn from_existing(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A loaded org document with stable block identifiers.
///
/// Wraps an [`OrgDoc`] and adds a `BlockId → headline path` index so
/// the kernel can address headlines by identity rather than position
/// (I2).
#[derive(Debug, Clone)]
pub struct Document {
    org: OrgDoc,
    headlines: Vec<DocHeadline>,
    by_id: HashMap<BlockId, usize>,
}

/// Flat headline record with stable id and path back into the `OrgDoc`
/// tree.
#[derive(Debug, Clone)]
pub struct DocHeadline {
    id: BlockId,
    path: Vec<usize>,
    title: String,
    level: u8,
}

impl DocHeadline {
    /// Block id of this headline.
    #[must_use]
    pub const fn id(&self) -> &BlockId {
        &self.id
    }

    /// Path of indices from `Document::roots()` to this headline.
    #[must_use]
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// Cached title text.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Nesting level.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }
}

/// Errors while loading a document.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The underlying parser failed.
    #[error("parse failure")]
    Parse,
}

impl Document {
    /// Load a document from an in-memory source string.
    pub fn load_str(src: &str) -> Result<Self, LoadError> {
        let org = parse(src).map_err(|_| LoadError::Parse)?;
        let mut headlines: Vec<DocHeadline> = Vec::new();
        for (i, root) in org.roots().iter().enumerate() {
            collect_headlines(root, &[i], &mut headlines);
        }
        let mut by_id: HashMap<BlockId, usize> = HashMap::with_capacity(headlines.len());
        for (idx, h) in headlines.iter().enumerate() {
            by_id.insert(h.id.clone(), idx);
        }
        Ok(Self {
            org,
            headlines,
            by_id,
        })
    }

    /// Byte-exact source text (carries I1 forward from `closure-org`).
    #[must_use]
    pub fn source(&self) -> String {
        closure_org::print(&self.org)
    }

    /// Top-level headlines in the document (flat slice of the tree).
    #[must_use]
    pub fn roots(&self) -> Vec<&DocHeadline> {
        self.headlines
            .iter()
            .filter(|h| h.path.len() == 1)
            .collect()
    }

    /// Iterate every headline in depth-first order.
    pub fn all_headlines(&self) -> impl Iterator<Item = &DocHeadline> {
        self.headlines.iter()
    }

    /// All known block ids in the document.
    #[must_use]
    pub fn all_block_ids(&self) -> Vec<BlockId> {
        self.headlines.iter().map(|h| h.id.clone()).collect()
    }

    /// Lookup a headline by its stable id.
    #[must_use]
    pub fn headline_by_id(&self, id: &BlockId) -> Option<&DocHeadline> {
        self.by_id.get(id).and_then(|&i| self.headlines.get(i))
    }

    /// Access the underlying parsed org document. Prefer the
    /// `Document`-level APIs over reaching into this where possible —
    /// shells must not depend on parser internals.
    #[must_use]
    pub const fn org(&self) -> &OrgDoc {
        &self.org
    }
}

fn collect_headlines(h: &Headline, path: &[usize], out: &mut Vec<DocHeadline>) {
    let id = h
        .properties()
        .and_then(|p| p.id())
        .map_or_else(BlockId::fresh, BlockId::from_existing);
    out.push(DocHeadline {
        id,
        path: path.to_vec(),
        title: h.title().to_owned(),
        level: h.level(),
    });
    for (i, c) in h.children().iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(i);
        collect_headlines(c, &child_path, out);
    }
}
