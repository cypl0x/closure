//! Character-level body CRDT (C2b): a Replicated Growable Array (RGA).
//!
//! The block-level LWW body register loses one side when two replicas
//! edit the *same* body concurrently. This RGA instead tracks each
//! character as an element with a globally-unique id and a causal
//! predecessor; merging is the union of elements (tombstones for
//! deletes) and the materialised string is a deterministic linear walk,
//! so concurrent inserts at the same position both survive and every
//! replica converges to the same text regardless of merge order.
//!
//! Hand-rolled rather than pulling Automerge/Yrs: those drag large
//! (and partly async) dependency trees that fight I10's hermetic,
//! dep-minimal build — see the 2026-06-19 char-CRDT Decision. The RGA
//! lives behind the existing `Edit`/`BlockId` surface (no closure-core
//! API change): edits still flow through kernel commands; this only
//! changes how a block's body register merges.

use std::collections::HashMap;

/// Globally-unique, totally-ordered identity of one RGA element.
///
/// `(counter, replica)` — `counter` is a per-replica monotonic clock,
/// `replica` breaks ties between concurrent ids sharing a counter. The
/// derived `Ord` (counter first, then replica) is the deterministic
/// tiebreak that makes the linear order — and thus the merged text —
/// identical on every peer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElemId {
    /// Per-replica monotonic counter.
    pub counter: u64,
    /// Originating replica name.
    pub replica: String,
}

