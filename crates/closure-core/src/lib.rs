//! Kernel document model: stable block IDs, command registry, keybinding
//! trie, event bus. Sits on top of [`closure_org`] and is UI-agnostic.
//!
//! This crate defines the frontend-agnostic API surface (spec invariant
//! I7): shells and adapters consume [`Document`], [`BlockId`], and the
//! command registry. They never reach into `closure-org` directly, and
//! they never see byte offsets / spans.

#![forbid(unsafe_code)]

/// Which files a build must watch to notice the commit moved.
pub mod gitwatch;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use closure_org::{
    Headline, OrgDoc, parse, rewrite_add_sibling_after_with_id, rewrite_add_sibling_before_with_id,
    rewrite_headline_demote, rewrite_headline_ensure_id, rewrite_headline_promote,
    rewrite_headline_set_body, rewrite_headline_set_planning, rewrite_headline_set_priority,
    rewrite_headline_set_property, rewrite_headline_set_tags, rewrite_headline_set_todo,
    rewrite_headline_title, rewrite_headline_toggle_archive, rewrite_headline_toggle_comment,
    rewrite_remove_subtree, rewrite_splice_subtree_after,
};
use closure_undo::{NodeId as UndoNodeId, UndoTree};
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
    history: UndoTree<Edit>,
}

/// Flat headline record with stable id and path back into the `OrgDoc`
/// tree.
#[derive(Debug, Clone)]
pub struct DocHeadline {
    id: BlockId,
    path: Vec<usize>,
    title: String,
    level: u8,
    todo: Option<String>,
    priority: Option<char>,
    tags: Vec<String>,
    link_targets: Vec<String>,
    body_text: String,
    scheduled: Option<String>,
    deadline: Option<String>,
    closed: Option<String>,
    properties: Vec<(String, String)>,
    is_comment: bool,
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

    /// TODO keyword if one is set.
    #[must_use]
    pub fn todo(&self) -> Option<&str> {
        self.todo.as_deref()
    }

    /// Priority letter if set.
    #[must_use]
    pub const fn priority(&self) -> Option<char> {
        self.priority
    }

    /// Tags attached to this headline in source order.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Link targets (URLs, `id:`, `wiki:`) found inside the headline's
    /// title and body. Used by backlink queries.
    #[must_use]
    pub fn link_targets(&self) -> &[String] {
        &self.link_targets
    }

    /// Concatenated body text (drawer excluded), useful for full-text
    /// search.
    #[must_use]
    pub fn body_text(&self) -> &str {
        &self.body_text
    }

    /// `SCHEDULED:` planning timestamp if set.
    #[must_use]
    pub fn scheduled(&self) -> Option<&str> {
        self.scheduled.as_deref()
    }

    /// `DEADLINE:` planning timestamp if set.
    #[must_use]
    pub fn deadline(&self) -> Option<&str> {
        self.deadline.as_deref()
    }

    /// `CLOSED:` planning timestamp if set.
    #[must_use]
    pub fn closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    /// All `:KEY: value` entries from the properties drawer in source
    /// order.
    #[must_use]
    pub fn properties(&self) -> &[(String, String)] {
        &self.properties
    }

    /// Lookup a single property value by key (case-sensitive).
    #[must_use]
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// True iff the headline carries the `COMMENT` keyword prefix.
    #[must_use]
    pub const fn is_comment(&self) -> bool {
        self.is_comment
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
        let (headlines, by_id) = build_index(&org);
        Ok(Self {
            org,
            headlines,
            by_id,
            history: UndoTree::new(),
        })
    }

    fn rebuild_index(&mut self) {
        let old: HashMap<Vec<usize>, BlockId> = self
            .headlines
            .iter()
            .map(|h| (h.path.clone(), h.id.clone()))
            .collect();
        let mut headlines: Vec<DocHeadline> = Vec::new();
        for (i, root) in self.org.roots().iter().enumerate() {
            collect_preserving(root, &[i], &mut headlines, &old);
        }
        let mut by_id: HashMap<BlockId, usize> = HashMap::with_capacity(headlines.len());
        for (idx, h) in headlines.iter().enumerate() {
            by_id.insert(h.id.clone(), idx);
        }
        self.headlines = headlines;
        self.by_id = by_id;
    }

    /// Record an edit in the history log. Branching: a new edit
    /// applied after `undo` becomes a sibling rather than overwriting
    /// redo history.
    pub fn push_history(&mut self, e: Edit) -> UndoNodeId {
        self.history.apply(e)
    }

    /// Number of nodes in the history tree.
    #[must_use]
    pub const fn history_len(&self) -> usize {
        self.history.len()
    }

    /// The undo tree flattened for a shell's history pane: one row per
    /// recorded edit in tree order, indented by tree depth, the active
    /// node flagged (undo-tree visualization, I3).
    #[must_use]
    pub fn history_view(&self) -> Vec<HistoryRow> {
        let current = self.history.current();
        let nodes = self.history.nodes();
        // Children by parent, in insertion order — a branch made later
        // is drawn below the one it forked from.
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        let mut roots: Vec<usize> = Vec::new();
        for (i, n) in nodes.iter().enumerate() {
            match n.parent.and_then(|p| nodes.iter().position(|m| m.id == p)) {
                Some(p) => children[p].push(i),
                None => roots.push(i),
            }
        }
        // Depth-first, so a whole branch is together on screen rather
        // than interleaved with the one it forked from. `bars` carries,
        // per ancestor, whether that ancestor still has siblings below
        // it — which is the difference between a `│` and a blank.
        let mut out: Vec<HistoryRow> = Vec::with_capacity(nodes.len());
        let mut stack: Vec<(usize, Option<usize>, Vec<bool>, bool)> = roots
            .iter()
            .rev()
            .enumerate()
            .map(|(rev, &i)| (i, None, Vec::new(), rev == 0))
            .collect();
        while let Some((i, parent_row, bars, last)) = stack.pop() {
            let mut graph = String::new();
            for bar in &bars {
                graph.push_str(if *bar { "│  " } else { "   " });
            }
            if parent_row.is_some() {
                graph.push_str(if last { "└─ " } else { "├─ " });
            }
            let row = out.len();
            out.push(HistoryRow {
                // The tree's own depth, not the drawing's: a root's
                // child is at 1 with no continuation bar beside it.
                depth: self.history.depth(nodes[i].id).unwrap_or(0),
                label: edit_label(&nodes[i].payload),
                is_current: Some(nodes[i].id) == current,
                graph,
                parent: parent_row,
                index: i,
            });
            let mut child_bars = bars;
            if parent_row.is_some() {
                child_bars.push(!last);
            }
            let kids = &children[i];
            for (rev, &child) in kids.iter().enumerate().rev() {
                stack.push((child, Some(row), child_bars.clone(), rev + 1 == kids.len()));
            }
        }
        out
    }

    /// Undo the current edit. Reverses the active node's payload and
    /// moves the cursor to its parent. Returns `UndoError::Empty` when
    /// at the root.
    pub fn undo(&mut self) -> Result<(), UndoError> {
        let Some(current) = self.history.current() else {
            return Err(UndoError::Empty);
        };
        let edit = self
            .history
            .node(current)
            .map(|n| n.payload.clone())
            .ok_or(UndoError::Empty)?;
        edit.reverse(self).map_err(|_| UndoError::ReverseFailed)?;
        self.history.undo().map_err(|_| UndoError::ReverseFailed)?;
        Ok(())
    }

