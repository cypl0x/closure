//! CRDT wrapper over closure-core [`Document`]s. Merges by stable
//! [`BlockId`] — no command bypasses the registry, and no merge
//! regenerates ids (I2 + I3).
//!
//! Current M6 skeleton: a last-writer-wins merge keyed by block id.
//! Automerge / Yrs integration replaces the inner merge function in a
//! later milestone.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use closure_core::{BlockId, Document};
use thiserror::Error;

/// CRDT-side view of a document. Stores a per-block title register
/// that can be merged with another replica.
#[derive(Debug, Clone, Default)]
pub struct Replica {
    titles: HashMap<BlockId, (u64, String)>,
}

impl Replica {
    /// Snapshot the titles of every headline at logical time `ts`.
    #[must_use]
    pub fn snapshot(doc: &Document, ts: u64) -> Self {
        let titles = doc
            .all_headlines()
            .map(|h| (h.id().clone(), (ts, h.title().to_owned())))
            .collect();
        Self { titles }
    }

    /// Merge another replica in: per-id, the entry with the higher
    /// timestamp wins. Ties keep the current entry.
    pub fn merge(&mut self, other: &Self) {
        for (id, (ts, title)) in &other.titles {
            let entry = self.titles.entry(id.clone()).or_insert((0, String::new()));
            if *ts > entry.0 {
                *entry = (*ts, title.clone());
            }
        }
    }

    /// Read the currently-winning title for a block.
    #[must_use]
    pub fn title_of(&self, id: &BlockId) -> Option<&str> {
        self.titles.get(id).map(|(_, t)| t.as_str())
    }
}

/// Merge errors (placeholder).
#[derive(Debug, Error)]
pub enum CrdtError {
    /// Schema mismatch between replicas.
    #[error("schema mismatch")]
    Schema,
}
