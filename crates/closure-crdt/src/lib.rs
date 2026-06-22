//! CRDT wrapper over closure-core [`Document`]s. Merges by stable
//! [`BlockId`] — no command bypasses the registry, and no merge
//! regenerates ids (I2 + I3).
//!
//! Per-block field registers (title, body) merge last-writer-wins
//! independently, so concurrent edits to different fields of the same
//! block both survive. Reconciliation back into a [`Document`] runs
//! through kernel commands and is therefore undoable.

#![forbid(unsafe_code)]

mod body;
pub use body::{BodyCrdt, ElemId};

use std::collections::HashMap;

use closure_core::{BlockId, Command, Document, RenameHeadline, SetBody};
use thiserror::Error;

/// Simple vector clock for P2P causality (replaces manual u64 ts per ROADMAP).
///
/// Each replica has an entry; local events increment own counter; merge takes max per entry.
/// The 'time' for LWW can be a summary (e.g. max or sum); full vector for causality tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VectorClock {
    counters: HashMap<String, u64>,
}

impl VectorClock {
    #[must_use]
    #[allow(missing_docs)]
    pub fn new(replica: &str) -> Self {
        let mut c = HashMap::new();
        c.insert(replica.to_owned(), 0);
        Self { counters: c }
    }

    /// Increment local counter (on local event/snapshot).
    pub fn bump(&mut self, replica: &str) {
        let e = self.counters.entry(replica.to_owned()).or_insert(0);
        *e += 1;
    }

    /// Merge: per replica, take the max.
    pub fn merge(&mut self, other: &Self) {
        for (r, &c) in &other.counters {
            let e = self.counters.entry(r.clone()).or_insert(0);
            if c > *e {
                *e = c;
            }
        }
    }

    /// Logical time for LWW comparison (e.g. max counter across; or sum for total order approximation).
    #[must_use]
    pub fn logical_time(&self) -> u64 {
        self.counters.values().copied().max().unwrap_or(0)
    }

    /// For causality property tests: the counter for a replica.
    #[must_use]
    pub fn get(&self, replica: &str) -> u64 {
        self.counters.get(replica).copied().unwrap_or(0)
    }
}

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

/// Per-block state: a LWW register for the title and a character-level
/// RGA ([`BodyCrdt`]) for the body, so concurrent edits to the same
/// body converge char-level instead of one side winning (C2b).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockState {
    title: Register,
    body: BodyCrdt,
}

/// Seed a per-replica element-id counter from a logical time, leaving
/// headroom for up to ~1M characters authored at that time. Monotone in
/// `ts`, so a later snapshot's inserts always outrank earlier ids.
const fn body_counter_seed(ts: u64) -> u64 {
    ts.saturating_mul(1_000_000)
}

/// CRDT-side view of a document: per-block field registers that can
/// be merged with another replica and reconciled back into a
/// [`Document`] through kernel commands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replica {
    blocks: HashMap<BlockId, BlockState>,
}