impl ElemId {
    /// New element id.
    #[must_use]
    pub fn new(counter: u64, replica: &str) -> Self {
        Self {
            counter,
            replica: replica.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Elem {
    /// Causal predecessor (`None` = inserted at the head).
    after: Option<ElemId>,
    ch: char,
    deleted: bool,
}

/// An RGA over the characters of a single block body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BodyCrdt {
    elems: HashMap<ElemId, Elem>,
}

impl BodyCrdt {
    /// Empty body.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert character `ch` with identity `id` immediately after
    /// `after` (`None` = at the head). A later concurrent insert sharing
    /// the same `after` is ordered deterministically by `id`.
    pub fn insert_after(&mut self, after: Option<ElemId>, ch: char, id: ElemId) {
        self.elems.insert(
            id,
            Elem {
                after,
                ch,
                deleted: false,
            },
        );
    }

    /// Tombstone the element `id` (a no-op if unknown). Deletes are
    /// permanent — the element stays as a tombstone so concurrent
    /// inserts that reference it still linearise correctly.
    pub fn delete(&mut self, id: &ElemId) {
        if let Some(e) = self.elems.get_mut(id) {
            e.deleted = true;
        }
    }

    /// Merge another RGA in: the union of elements by id; a tombstone on
    /// either side wins (a delete is never resurrected). Commutative,
    /// associative, idempotent — the CRDT merge.
    pub fn merge(&mut self, other: &Self) {
        for (id, e) in &other.elems {
            self.elems
                .entry(id.clone())
                .and_modify(|mine| mine.deleted = mine.deleted || e.deleted)
                .or_insert_with(|| e.clone());
        }
    }

    /// The element ids in their deterministic linear order (tombstones
    /// included). Same element set ⇒ same order on every replica.
    fn ordered_ids(&self) -> Vec<ElemId> {
        // children[after] = sibling ids sharing that predecessor.
        let mut children: HashMap<Option<ElemId>, Vec<ElemId>> = HashMap::new();
        for (id, e) in &self.elems {
            children.entry(e.after.clone()).or_default().push(id.clone());
        }
        // Descending id: a newer concurrent insert appears first (RGA).
        for v in children.values_mut() {
            v.sort_by(|a, b| b.cmp(a));
        }
        let mut out = Vec::with_capacity(self.elems.len());
        // Iterative pre-order DFS: emit an element, then its children.
        let mut stack: Vec<ElemId> = children.get(&None).cloned().unwrap_or_default();
        stack.reverse(); // so the first sibling is popped first
        while let Some(id) = stack.pop() {
            out.push(id.clone());
            if let Some(kids) = children.get(&Some(id)) {
                for kid in kids.iter().rev() {
                    stack.push(kid.clone());
                }
            }
        }
        out
    }

    /// The current text: the live (non-tombstoned) characters in order.
    #[must_use]
    pub fn materialize(&self) -> String {
        self.ordered_ids()
            .into_iter()
            .filter_map(|id| {
                let e = &self.elems[&id];
                (!e.deleted).then_some(e.ch)
            })
            .collect()
    }

    /// Reconcile this RGA toward `new_text` as edits authored by
    /// `replica`, minting fresh ids from `counter` (advanced past the
    /// ids used). Characters shared with the current text keep their
    /// element ids (so a peer's concurrent edits to *other* positions
    /// survive the merge); inserted characters get new ids placed after
    /// their predecessor; removed characters are tombstoned.
    ///
    /// A longest-common-subsequence alignment decides which characters
    /// are shared vs inserted/removed, so an edit only touches the
    /// characters that actually changed.
    pub fn edit_to(&mut self, new_text: &str, replica: &str, counter: &mut u64) {
        let old_ids = self.ordered_ids();
        let old: Vec<(ElemId, char)> = old_ids
            .iter()
            .map(|id| (id.clone(), self.elems[id].ch))
            .collect();
        let live: Vec<usize> = old
            .iter()
            .enumerate()
            .filter(|(_, (id, _))| !self.elems[id].deleted)
            .map(|(i, _)| i)
            .collect();
        let old_live_chars: Vec<char> = live.iter().map(|&i| old[i].1).collect();
        let new_chars: Vec<char> = new_text.chars().collect();

        // LCS over the live old chars vs the new chars.
        let keep = lcs_pairs(&old_live_chars, &new_chars);
        let kept_old: std::collections::HashSet<usize> = keep.iter().map(|&(o, _)| o).collect();
        let kept_new: std::collections::HashSet<usize> = keep.iter().map(|&(_, n)| n).collect();

        // Tombstone live chars dropped from the new text.
        for (li, &oi) in live.iter().enumerate() {
            if !kept_old.contains(&li) {
                self.delete(&old[oi].0);
            }
        }

        // Walk the new text; emit inserts after the running predecessor.
        // `pred` is the element id the next insert attaches after; it
        // advances to each kept element so inserts land in place.
        let mut pred: Option<ElemId> = None;
        let mut li = 0usize; // index into `live`
        for (ni, &ch) in new_chars.iter().enumerate() {
            if kept_new.contains(&ni) {
                // Advance to the matching kept live element.
                while li < live.len() {
                    let this = li;
                    li += 1;
                    if kept_old.contains(&this) {
                        pred = Some(old[live[this]].0.clone());
                        break;
                    }
                }
            } else {
                let id = ElemId::new(*counter, replica);
                *counter += 1;
                self.insert_after(pred.clone(), ch, id.clone());
                pred = Some(id);
            }
        }
    }

    /// The highest element-id counter present (0 if empty). New inserts
    /// must be seeded above this so they sort as "newer" than every
    /// existing element (the RGA convergence + correct-placement rule).
    #[must_use]
    pub fn max_counter(&self) -> u64 {
        self.elems.keys().map(|id| id.counter).max().unwrap_or(0)
    }

    /// Build an RGA from scratch representing `text`, authored by
    /// `replica` from `counter`.
    #[must_use]
    pub fn from_text(text: &str, replica: &str, counter: &mut u64) -> Self {
        let mut b = Self::new();
        b.edit_to(text, replica, counter);
        b
    }

    /// Serialise (length-prefixed) into `out` for the wire. Pairs with
    /// `BodyCrdt::decode` (added with the Replica wire integration).
    pub fn encode(&self, out: &mut Vec<u8>) {
        let ids = self.ordered_ids();
        out.extend_from_slice(&u32::try_from(ids.len()).unwrap_or(u32::MAX).to_le_bytes());
        for id in ids {
            let e = &self.elems[&id];
            out.extend_from_slice(&id.counter.to_le_bytes());
            put_str(out, &id.replica);
            // predecessor: presence flag + (counter, replica)
            match &e.after {
                Some(a) => {
                    out.push(1);
                    out.extend_from_slice(&a.counter.to_le_bytes());
                    put_str(out, &a.replica);
                }
                None => out.push(0),
            }
            let mut chbuf = [0u8; 4];
            put_str(out, e.ch.encode_utf8(&mut chbuf));
            out.push(u8::from(e.deleted));
        }
    }

    /// Decode an RGA written by [`Self::encode`] from the shared cursor.
    ///
    /// # Errors
    ///
    /// [`crate::CrdtError::Decode`] on a truncated or malformed buffer.
    pub(crate) fn decode(cur: &mut crate::Cursor) -> Result<Self, crate::CrdtError> {
        let n = cur.u32()?;
        let mut elems = HashMap::with_capacity(n as usize);
        for _ in 0..n {
            let id = ElemId::new(cur.u64()?, &cur.string()?);
            let after = if cur.u8()? == 1 {
                Some(ElemId::new(cur.u64()?, &cur.string()?))
            } else {
                None
            };
            let ch_s = cur.string()?;
            let ch = ch_s
                .chars()
                .next()
                .ok_or_else(|| crate::CrdtError::Decode("empty rga char".into()))?;
            let deleted = cur.u8()? != 0;
            elems.insert(id, Elem { after, ch, deleted });
        }
        Ok(Self { elems })
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&u32::try_from(s.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Longest-common-subsequence index pairs `(old_index, new_index)`,
/// ascending. Classic O(n·m) DP — bodies are small.
fn lcs_pairs(old: &[char], new: &[char]) -> Vec<(usize, usize)> {
    let (on, nn) = (old.len(), new.len());
    let mut dp = vec![vec![0u32; nn + 1]; on + 1];
    for oi in (0..on).rev() {
        for ni in (0..nn).rev() {
            dp[oi][ni] = if old[oi] == new[ni] {
                dp[oi + 1][ni + 1] + 1
            } else {
                dp[oi + 1][ni].max(dp[oi][ni + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut oi, mut ni) = (0, 0);
    while oi < on && ni < nn {
        if old[oi] == new[ni] {
            out.push((oi, ni));
            oi += 1;
            ni += 1;
        } else if dp[oi + 1][ni] >= dp[oi][ni + 1] {
            oi += 1;
        } else {
            ni += 1;
        }
    }
    out
}