    /// Jump the undo cursor to the history node at `index` (insertion
    /// order — the same order [`Self::history_view`] rows use), by
    /// composing the two existing primitives: [`Self::undo`] up to the
    /// deepest common ancestor, then branch-exact [`Self::redo`] down
    /// to the target ([`closure_undo::UndoTree::path_between`], Q2).
    /// Jumping is cursor navigation, not an edit — no node is added
    /// (undo-tree / vim semantics).
    ///
    /// # Errors
    ///
    /// [`UndoError::Empty`] for an unknown index;
    /// [`UndoError::ReverseFailed`] if a step fails mid-walk.
    pub fn jump_in_history(&mut self, index: usize) -> Result<(), UndoError> {
        let target = self
            .history
            .nodes()
            .get(index)
            .map(|n| n.id)
            .ok_or(UndoError::Empty)?;
        let steps = self
            .history
            .path_between(self.history.current(), target)
            .map_err(|_| UndoError::Empty)?;
        for step in steps {
            match step {
                closure_undo::Step::Undo(_) => self.undo()?,
                closure_undo::Step::Redo(id) => self.redo(Some(id))?,
            }
        }
        Ok(())
    }

    /// Re-apply an undone edit. Without `branch`, picks the most
    /// recently created child of the current node.
    pub fn redo(&mut self, branch: Option<UndoNodeId>) -> Result<(), UndoError> {
        let next = self.history.redo(branch).map_err(|_| UndoError::Empty)?;
        let edit = self
            .history
            .node(next)
            .map(|n| n.payload.clone())
            .ok_or(UndoError::Empty)?;
        edit.replay(self).map_err(|_| UndoError::ReverseFailed)
    }

    /// Path from `roots()` to the headline identified by `id`, if any.
    #[must_use]
    pub fn path_of(&self, id: &BlockId) -> Option<Vec<usize>> {
        self.by_id.get(id).map(|&i| self.headlines[i].path.clone())
    }

    /// Byte-exact source text (carries I1 forward from `closure-org`).
    #[must_use]
    pub fn source(&self) -> String {
        closure_org::print(&self.org)
    }

    /// Stable 64-bit FNV-1a hash of the document source. Useful as a
    /// cache key (e.g. memoised query results, evaluator outputs) that
    /// survives reparses but invalidates on any edit.
    #[must_use]
    pub fn source_hash(&self) -> u64 {
        self.org.source_hash()
    }

    /// Whitespace-separated word count over the document source.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.org.source().split_whitespace().count()
    }

    /// Unicode character count over the document source.
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.org.source().chars().count()
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
    out.push(make_doc_headline(h, path, id));
    for (i, c) in h.children().iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(i);
        collect_headlines(c, &child_path, out);
    }
}

fn make_doc_headline(h: &Headline, path: &[usize], id: BlockId) -> DocHeadline {
    let mut link_targets: Vec<String> = closure_org::find_links(h.title())
        .into_iter()
        .map(|l| l.target.to_owned())
        .collect();
    let mut body_text = String::new();
    for n in h.body() {
        for l in closure_org::find_links(n.source()) {
            link_targets.push(l.target.to_owned());
        }
        body_text.push_str(n.source());
    }
    let planning = h.planning();
    let properties: Vec<(String, String)> = h
        .properties()
        .map(|p| {
            p.iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default();
    DocHeadline {
        id,
        path: path.to_vec(),
        title: h.title().to_owned(),
        level: h.level(),
        todo: h.todo().map(str::to_owned),
        priority: h.priority(),
        tags: h.tags().into_iter().map(str::to_owned).collect(),
        link_targets,
        body_text,
        scheduled: planning.and_then(|p| p.scheduled).map(str::to_owned),
        deadline: planning.and_then(|p| p.deadline).map(str::to_owned),
        closed: planning.and_then(|p| p.closed).map(str::to_owned),
        properties,
        is_comment: h.is_comment(),
    }
}

fn build_index(org: &OrgDoc) -> (Vec<DocHeadline>, HashMap<BlockId, usize>) {
    let mut headlines: Vec<DocHeadline> = Vec::new();
    for (i, root) in org.roots().iter().enumerate() {
        collect_headlines(root, &[i], &mut headlines);
    }
    let mut by_id: HashMap<BlockId, usize> = HashMap::with_capacity(headlines.len());
    for (idx, h) in headlines.iter().enumerate() {
        by_id.insert(h.id.clone(), idx);
    }
    (headlines, by_id)
}

fn collect_preserving(
    h: &Headline,
    path: &[usize],
    out: &mut Vec<DocHeadline>,
    old: &HashMap<Vec<usize>, BlockId>,
) {
    let id = h.properties().and_then(|p| p.id()).map_or_else(
        || old.get(path).cloned().unwrap_or_else(BlockId::fresh),
        BlockId::from_existing,
    );
    out.push(make_doc_headline(h, path, id));
    for (i, c) in h.children().iter().enumerate() {
        let mut cp = path.to_vec();
        cp.push(i);
        collect_preserving(c, &cp, out, old);
    }
}

/// Parsed key chord (e.g. `C-c C-x`, `SPC f f`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord(Vec<String>);

impl KeyChord {
    /// Construct from a list of key strokes.
    #[must_use]
    pub fn from_strokes(strokes: &[&str]) -> Self {
        Self(strokes.iter().map(|s| (*s).to_owned()).collect())
    }
}

impl FromStr for KeyChord {
    type Err = ChordParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            return Err(ChordParseError::Empty);
        }
        Ok(Self(s.split_whitespace().map(str::to_owned).collect()))
    }
}

impl std::fmt::Display for KeyChord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join(" "))
    }
}

/// Chord parse error.
#[derive(Debug, Error)]
pub enum ChordParseError {
    /// Empty string or only whitespace.
    #[error("empty chord")]
    Empty,
}

/// One row of the flattened undo tree ([`Document::history_view`]):
/// depth-indented, human-labelled, the active node flagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    /// Tree depth (root edits at 0).
    pub depth: usize,
    /// Short human description of the edit.
    pub label: String,
    /// Whether this node is the undo cursor.
    pub is_current: bool,
    /// The tree drawing for this row — the ancestors' continuation
    /// bars and this row's own tee or corner (`│  ├─ `).
    ///
    /// Precomputed here rather than in each shell: an undo tree drawn
    /// differently by the TUI and the window is two answers to the same
    /// question (I7).
    pub graph: String,
    /// Index of this row's parent *in this list*, when it has one.
    pub parent: Option<usize>,
    /// Insertion index of the node — what [`Document::jump_in_history`]
    /// addresses. Rows come out in walk order, which is not insertion
    /// order once the history has forked, so a row that did not carry
    /// this would send a click to whatever edit happened to be there.
    pub index: usize,
}

/// Short human label for an [`Edit`] — the undo-history row text.
fn edit_label(e: &Edit) -> String {
    match e {
        Edit::RenameHeadline {
            old_title,
            new_title,
            ..
        } => format!("rename: {old_title} → {new_title}"),
        Edit::SetTodo { old, new, .. } => format!(
            "todo: {} → {}",
            old.as_deref().unwrap_or("∅"),
            new.as_deref().unwrap_or("∅")
        ),
        Edit::Repeat {
            todo, new_planning, ..
        } => format!(
            "repeat: {} → {}",
            todo.0.as_deref().unwrap_or("∅"),
            new_planning
                .0
                .as_deref()
                .or(new_planning.1.as_deref())
                .unwrap_or("∅")
        ),
        Edit::SetPriority { old, new, .. } => format!(
            "priority: {} → {}",
            old.map_or("∅".to_owned(), |c| c.to_string()),
            new.map_or("∅".to_owned(), |c| c.to_string())
        ),
        Edit::SetTags { new, .. } => format!("tags: {}", new.join(" ")),
        Edit::Promote { .. } => "promote".to_owned(),
        Edit::Demote { .. } => "demote".to_owned(),
        Edit::AddSibling { .. } => "add heading".to_owned(),
        Edit::RemoveSubtree { .. } => "remove subtree".to_owned(),
        Edit::SetPlanning { .. } => "planning".to_owned(),
        Edit::SetBody { .. } => "edit body".to_owned(),
        Edit::MoveSubtree { .. } => "move subtree".to_owned(),
        Edit::ToggleArchive { .. } => "archive".to_owned(),
        Edit::ToggleComment { .. } => "comment".to_owned(),
        Edit::SetProperty { key, .. } => format!("property: {key}"),
        Edit::Noop => "no-op".to_owned(),
    }
}

