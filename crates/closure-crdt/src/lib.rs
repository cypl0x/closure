//! CRDT wrapper over closure-core [`Document`]s. Merges by stable
//! [`BlockId`] — no command bypasses the registry, and no merge
//! regenerates ids (I2 + I3).
//!
//! Per-block field registers (title, body) merge last-writer-wins
//! independently, so concurrent edits to different fields of the same
//! block both survive. Reconciliation back into a [`Document`] runs
//! through kernel commands and is therefore undoable.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use closure_core::{BlockId, Command, Document, RenameHeadline, SetBody};
use thiserror::Error;

/// A last-writer-wins register.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    ts: u64,
    value: String,
}

impl Register {
    fn merge(&mut self, other: &Self) {
        if other.ts > self.ts {
            *self = other.clone();
        }
    }
}

/// Per-block state: independent LWW registers for title and body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockState {
    title: Register,
    body: Register,
}

/// CRDT-side view of a document: per-block field registers that can
/// be merged with another replica and reconciled back into a
/// [`Document`] through kernel commands.
#[derive(Debug, Clone, Default)]
pub struct Replica {
    blocks: HashMap<BlockId, BlockState>,
}

impl Replica {
    /// Snapshot the title and body of every headline at logical time
    /// `ts`.
    #[must_use]
    pub fn snapshot(doc: &Document, ts: u64) -> Self {
        let blocks = doc
            .all_headlines()
            .map(|h| {
                (
                    h.id().clone(),
                    BlockState {
                        title: Register {
                            ts,
                            value: h.title().to_owned(),
                        },
                        body: Register {
                            ts,
                            value: h.body_text().to_owned(),
                        },
                    },
                )
            })
            .collect();
        Self { blocks }
    }

    /// Snapshot `doc` relative to a common `base`: a field's
    /// timestamp only advances to `ts` when its value actually
    /// changed against the base register, so an untouched field never
    /// outranks a concurrent edit on the other replica.
    #[must_use]
    pub fn snapshot_against(base: &Self, doc: &Document, ts: u64) -> Self {
        let mut snap = Self::snapshot(doc, ts);
        for (id, state) in &mut snap.blocks {
            if let Some(b) = base.blocks.get(id) {
                if state.title.value == b.title.value {
                    state.title.ts = b.title.ts;
                }
                if state.body.value == b.body.value {
                    state.body.ts = b.body.ts;
                }
            }
        }
        snap
    }

    /// Merge another replica in: per block and per field, the
    /// register with the higher timestamp wins; ties keep the current
    /// entry. No merge ever creates a fresh id (I2).
    pub fn merge(&mut self, other: &Self) {
        for (id, state) in &other.blocks {
            if let Some(mine) = self.blocks.get_mut(id) {
                mine.title.merge(&state.title);
                mine.body.merge(&state.body);
            } else {
                self.blocks.insert(id.clone(), state.clone());
            }
        }
    }

    /// The block ids known to this replica.
    pub fn block_ids(&self) -> impl Iterator<Item = &BlockId> {
        self.blocks.keys()
    }

    /// The currently-winning title for a block.
    #[must_use]
    pub fn title_of(&self, id: &BlockId) -> Option<&str> {
        self.blocks.get(id).map(|b| b.title.value.as_str())
    }

    /// The currently-winning body for a block.
    #[must_use]
    pub fn body_of(&self, id: &BlockId) -> Option<&str> {
        self.blocks.get(id).map(|b| b.body.value.as_str())
    }

    /// Reconcile `doc` to this replica's winning registers through
    /// kernel commands ([`RenameHeadline`] / [`SetBody`]) — undoable
    /// (I3), ids untouched (I2). Blocks unknown to `doc` are skipped.
    /// Returns the number of edits applied.
    ///
    /// # Errors
    ///
    /// [`CrdtError::Apply`] when a kernel command refuses an edit.
    pub fn apply_to(&self, doc: &mut Document) -> Result<usize, CrdtError> {
        let targets: Vec<(BlockId, String, String)> = doc
            .all_headlines()
            .map(|h| {
                (
                    h.id().clone(),
                    h.title().to_owned(),
                    h.body_text().to_owned(),
                )
            })
            .collect();
        let mut changed = 0;
        for (id, title, body) in targets {
            let Some(state) = self.blocks.get(&id) else {
                continue;
            };
            if state.title.value != title {
                let cmd = RenameHeadline::new(id.clone(), state.title.value.clone());
                cmd.apply(doc).map_err(|e| CrdtError::Apply(e.to_string()))?;
                changed += 1;
            }
            if state.body.value != body {
                let cmd = SetBody::new(id.clone(), state.body.value.clone());
                cmd.apply(doc).map_err(|e| CrdtError::Apply(e.to_string()))?;
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// Merge errors.
#[derive(Debug, Error)]
pub enum CrdtError {
    /// Schema mismatch between replicas.
    #[error("schema mismatch")]
    Schema,
    /// A kernel command refused a reconciliation edit.
    #[error("apply: {0}")]
    Apply(String),
}