impl Replica {
    /// Snapshot the title (LWW) and body (fresh RGA) of every headline
    /// at logical time `ts`, authored by `replica`. Body element ids are
    /// minted for `replica` from a `ts`-seeded counter, so the same
    /// `(doc, ts, replica)` always yields identical ids (I6).
    #[must_use]
    pub fn snapshot(doc: &Document, ts: u64, replica: &str) -> Self {
        let mut counter = body_counter_seed(ts);
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
                        body: BodyCrdt::from_text(h.body_text(), replica, &mut counter),
                    },
                )
            })
            .collect();
        Self { blocks }
    }

    /// Snapshot `doc` relative to a common `base`, authored by `replica`.
    /// The title register's timestamp only advances when the title
    /// actually changed (so an untouched title never outranks a
    /// concurrent edit). The body is the base block's RGA *edited toward*
    /// the new text: shared characters keep their element ids, so a
    /// peer's concurrent edits to other positions survive the merge.
    #[must_use]
    pub fn snapshot_against(base: &Self, doc: &Document, ts: u64, replica: &str) -> Self {
        let blocks = doc
            .all_headlines()
            .map(|h| {
                let id = h.id().clone();
                let base_block = base.blocks.get(&id);
                let title_value = h.title().to_owned();
                let title_ts = match base_block {
                    Some(b) if b.title.value == title_value => b.title.ts,
                    _ => ts,
                };
                let mut body = base_block.map_or_else(BodyCrdt::new, |b| b.body.clone());
                // Seed new element ids above every existing one so this
                // edit's inserts sort as "newer" than the base (and any
                // already-merged peer) regardless of clock skew — the
                // key to correct char-level placement across replicas.
                let mut counter = body_counter_seed(ts).max(body.max_counter() + 1);
                body.edit_to(h.body_text(), replica, &mut counter);
                (
                    id,
                    BlockState {
                        title: Register {
                            ts: title_ts,
                            value: title_value,
                        },
                        body,
                    },
                )
            })
            .collect();
        Self { blocks }
    }

    /// Snapshot using a `VectorClock` (for P2P causality per ROADMAP).
    /// Bumps the replica's counter, uses the logical time for the
    /// register ts and the body element-id seed.
    #[must_use]
    pub fn snapshot_with_clock(doc: &Document, clock: &mut VectorClock, replica: &str) -> Self {
        clock.bump(replica);
        let ts = clock.logical_time();
        Self::snapshot(doc, ts, replica)
    }

    /// Merge another replica in: per block and per field, the
    /// register with the higher timestamp wins; ties keep the current
    /// entry. No merge ever creates a fresh id (I2).
    pub fn merge(&mut self, other: &Self) {
        for (id, state) in &other.blocks {
            if let Some(mine) = self.blocks.get_mut(id) {
                mine.title.merge(&state.title);
                mine.body.merge(&state.body); // RGA union (C2b)
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

    /// The currently-converged body text for a block (materialised from
    /// the RGA). Returns an owned `String` since the text is computed.
    #[must_use]
    pub fn body_of(&self, id: &BlockId) -> Option<String> {
        self.blocks.get(id).map(|b| b.body.materialize())
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
                cmd.apply(doc)
                    .map_err(|e| CrdtError::Apply(e.to_string()))?;
                changed += 1;
            }
            let merged_body = state.body.materialize();
            if merged_body != body {
                let cmd = SetBody::new(id.clone(), merged_body);
                cmd.apply(doc)
                    .map_err(|e| CrdtError::Apply(e.to_string()))?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Encode this replica to a self-describing little-endian byte
    /// buffer so a transport can ship it: a `u32` block count, then per
    /// block the id, title `(ts, value)` and body `(ts, value)`, each
    /// scalar length-prefixed. Pairs with [`Self::decode`].
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        fn put_str(out: &mut Vec<u8>, s: &str) {
            out.extend_from_slice(&u32::try_from(s.len()).unwrap_or(u32::MAX).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        let mut out = Vec::new();
        out.extend_from_slice(
            &u32::try_from(self.blocks.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for (id, st) in &self.blocks {
            put_str(&mut out, id.as_str());
            out.extend_from_slice(&st.title.ts.to_le_bytes());
            put_str(&mut out, &st.title.value);
            st.body.encode(&mut out);
        }
        out
    }

    /// Decode a buffer produced by [`Self::encode`].
    ///
    /// # Errors
    ///
    /// [`CrdtError::Decode`] on a truncated or malformed buffer (never
    /// panics).
    pub fn decode(bytes: &[u8]) -> Result<Self, CrdtError> {
        let mut cur = Cursor { buf: bytes, pos: 0 };
        let n = cur.u32()?;
        let mut blocks = HashMap::new();
        for _ in 0..n {
            let id = cur.string()?;
            let title = Register {
                ts: cur.u64()?,
                value: cur.string()?,
            };
            let body = BodyCrdt::decode(&mut cur)?;
            blocks.insert(BlockId::from_existing(&id), BlockState { title, body });
        }
        Ok(Self { blocks })
    }
}

/// Bounds-checked little-endian reader for [`Replica::decode`] (shared
/// with the body-RGA decoder in the `body` module).
pub(crate) struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&[u8], CrdtError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| CrdtError::Decode("overflow".into()))?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| CrdtError::Decode("unexpected end of buffer".into()))?;
        self.pos = end;
        Ok(slice)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, CrdtError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u32(&mut self) -> Result<u32, CrdtError> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| CrdtError::Decode("u32".into()))?;
        Ok(u32::from_le_bytes(b))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, CrdtError> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| CrdtError::Decode("u64".into()))?;
        Ok(u64::from_le_bytes(b))
    }
    pub(crate) fn string(&mut self) -> Result<String, CrdtError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(ToOwned::to_owned)
            .map_err(|_| CrdtError::Decode("invalid utf-8".into()))
    }
}

/// Which field of a block a [`FieldConflict`] is about (V9a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictField {
    /// The headline title (LWW register).
    Title,
    /// The headline body (materialised RGA text).
    Body,
}

/// A 3-way divergence on one field of one block (V9a): both `ours` and
/// `theirs` changed it, differently, relative to `base`. LWW would
/// silently pick one; this surfaces all three for a resolution choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldConflict {
    /// The conflicting block.
    pub block: BlockId,
    /// Which field.
    pub field: ConflictField,
    /// The common-ancestor value (`None` if the block is new on a side).
    pub base: Option<String>,
    /// Our value.
    pub ours: String,
    /// Their value.
    pub theirs: String,
}

/// Detect 3-way field conflicts between two replicas derived from a
/// common `base` (V9a).
///
/// A field conflicts when `ours` and `theirs` hold different values and
/// both differ from `base` (an add/add with differing values also
/// conflicts). One-sided changes and identical edits do not. Output is
/// sorted by `(block, field)` for determinism (I6).
#[must_use]
pub fn conflicts(base: &Replica, ours: &Replica, theirs: &Replica) -> Vec<FieldConflict> {
    let mut out = Vec::new();
    // Union of block ids present in ours or theirs, ordered + deduped by
    // their string form (BlockId is not Ord) for determinism (I6).
    let ids: std::collections::BTreeMap<String, &BlockId> = ours
        .block_ids()
        .chain(theirs.block_ids())
        .map(|id| (id.to_string(), id))
        .collect();

    for id in ids.into_values() {
        let detect = |field, b: Option<String>, o: Option<String>, t: Option<String>| {
            let (o, t) = (o?, t?);
            (o != t && Some(&o) != b.as_ref() && Some(&t) != b.as_ref()).then(|| FieldConflict {
                block: id.clone(),
                field,
                base: b,
                ours: o,
                theirs: t,
            })
        };
        if let Some(c) = detect(
            ConflictField::Title,
            base.title_of(id).map(ToOwned::to_owned),
            ours.title_of(id).map(ToOwned::to_owned),
            theirs.title_of(id).map(ToOwned::to_owned),
        ) {
            out.push(c);
        }
        if let Some(c) = detect(
            ConflictField::Body,
            base.body_of(id),
            ours.body_of(id),
            theirs.body_of(id),
        ) {
            out.push(c);
        }
    }
    out
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
    /// A wire buffer could not be decoded into a [`Replica`].
    #[error("decode: {0}")]
    Decode(String),
}