/// An edit record in the undo tree. Each command's `apply` produces an
/// `Edit` whose `reverse` rolls the document back and whose `replay`
/// re-applies the change after a `redo`.
#[derive(Debug, Clone)]
pub enum Edit {
    /// A title rename, with both before and after states so both
    /// directions are addressable.
    RenameHeadline {
        /// Block whose title was renamed.
        id: BlockId,
        /// Title before the edit.
        old_title: String,
        /// Title after the edit.
        new_title: String,
    },
    /// Set or clear the TODO keyword on a headline.
    SetTodo {
        /// Block whose TODO keyword changed.
        id: BlockId,
        /// Keyword before the edit (`None` if absent).
        old: Option<String>,
        /// Keyword after the edit (`None` if cleared).
        new: Option<String>,
    },
    /// Set or clear the `[#X]` priority on a headline.
    SetPriority {
        /// Block whose priority changed.
        id: BlockId,
        /// Priority before the edit.
        old: Option<char>,
        /// Priority after the edit.
        new: Option<char>,
    },
    /// Replace the trailing tag list on a headline.
    SetTags {
        /// Block whose tags changed.
        id: BlockId,
        /// Tags before the edit.
        old: Vec<String>,
        /// Tags after the edit.
        new: Vec<String>,
    },
    /// Promote a headline (decrease level by 1).
    Promote {
        /// Block whose level changed.
        id: BlockId,
    },
    /// Demote a headline (increase level by 1).
    Demote {
        /// Block whose level changed.
        id: BlockId,
    },
    /// Insert a sibling headline.
    AddSibling {
        /// Block id of the headline this sibling sits after.
        after_id: BlockId,
        /// Newly-allocated id pinned into the inserted headline.
        new_id: BlockId,
        /// Title given to the new headline.
        title: String,
    },
    /// Remove a subtree, retaining the deleted source text and the
    /// byte offset it sat at so undo can splice it back.
    RemoveSubtree {
        /// Stable id of the removed headline (also embedded in
        /// `removed_source` via its `:PROPERTIES:` drawer).
        id: BlockId,
        /// Verbatim source text of the subtree as it appeared on disk.
        removed_source: String,
        /// Byte offset in the document where the subtree started.
        insert_at: usize,
    },
    /// Replace a headline's planning line (SCHEDULED/DEADLINE/CLOSED).
    SetPlanning {
        /// Block whose planning line changed.
        id: BlockId,
        /// Previous (scheduled, deadline, closed) timestamps.
        old: (Option<String>, Option<String>, Option<String>),
        /// New (scheduled, deadline, closed) timestamps.
        new: (Option<String>, Option<String>, Option<String>),
    },
    /// Finish one occurrence of a repeating task.
    ///
    /// Three changes that have to undo as one: the keyword went back to
    /// not-done, the date moved on, and `:LAST_REPEAT:` recorded when
    /// this occurrence was finished. Pressing undo once and getting the
    /// keyword back but keeping next week's date would leave a document
    /// that never existed.
    Repeat {
        /// Block that repeated.
        id: BlockId,
        /// Keyword before, and the not-done keyword it went back to.
        todo: (Option<String>, Option<String>),
        /// Planning before, as (scheduled, deadline, closed).
        old_planning: (Option<String>, Option<String>, Option<String>),
        /// Planning after.
        new_planning: (Option<String>, Option<String>, Option<String>),
        /// `:LAST_REPEAT:` before, if it had one.
        old_last_repeat: Option<String>,
        /// `:LAST_REPEAT:` after.
        new_last_repeat: String,
    },
    /// Replace a headline's body wholesale.
    SetBody {
        /// Block whose body changed.
        id: BlockId,
        /// Body before the edit.
        old: String,
        /// Body after the edit.
        new: String,
    },
    /// Move a subtree to immediately after a target headline.
    MoveSubtree {
        /// Block being moved (id pinned in `subtree_source`).
        id: BlockId,
        /// Verbatim subtree source (with pinned `:ID:`).
        subtree_source: String,
        /// Predecessor headline id at the original location, if any.
        old_after_id: Option<BlockId>,
        /// Predecessor headline id at the new location.
        new_after_id: BlockId,
    },
    /// Toggle the `ARCHIVE` tag on a headline. Reversible by re-applying
    /// the toggle.
    ToggleArchive {
        /// Block whose archive tag flipped.
        id: BlockId,
    },
    /// Toggle the `COMMENT` keyword prefix. Reversible by re-applying.
    ToggleComment {
        /// Block whose comment state flipped.
        id: BlockId,
    },
    /// Set a `:KEY: value` entry in the properties drawer.
    SetProperty {
        /// Block whose property drawer changed.
        id: BlockId,
        /// Property key.
        key: String,
        /// Previous value for `key` if any.
        old: Option<String>,
        /// New value.
        new: String,
    },
    /// Idempotent edit (e.g. ensure-id) — undo / redo are no-ops at
    /// the kernel level.
    Noop,
}

impl Edit {
    /// Whether this edit carries enough information to undo itself.
    #[must_use]
    pub const fn is_reversible(&self) -> bool {
        matches!(self, Self::RenameHeadline { .. })
    }

    #[allow(clippy::too_many_lines)]
    fn reverse(&self, doc: &mut Document) -> Result<(), CommandError> {
        match self {
            Self::RenameHeadline { id, old_title, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_title(doc.org(), &path, old_title)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetTodo { id, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_todo(doc.org(), &path, old.as_deref())
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                // A cookie is derived, so restoring what it counts
                // restores it — no old title to carry in the edit, and
                // nothing that can go stale if the count changes for
                // another reason first.
                refresh_parent_cookie(doc, &path);
                doc.rebuild_index();
                Ok(())
            }
            Self::SetPriority { id, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_priority(doc.org(), &path, *old)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetTags { id, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let refs: Vec<&str> = old.iter().map(String::as_str).collect();
                let org = rewrite_headline_set_tags(doc.org(), &path, &refs)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            // Reverse of Promote = Demote (and vice versa).
            Self::Promote { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org =
                    rewrite_headline_demote(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Demote { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_promote(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::AddSibling { new_id, .. } => {
                let path = doc.path_of(new_id).ok_or(CommandError::BlockNotFound)?;
                let org =
                    rewrite_remove_subtree(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::RemoveSubtree {
                removed_source,
                insert_at,
                ..
            } => {
                let mut src = doc.org().source().to_owned();
                let pos = (*insert_at).min(src.len());
                src.insert_str(pos, removed_source);
                let org = closure_org::parse(&src).map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetBody { id, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_body(doc.org(), &path, old)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetPlanning { id, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_planning(
                    doc.org(),
                    &path,
                    old.0.as_deref(),
                    old.1.as_deref(),
                    old.2.as_deref(),
                )
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Repeat {
                id,
                todo,
                old_planning,
                old_last_repeat,
                ..
            } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_todo(doc.org(), &path, todo.0.as_deref())
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                let org = rewrite_headline_set_planning(
                    doc.org(),
                    &path,
                    old_planning.0.as_deref(),
                    old_planning.1.as_deref(),
                    old_planning.2.as_deref(),
                )
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                // A property that was not there before is cleared
                // rather than set to an empty string, so undo leaves
                // the drawer as it found it.
                let org = match old_last_repeat {
                    Some(v) => closure_org::rewrite_headline_set_property(
                        doc.org(),
                        &path,
                        "LAST_REPEAT",
                        v,
                    ),
                    None => closure_org::rewrite_headline_remove_property(
                        doc.org(),
                        &path,
                        "LAST_REPEAT",
                    ),
                }
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::ToggleArchive { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_toggle_archive(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::ToggleComment { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_toggle_comment(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetProperty { id, key, old, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = old
                    .as_ref()
                    .map_or(Err(closure_org::RewriteError::Parse), |prev| {
                        rewrite_headline_set_property(doc.org(), &path, key, prev)
                    })
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::MoveSubtree {
                id,
                subtree_source,
                old_after_id,
                ..
            } => {
                // Remove from current (new) location, splice back at
                // old location.
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let intermediate =
                    rewrite_remove_subtree(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                let after_id = old_after_id.as_ref().ok_or(CommandError::Rewrite)?;
                let after_path =
                    path_of_in(&intermediate, after_id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_splice_subtree_after(&intermediate, &after_path, subtree_source)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Noop => Ok(()),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn replay(&self, doc: &mut Document) -> Result<(), CommandError> {
        match self {
            Self::RenameHeadline { id, new_title, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_title(doc.org(), &path, new_title)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetTodo { id, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_todo(doc.org(), &path, new.as_deref())
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                refresh_parent_cookie(doc, &path);
                doc.rebuild_index();
                Ok(())
            }
            Self::SetPriority { id, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_priority(doc.org(), &path, *new)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetTags { id, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let refs: Vec<&str> = new.iter().map(String::as_str).collect();
                let org = rewrite_headline_set_tags(doc.org(), &path, &refs)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Promote { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_promote(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Demote { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org =
                    rewrite_headline_demote(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::AddSibling {
                after_id,
                new_id,
                title,
            } => {
                let path = doc.path_of(after_id).ok_or(CommandError::BlockNotFound)?;
                let org =
                    rewrite_add_sibling_after_with_id(doc.org(), &path, title, new_id.as_str())
                        .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::RemoveSubtree { id, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org =
                    rewrite_remove_subtree(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetBody { id, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_body(doc.org(), &path, new)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Repeat {
                id,
                todo,
                new_planning,
                new_last_repeat,
                ..
            } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_todo(doc.org(), &path, todo.1.as_deref())
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                let org = rewrite_headline_set_planning(
                    doc.org(),
                    &path,
                    new_planning.0.as_deref(),
                    new_planning.1.as_deref(),
                    new_planning.2.as_deref(),
                )
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                let org = closure_org::rewrite_headline_set_property(
                    doc.org(),
                    &path,
                    "LAST_REPEAT",
                    new_last_repeat,
                )
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetPlanning { id, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_planning(
                    doc.org(),
                    &path,
                    new.0.as_deref(),
                    new.1.as_deref(),
                    new.2.as_deref(),
                )
                .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::ToggleArchive { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_toggle_archive(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::ToggleComment { id } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_toggle_comment(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::SetProperty { id, key, new, .. } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_headline_set_property(doc.org(), &path, key, new)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::MoveSubtree {
                id,
                subtree_source,
                new_after_id,
                ..
            } => {
                let path = doc.path_of(id).ok_or(CommandError::BlockNotFound)?;
                let intermediate =
                    rewrite_remove_subtree(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
                let after_path =
                    path_of_in(&intermediate, new_after_id).ok_or(CommandError::BlockNotFound)?;
                let org = rewrite_splice_subtree_after(&intermediate, &after_path, subtree_source)
                    .map_err(|_| CommandError::Rewrite)?;
                doc.org = org;
                doc.rebuild_index();
                Ok(())
            }
            Self::Noop => Ok(()),
        }
    }
}

/// Lookup a headline path by `BlockId` in a freshly-parsed `OrgDoc`,
/// without going through a full `Document` index. Used by
/// `Edit::MoveSubtree` reverse / replay where the kernel hands an
/// intermediate state to closure-org without rebuilding the index in
/// between.
fn path_of_in(org: &OrgDoc, id: &BlockId) -> Option<Vec<usize>> {
    fn walk(h: &Headline, target: &str, path: &[usize], out: &mut Option<Vec<usize>>) {
        if out.is_some() {
            return;
        }
        if let Some(p) = h.properties()
            && p.id() == Some(target)
        {
            *out = Some(path.to_vec());
            return;
        }
        for (i, c) in h.children().iter().enumerate() {
            let mut child_path = path.to_vec();
            child_path.push(i);
            walk(c, target, &child_path, out);
        }
    }
    let mut out: Option<Vec<usize>> = None;
    for (i, root) in org.roots().iter().enumerate() {
        walk(root, id.as_str(), &[i], &mut out);
        if out.is_some() {
            break;
        }
    }
    out
}

/// Failure mode during a command execution.
#[derive(Debug, Error)]
pub enum CommandError {
    /// Referenced block doesn't exist.
    #[error("block id not found")]
    BlockNotFound,
    /// Underlying rewrite failed.
    #[error("rewrite failed")]
    Rewrite,
}

/// Failure mode during undo.
#[derive(Debug, Error)]
pub enum UndoError {
    /// History is empty.
    #[error("nothing to undo")]
    Empty,
    /// Inverse rewrite failed.
    #[error("reverse edit failed")]
    ReverseFailed,
}

/// A registered command.
pub trait Command {
    /// Stable identifier for this command (kebab-case).
    fn name(&self) -> &str;
    /// Default keybindings (I4).
    fn keys(&self) -> &[KeyChord];
    /// Apply the command to the document, producing an [`Edit`] that
    /// the undo-tree can replay in reverse.
    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError>;
}

/// Build a registry with every built-in mutation command registered
/// at its default chord. Shells get a complete which-key listing
/// without hand-wiring every command name.
#[must_use]
pub fn default_registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r.register(Box::new(EnsureId::new_placeholder()));
    r.register(Box::new(SetTodo::new_placeholder()));
    r.register(Box::new(SetPriority::new_placeholder()));
    r.register(Box::new(SetTags::new_placeholder()));
    r.register(Box::new(SetBody::new_placeholder()));
    r.register(Box::new(SetPlanning::new_placeholder()));
    r.register(Box::new(ToggleArchive::new_placeholder()));
    r.register(Box::new(ToggleComment::new_placeholder()));
    r.register(Box::new(SetProperty::new_placeholder()));
    r.register(Box::new(Promote::new_placeholder()));
    r.register(Box::new(Demote::new_placeholder()));
    r.register(Box::new(AddSibling::new_placeholder()));
    r.register(Box::new(RemoveSubtree::new_placeholder()));
    r.register(Box::new(MoveSubtree::new_placeholder()));
    r
}

/// The command registry: name → command.
#[derive(Default)]
pub struct Registry {
    by_name: HashMap<String, Box<dyn Command + Send + Sync>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("commands", &self.by_name.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Registry {
    /// Fresh, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command.
    pub fn register(&mut self, cmd: Box<dyn Command + Send + Sync>) {
        self.by_name.insert(cmd.name().to_owned(), cmd);
    }

    /// Lookup a command by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&(dyn Command + Send + Sync)> {
        self.by_name.get(name).map(std::convert::AsRef::as_ref)
    }

    /// Iterate registered `(name, command)` pairs.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &(dyn Command + Send + Sync))> {
        self.by_name
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_ref() as &(dyn Command + Send + Sync)))
    }

    /// Iterate registered command names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }
}

/// Command: rename a headline's title.
pub struct RenameHeadline {
    id: BlockId,
    new_title: String,
    keys: Vec<KeyChord>,
}

impl RenameHeadline {
    /// Build a rename command for a specific block id and new title.
    #[must_use]
    pub fn new(id: BlockId, new_title: String) -> Self {
        Self {
            id,
            new_title,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "r"])],
        }
    }

    /// Placeholder instance used for registry discovery and key
    /// introspection before the user has picked a target/title.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new_title: String::new(),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "r"])],
        }
    }
}

impl Command for RenameHeadline {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "rename-headline"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let old_title = doc
            .headline_by_id(&self.id)
            .ok_or(CommandError::BlockNotFound)?
            .title()
            .to_owned();
        let org = rewrite_headline_title(doc.org(), &path, &self.new_title)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::RenameHeadline {
            id: self.id.clone(),
            old_title,
            new_title: self.new_title.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: set or clear the TODO keyword on a headline.
pub struct SetTodo {
    id: BlockId,
    new: Option<String>,
    keys: Vec<KeyChord>,
}

impl SetTodo {
    /// Set TODO keyword to `new` (or clear with `None`).
    #[must_use]
    pub fn new(id: BlockId, new: Option<String>) -> Self {
        Self {
            id,
            new,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-t"])],
        }
    }

    /// Placeholder for registry introspection.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new: None,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-t"])],
        }
    }
}

impl Command for SetTodo {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-todo"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let old = doc
            .headline_by_id(&self.id)
            .ok_or(CommandError::BlockNotFound)?
            .todo()
            .map(str::to_owned);
        // A repeating task is never done, only done *this time*: org's
        // rule, and the reason a repeater is worth reading at all. The
        // date moves on, the keyword goes back to a not-done one, and
        // `:LAST_REPEAT:` records when this one was finished.
        let repeat = self
            .new
            .as_deref()
            .filter(|k| DONE_KEYWORDS.contains(k))
            .and_then(|_| repeat_of(doc, &self.id));
        let effective = match &repeat {
            Some(_) => Some(NOT_DONE.to_owned()),
            None => self.new.clone(),
        };
        let org = rewrite_headline_set_todo(doc.org(), &path, effective.as_deref())
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        // A parent's cookie changing *is* part of finishing the child:
        // one edit, one undo. A separate command would let the two
        // drift by exactly the amount somebody forgot to run it, and a
        // `[1/3]` nobody updates is worse than no cookie — an absent
        // count says nothing, a stale one says something false while
        // looking maintained.
        refresh_parent_cookie(doc, &path);
        let edit = if let Some(Repeat {
            scheduled,
            deadline,
            today,
            was,
            last_repeat,
        }) = repeat
        {
            let org = closure_org::rewrite_headline_set_planning(
                doc.org(),
                &path,
                scheduled.as_deref(),
                deadline.as_deref(),
                None,
            )
            .map_err(|_| CommandError::Rewrite)?;
            doc.org = org;
            let org =
                closure_org::rewrite_headline_set_property(doc.org(), &path, "LAST_REPEAT", &today)
                    .map_err(|_| CommandError::Rewrite)?;
            doc.org = org;
            Edit::Repeat {
                id: self.id.clone(),
                todo: (old, effective),
                old_planning: was,
                new_planning: (scheduled, deadline, None),
                old_last_repeat: last_repeat,
                new_last_repeat: today,
            }
        } else {
            Edit::SetTodo {
                id: self.id.clone(),
                old,
                new: effective,
            }
        };
        doc.rebuild_index();
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Bring the cookie in the parent of `path` up to date, if it has one.
///
/// Silent when the parent has none: adding one nobody asked for would
/// edit a title the author wrote.
fn refresh_parent_cookie(doc: &mut Document, path: &[usize]) {
    if path.len() < 2 {
        return;
    }
    let parent_path = path[..path.len() - 1].to_vec();
    let Some(parent) = closure_org::headline_at(doc.org(), &parent_path) else {
        return;
    };
    let title = parent.title().to_owned();
    let Some(kind) = closure_org::cookie_in(&title) else {
        return;
    };
    let Some(id) = parent.id_property().map(ToOwned::to_owned) else {
        return;
    };
    let Some((done, total)) = closure_org::statistics_for(doc.org(), &id) else {
        return;
    };
    let fresh = closure_org::render_cookie(kind, done, total);
    // Replace the cookie in place, leaving the rest of the title as the
    // author wrote it.
    let (Some(open), Some(close)) = (title.find('['), title.find(']')) else {
        return;
    };
    let mut new_title = title.clone();
    new_title.replace_range(open..=close, &fresh);
    if let Ok(org) = closure_org::rewrite_headline_title(doc.org(), &parent_path, &new_title) {
        doc.org = org;
    }
}

/// Keywords that mean a task is finished.
///
/// Two, matching `closure-org`'s own list. A configurable set is a
/// different feature and belongs beside the rest of the config (I9).
const DONE_KEYWORDS: &[&str] = &["DONE"];

/// Where a finished repeating task goes back to.
const NOT_DONE: &str = "TODO";

/// What a repeating headline's planning line becomes when this
/// occurrence is finished.
struct Repeat {
    /// The advanced `SCHEDULED:`, if it had one that repeats.
    scheduled: Option<String>,
    /// The advanced `DEADLINE:`, if it had one that repeats.
    deadline: Option<String>,
    /// Today, `YYYY-MM-DD`, for `:LAST_REPEAT:`.
    today: String,
    /// The planning line as it was, so undo can put it back.
    was: (Option<String>, Option<String>, Option<String>),
    /// `:LAST_REPEAT:` as it was, if there was one.
    last_repeat: Option<String>,
}

/// The advanced planning for the headline `id`, if either of its
/// dates repeats.
fn repeat_of(doc: &Document, id: &BlockId) -> Option<Repeat> {
    let planning = doc.org().planning_of(&id.to_string())?;
    let today = today_civil();
    let scheduled = planning
        .scheduled
        .and_then(|ts| closure_org::advance(ts, &today));
    let deadline = planning
        .deadline
        .and_then(|ts| closure_org::advance(ts, &today));
    if scheduled.is_none() && deadline.is_none() {
        return None;
    }
    Some(Repeat {
        // Anything that did not repeat is kept as it was rather than
        // dropped: `set_planning` writes what it is given.
        scheduled: scheduled.or_else(|| planning.scheduled.map(ToOwned::to_owned)),
        deadline: deadline.or_else(|| planning.deadline.map(ToOwned::to_owned)),
        today,
        was: (
            planning.scheduled.map(ToOwned::to_owned),
            planning.deadline.map(ToOwned::to_owned),
            planning.closed.map(ToOwned::to_owned),
        ),
        last_repeat: doc
            .org()
            .properties_of(&id.to_string())
            .and_then(|p| p.get("LAST_REPEAT").map(ToOwned::to_owned)),
    })
}

/// Today as `YYYY-MM-DD`, from the system clock.
fn today_civil() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    #[allow(clippy::cast_possible_wrap)]
    let (y, m, d) = closure_org::civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Command: recompute a headline's table from its `#+TBLFM:` line.
///
/// Org's `C-c C-c` on a formula line. `closure_org::eval_table_formulas`
/// says what each cell should be; this is what puts it back, which
/// makes it a mutation and so a command (I8) with an `Edit` the undo
/// tree can reverse (I3).
///
/// One edit for the whole table, not one per cell: a half-recomputed
/// table is a table that never existed, and `undo` has to be able to
/// say so.
pub struct RecomputeTable {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl RecomputeTable {
    /// Recompute the table in the body of `id`.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-c"])],
        }
    }

    /// Placeholder for registry introspection.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self::new(BlockId::from_existing(""))
    }
}

impl Command for RecomputeTable {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "recompute-table"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let body = doc
            .headline_by_id(&self.id)
            .ok_or(CommandError::BlockNotFound)?
            .body_text()
            .to_owned();
        let formulas = body
            .lines()
            .find_map(closure_org::table_formulas)
            .ok_or(CommandError::Rewrite)?;

        // The table's data rows, cell by cell, with the separator and
        // the header left where they are: a formula is about data, and
        // computing over the header would put a number where the
        // column's name goes.
        let mut out = String::with_capacity(body.len());
        let mut seen_header = false;
        for line in body.lines() {
            let trimmed = line.trim();
            let is_row = trimmed.starts_with('|');
            let is_separator = is_row && trimmed.contains("|-");
            if !is_row || is_separator || !seen_header {
                if is_row && !is_separator {
                    seen_header = true;
                }
                out.push_str(line);
                out.push('\n');
                continue;
            }
            let cells: Vec<String> = trimmed
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect();
            let computed = closure_org::eval_table_formulas(&[cells.clone()], &formulas);
            let widths: Vec<usize> = trimmed.trim_matches('|').split('|').map(str::len).collect();
            // Rebuilt at the widths the file already used, so a
            // recompute is a change of values and not of layout.
            use std::fmt::Write as _;
            let mut rebuilt = String::from("|");
            for (i, cell) in computed[0].iter().enumerate() {
                let w = widths.get(i).copied().unwrap_or(cell.len() + 2);
                let _ = write!(rebuilt, " {cell:<width$}|", width = w.saturating_sub(1));
            }
            out.push_str(&rebuilt);
            out.push('\n');
        }

        let org = closure_org::rewrite_headline_set_body(doc.org(), &path, &out)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetBody {
            id: self.id.clone(),
            old: body,
            new: out,
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: set or clear the `[#X]` priority on a headline.
pub struct SetPriority {
    id: BlockId,
    new: Option<char>,
    keys: Vec<KeyChord>,
}

impl SetPriority {
    /// Set priority cookie to `new` (or clear with `None`).
    #[must_use]
    pub fn new(id: BlockId, new: Option<char>) -> Self {
        Self {
            id,
            new,
            keys: vec![KeyChord::from_strokes(&["C-c", ","])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new: None,
            keys: vec![KeyChord::from_strokes(&["C-c", ","])],
        }
    }
}

impl Command for SetPriority {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-priority"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let old = doc
            .headline_by_id(&self.id)
            .ok_or(CommandError::BlockNotFound)?
            .priority();
        let org = rewrite_headline_set_priority(doc.org(), &path, self.new)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetPriority {
            id: self.id.clone(),
            old,
            new: self.new,
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: replace the trailing tag list on a headline.
pub struct SetTags {
    id: BlockId,
    new: Vec<String>,
    keys: Vec<KeyChord>,
}

impl SetTags {
    /// Replace tags wholesale.
    #[must_use]
    pub fn new(id: BlockId, new: Vec<String>) -> Self {
        Self {
            id,
            new,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-q"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new: Vec::new(),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-q"])],
        }
    }
}

impl Command for SetTags {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-tags"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let old: Vec<String> = doc
            .headline_by_id(&self.id)
            .ok_or(CommandError::BlockNotFound)?
            .tags()
            .to_vec();
        let refs: Vec<&str> = self.new.iter().map(String::as_str).collect();
        let org = rewrite_headline_set_tags(doc.org(), &path, &refs)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetTags {
            id: self.id.clone(),
            old,
            new: self.new.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: promote a headline (decrease level by 1).
pub struct Promote {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl Promote {
    /// Promote the headline with this id.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["M-S-<left>"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["M-S-<left>"])],
        }
    }
}

impl Command for Promote {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "promote-headline"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        // Raising the stars is only half of it. Left where it is, the
        // promoted headline sits *between* its former siblings, and
        // every sibling below — which was a child of the same parent —
        // silently becomes a child of it. Org does that too and it is
        // defensible on the grounds that nothing moved, but it is
        // expensive to undo: you must promote each stranded sibling in
        // turn and hope you counted right.
        //
        // "The default case is that just this subheading gets
        // promoted." So the subtree steps out of the parent it is
        // leaving and lands after it, and the rest of the tree keeps
        // the shape it had.
        //
        // Captured and dedented *before* anything moves: promoting
        // first would change this headline's own path, and the removal
        // would then address the wrong node.
        let org = match path.split_last() {
            Some((_, parent)) if !parent.is_empty() => {
                let moved = closure_org::subtree_source_at(doc.org(), &path)
                    .ok_or(CommandError::Rewrite)?;
                let dedented = dedent_subtree(moved).ok_or(CommandError::Rewrite)?;
                let parent = parent.to_vec();
                let without = closure_org::rewrite_remove_subtree(doc.org(), &path)
                    .map_err(|_| CommandError::Rewrite)?;
                closure_org::rewrite_splice_subtree_after(&without, &parent, &dedented)
                    .map_err(|_| CommandError::Rewrite)?
            }
            // No parent to step out of: a top-level headline has
            // nowhere to be promoted to, which this refuses.
            _ => rewrite_headline_promote(doc.org(), &path).map_err(|_| CommandError::Rewrite)?,
        };
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::Promote {
            id: self.id.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// One star off every headline in a captured subtree.
///
/// `None` when the subtree's own root is already at level 1 — there is
/// no level above it to promote into, and silently returning the text
/// unchanged would report a move that did not happen.
fn dedent_subtree(source: &str) -> Option<String> {
    let root = source.lines().next()?;
    let depth = root.chars().take_while(|c| *c == '*').count();
    if depth <= 1 {
        return None;
    }
    let mut out = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        // Only headlines carry stars in column zero; a body line that
        // begins with `*` is emphasis and is left alone.
        if line.starts_with("**") {
            out.push_str(&line[1..]);
        } else {
            out.push_str(line);
        }
    }
    Some(out)
}

/// Command: demote a headline (increase level by 1).
pub struct Demote {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl Demote {
    /// Demote the headline with this id.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["M-S-<right>"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["M-S-<right>"])],
        }
    }
}

impl Command for Demote {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "demote-headline"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let org = rewrite_headline_demote(doc.org(), &path).map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::Demote {
            id: self.id.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: replace the body of a headline (between drawer and first
/// child) with new text.
pub struct SetBody {
    id: BlockId,
    new: String,
    keys: Vec<KeyChord>,
}

impl SetBody {
    /// Replace body wholesale.
    #[must_use]
    pub fn new(id: BlockId, new: String) -> Self {
        Self {
            id,
            new,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-b"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new: String::new(),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-b"])],
        }
    }
}

impl Command for SetBody {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-body"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        // Capture current body verbatim.
        let with_id = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        let old = current_body(&with_id, &path)?;
        let org = rewrite_headline_set_body(&with_id, &path, &self.new)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetBody {
            id: self.id.clone(),
            old,
            new: self.new.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: set or clear the planning line (`SCHEDULED:` /
/// `DEADLINE:` / `CLOSED:`) on a headline. Pass `None` for any field
/// to omit it; passing all three as `None` removes any existing
/// planning line.
pub struct SetPlanning {
    id: BlockId,
    scheduled: Option<String>,
    deadline: Option<String>,
    closed: Option<String>,
    keys: Vec<KeyChord>,
}

impl SetPlanning {
    /// Set planning to a new triple.
    #[must_use]
    pub fn new(
        id: BlockId,
        scheduled: Option<String>,
        deadline: Option<String>,
        closed: Option<String>,
    ) -> Self {
        Self {
            id,
            scheduled,
            deadline,
            closed,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-s"])],
        }
    }

    /// Placeholder for registry.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            scheduled: None,
            deadline: None,
            closed: None,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-s"])],
        }
    }
}

impl Command for SetPlanning {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-planning"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let with_id = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        let old = current_planning(&with_id, &path)?;
        let org = rewrite_headline_set_planning(
            &with_id,
            &path,
            self.scheduled.as_deref(),
            self.deadline.as_deref(),
            self.closed.as_deref(),
        )
        .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetPlanning {
            id: self.id.clone(),
            old,
            new: (
                self.scheduled.clone(),
                self.deadline.clone(),
                self.closed.clone(),
            ),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Triple of optional `(scheduled, deadline, closed)` planning timestamps.
type PlanningTriple = (Option<String>, Option<String>, Option<String>);

/// Command: set a `:KEY: value` entry on a headline's properties drawer.
pub struct SetProperty {
    id: BlockId,
    key: String,
    new: String,
    keys: Vec<KeyChord>,
}

impl SetProperty {
    /// Set `key` to `value` on `id`.
    #[must_use]
    pub fn new(id: BlockId, key: String, new: String) -> Self {
        Self {
            id,
            key,
            new,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "p"])],
        }
    }

    /// Placeholder for registry.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            key: String::new(),
            new: String::new(),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "p"])],
        }
    }
}

impl Command for SetProperty {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "set-property"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let with_id = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        let h = navigate_in(&with_id, &path).ok_or(CommandError::BlockNotFound)?;
        let prev = h
            .properties()
            .and_then(|p| p.get(&self.key))
            .map(str::to_owned);
        let org = rewrite_headline_set_property(&with_id, &path, &self.key, &self.new)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetProperty {
            id: self.id.clone(),
            key: self.key.clone(),
            old: prev,
            new: self.new.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

fn navigate_in<'a>(org: &'a OrgDoc, path: &[usize]) -> Option<&'a Headline> {
    let first = *path.first()?;
    let mut cur = org.roots().get(first)?;
    for &i in &path[1..] {
        cur = cur.children().get(i)?;
    }
    Some(cur)
}

/// Command: flip the `COMMENT` keyword prefix on a headline.
pub struct ToggleComment {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl ToggleComment {
    /// Toggle COMMENT on `id`.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", ";"])],
        }
    }

    /// Placeholder for registry.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", ";"])],
        }
    }
}

impl Command for ToggleComment {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "toggle-comment"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let with_id = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        let org =
            rewrite_headline_toggle_comment(&with_id, &path).map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::ToggleComment {
            id: self.id.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: flip the `ARCHIVE` tag on a headline.
pub struct ToggleArchive {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl ToggleArchive {
    /// Toggle archive on `id`.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "a"])],
        }
    }

    /// Placeholder for registry.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "a"])],
        }
    }
}

impl Command for ToggleArchive {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "toggle-archive"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let with_id = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        let org =
            rewrite_headline_toggle_archive(&with_id, &path).map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::ToggleArchive {
            id: self.id.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

fn current_planning(org: &OrgDoc, path: &[usize]) -> Result<PlanningTriple, CommandError> {
    let mut current = org
        .roots()
        .get(*path.first().ok_or(CommandError::BlockNotFound)?)
        .ok_or(CommandError::BlockNotFound)?;
    for &i in &path[1..] {
        current = current
            .children()
            .get(i)
            .ok_or(CommandError::BlockNotFound)?;
    }
    Ok(current.planning().map_or((None, None, None), |p| {
        (
            p.scheduled.map(str::to_owned),
            p.deadline.map(str::to_owned),
            p.closed.map(str::to_owned),
        )
    }))
}

fn current_body(org: &OrgDoc, path: &[usize]) -> Result<String, CommandError> {
    let mut node = org
        .roots()
        .get(*path.first().ok_or(CommandError::BlockNotFound)?);
    let mut current = node.ok_or(CommandError::BlockNotFound)?;
    for &i in &path[1..] {
        node = current.children().get(i);
        current = node.ok_or(CommandError::BlockNotFound)?;
    }
    let mut out = String::new();
    for n in current.body() {
        out.push_str(n.source());
    }
    Ok(out)
}

/// Command: move the subtree rooted at `id` to immediately after the
/// subtree of `new_after_id`. Both before and after positions are
/// reachable through the registered `Edit::MoveSubtree`.
///
/// Currently does not support moving the very first headline of a
/// document (where no predecessor exists). Such moves return
/// `CommandError::Rewrite`.
pub struct MoveSubtree {
    id: BlockId,
    new_after_id: BlockId,
    keys: Vec<KeyChord>,
}

impl MoveSubtree {
    /// Move headline `id` to right after `new_after_id`'s subtree.
    #[must_use]
    pub fn new(id: BlockId, new_after_id: BlockId) -> Self {
        Self {
            id,
            new_after_id,
            keys: vec![KeyChord::from_strokes(&["M-S-<down>"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            new_after_id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["M-S-<down>"])],
        }
    }
}

impl Command for MoveSubtree {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "move-subtree"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        // Pin :ID: drawers on every headline whose path we need to
        // address through path_of_in after intermediate rewrites:
        // moving headline, new predecessor, AND old predecessor (so
        // undo can find it).
        let move_path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let after_path_initial = doc
            .path_of(&self.new_after_id)
            .ok_or(CommandError::BlockNotFound)?;
        // Predecessor's id (in-memory): look it up before any rewrite.
        let old_pred_id: Option<BlockId> = doc_predecessor(doc, &move_path);

        let mut org = doc.org().clone();
        org = rewrite_headline_ensure_id(&org, &move_path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        org = rewrite_headline_ensure_id(&org, &after_path_initial, self.new_after_id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        if let Some(pid) = &old_pred_id {
            let pred_path = doc.path_of(pid).ok_or(CommandError::BlockNotFound)?;
            org = rewrite_headline_ensure_id(&org, &pred_path, pid.as_str())
                .map_err(|_| CommandError::Rewrite)?;
        }

        let path = path_of_in(&org, &self.id).ok_or(CommandError::BlockNotFound)?;
        let (_, subtree_source) = capture_subtree(&org, &path)?;
        let intermediate =
            rewrite_remove_subtree(&org, &path).map_err(|_| CommandError::Rewrite)?;
        let after_path =
            path_of_in(&intermediate, &self.new_after_id).ok_or(CommandError::BlockNotFound)?;
        let org = rewrite_splice_subtree_after(&intermediate, &after_path, &subtree_source)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::MoveSubtree {
            id: self.id.clone(),
            subtree_source,
            old_after_id: old_pred_id,
            new_after_id: self.new_after_id.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Predecessor's id in document order. Looks at the previous sibling
/// path (last path component - 1) and returns that `DocHeadline`'s id.
fn doc_predecessor(doc: &Document, path: &[usize]) -> Option<BlockId> {
    let last = *path.last()?;
    if last == 0 {
        return None;
    }
    let mut prev = path.to_vec();
    prev.pop();
    prev.push(last - 1);
    doc.headlines
        .iter()
        .find(|h| h.path == prev)
        .map(|h| h.id.clone())
}

/// Find the previous sibling's id (in tree order) for the headline at
/// `path` within `org`. Returns `None` for first-child or root[0].
#[allow(dead_code)]
fn predecessor_id(org: &OrgDoc, path: &[usize]) -> Option<BlockId> {
    let last = *path.last()?;
    if last == 0 {
        return None;
    }
    let mut parent_path = path.to_vec();
    parent_path.pop();
    parent_path.push(last - 1);
    // Navigate to the previous sibling.
    let mut node = org.roots().get(*parent_path.first()?)?;
    for &i in &parent_path[1..] {
        node = node.children().get(i)?;
    }
    let id_str = node.properties().and_then(|p| p.id())?;
    Some(BlockId::from_existing(id_str))
}

/// Command: remove the subtree rooted at the given block.
///
/// Currently irreversible at the kernel level — the deleted source
/// is not retained for `undo` / `redo`. A reversible variant lands
/// once the `Edit::RemoveSubtree` payload carries the saved subtree
/// text.
pub struct RemoveSubtree {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl RemoveSubtree {
    /// Remove the subtree rooted at `id`.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-w"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-w"])],
        }
    }
}

impl Command for RemoveSubtree {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "remove-subtree"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        // Ensure the headline has an :ID: drawer so the captured
        // source carries enough info for replay.
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let with_id = closure_org::rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        // Compute insertion point and saved source from the with-id
        // version, then remove from it.
        let (insert_at, removed_source) = capture_subtree(&with_id, &path)?;
        let org = closure_org::rewrite_remove_subtree(&with_id, &path)
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::RemoveSubtree {
            id: self.id.clone(),
            removed_source,
            insert_at,
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

fn capture_subtree(doc: &OrgDoc, path: &[usize]) -> Result<(usize, String), CommandError> {
    let mut node = doc
        .roots()
        .get(*path.first().ok_or(CommandError::BlockNotFound)?);
    let mut current = node.ok_or(CommandError::BlockNotFound)?;
    for &i in &path[1..] {
        node = current.children().get(i);
        current = node.ok_or(CommandError::BlockNotFound)?;
    }
    let src = doc.source();
    let header = current.header();
    let start = src.find(header).ok_or(CommandError::Rewrite)?;
    // Determine end via header + body + recursive children.
    let end = subtree_end_offset(src, current, start);
    Ok((start, src[start..end].to_owned()))
}

fn subtree_end_offset(src: &str, h: &closure_org::Headline, begin: usize) -> usize {
    let after_header = begin + h.header().len();
    let level = h.level();
    let mut cursor = after_header;
    while cursor < src.len() {
        let line_end = src[cursor..]
            .find('\n')
            .map_or(src.len(), |n| cursor + n + 1);
        let line = &src[cursor..line_end];
        let nstars = line.chars().take_while(|&c| c == '*').count();
        if nstars > 0
            && nstars <= usize::from(level)
            && line
                .as_bytes()
                .get(nstars)
                .is_some_and(|b| *b == b' ' || *b == b'\n')
        {
            break;
        }
        cursor = line_end;
        if line_end == src.len() {
            break;
        }
    }
    cursor
}

/// Command: insert a new sibling headline beside the given block.
///
/// Above or below is a flag rather than a second command: the two
/// differ in one offset, and everything else — the id, the drawer, the
/// history entry, the undo — is the same insertion. A second command
/// would be a second owner of all of it.
pub struct AddSibling {
    after_id: BlockId,
    title: String,
    /// Insert before the target instead of after its subtree
    /// (Doom's `+org/insert-item-above`).
    before: bool,
    keys: Vec<KeyChord>,
}

impl AddSibling {
    /// Insert a new sibling after the headline with `after_id`.
    #[must_use]
    pub fn new(after_id: BlockId, title: String) -> Self {
        Self {
            after_id,
            title,
            before: false,
            keys: vec![KeyChord::from_strokes(&["M-<return>"])],
        }
    }

    /// Insert a new sibling *before* the headline with `before_id`.
    #[must_use]
    pub fn before(before_id: BlockId, title: String) -> Self {
        Self {
            after_id: before_id,
            title,
            before: true,
            keys: vec![KeyChord::from_strokes(&["C-S-<return>"])],
        }
    }

    /// Placeholder.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            after_id: BlockId::from_existing(""),
            title: String::new(),
            before: false,
            keys: vec![KeyChord::from_strokes(&["M-<return>"])],
        }
    }
}

impl Command for AddSibling {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "add-sibling"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc
            .path_of(&self.after_id)
            .ok_or(CommandError::BlockNotFound)?;
        let new_id = BlockId::fresh();
        let rewrite = if self.before {
            rewrite_add_sibling_before_with_id
        } else {
            rewrite_add_sibling_after_with_id
        };
        let org = rewrite(doc.org(), &path, &self.title, new_id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::AddSibling {
            after_id: self.after_id.clone(),
            new_id,
            title: self.title.clone(),
        };
        doc.push_history(edit.clone());
        Ok(edit)
    }
}

/// Command: persist a fresh ULID into the headline's `:ID:` property.
/// Existing ids are never overwritten (I2).
pub struct EnsureId {
    id: BlockId,
    keys: Vec<KeyChord>,
}

impl EnsureId {
    /// Ensure the headline with the given (possibly in-memory) id has
    /// its id written to disk.
    #[must_use]
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "i"])],
        }
    }

    /// Placeholder for registry introspection.
    #[must_use]
    pub fn new_placeholder() -> Self {
        Self {
            id: BlockId::from_existing(""),
            keys: vec![KeyChord::from_strokes(&["C-c", "C-x", "i"])],
        }
    }
}

impl Command for EnsureId {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "ensure-id"
    }

    fn keys(&self) -> &[KeyChord] {
        &self.keys
    }

    fn apply(&self, doc: &mut Document) -> Result<Edit, CommandError> {
        let path = doc.path_of(&self.id).ok_or(CommandError::BlockNotFound)?;
        let org = rewrite_headline_ensure_id(doc.org(), &path, self.id.as_str())
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        // EnsureId is not a mutation the user should undo; no history entry.
        Ok(Edit::Noop)
    }
}

// === What was built ===

/// What the running binary was built from.
///
/// "build time git commit hash (and if from dirty working tree append
/// that too) … I don't want to have a timestamp when the executable
/// has been built, because that would break the reproducibility."
///
/// The reproducibility argument decides the shape. A timestamp is a
/// property of *when* you built; a commit is a property of *what* you
/// built — so two builds of one tree stay identical, which is what
/// nix's epoch mtimes protect, and the value still names the source
/// exactly. The dirty flag keeps that honest: a build from an edited
/// tree is not the commit it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// Short commit hash, or `None` when built without a git tree —
    /// a source tarball, which is ordinary and must not be a failure.
    pub commit: Option<&'static str>,
    /// How many commits lead to it. "the commit count is something I
    /// could make use of as well".
    pub commits: Option<u64>,
    /// Whether tracked files differed from that commit when this was
    /// compiled.
    ///
    /// Tracked only: untracked files were not compiled into anything,
    /// and calling a build dirty for an editor swap file beside it
    /// would make the flag mean nothing.
    pub dirty: bool,
}

impl BuildInfo {
    /// One line naming the build, for a status bar or a message log.
    ///
    /// Deliberately free of anything that changes between two builds
    /// of the same source.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut out = match (self.commit, self.commits) {
            (Some(c), Some(n)) => format!("{c} ({n} commits)"),
            (Some(c), None) => c.to_owned(),
            (None, Some(n)) => format!("unknown commit ({n} commits)"),
            (None, None) => "unknown commit".to_owned(),
        };
        if self.dirty {
            // Named, not punctuated: `-dirty` reads as part of a hash.
            out.push_str(" · dirty tree");
        }
        out
    }

    /// The build, plus the optional features `extra` names.
    ///
    /// The features are passed in rather than read here: cargo
    /// features are per crate, and this crate has none of its own, so
    /// anything it reported about the *binary* would be a confident
    /// "nothing". `closure-cli` owns the flags that vary and hands
    /// them over.
    #[must_use]
    pub fn describe_with(&self, extra: &[&str]) -> String {
        let mut out = self.describe();
        if !extra.is_empty() {
            out.push_str(" · features: ");
            out.push_str(&extra.join(", "));
        }
        out
    }
}

/// What this binary was built from ([`BuildInfo`]).
#[must_use]
pub const fn build_info() -> BuildInfo {
    BuildInfo {
        commit: option_env!("CLOSURE_GIT_COMMIT"),
        // `option_env!` is a `&str`; parsing it in a const fn needs a
        // match rather than `.parse()`.
        commits: match option_env!("CLOSURE_GIT_COMMITS") {
            Some(s) => parse_u64(s.as_bytes(), 0, 0),
            None => None,
        },
        dirty: option_env!("CLOSURE_GIT_DIRTY").is_some(),
    }
}

/// Decimal parse, const so [`build_info`] can stay one.
const fn parse_u64(bytes: &[u8], at: usize, acc: u64) -> Option<u64> {
    if at >= bytes.len() {
        return if at == 0 { None } else { Some(acc) };
    }
    let b = bytes[at];
    if b.is_ascii_digit() {
        parse_u64(bytes, at + 1, acc * 10 + (b - b'0') as u64)
    } else {
        None
    }
}
