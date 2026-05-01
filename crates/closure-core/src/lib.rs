//! Kernel document model: stable block IDs, command registry, keybinding
//! trie, event bus. Sits on top of [`closure_org`] and is UI-agnostic.
//!
//! This crate defines the frontend-agnostic API surface (spec invariant
//! I7): shells and adapters consume [`Document`], [`BlockId`], and the
//! command registry. They never reach into `closure-org` directly, and
//! they never see byte offsets / spans.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use closure_org::{
    Headline, OrgDoc, parse, rewrite_headline_ensure_id, rewrite_headline_set_priority,
    rewrite_headline_set_tags, rewrite_headline_set_todo, rewrite_headline_title,
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
    for n in h.body() {
        for l in closure_org::find_links(n.source()) {
            link_targets.push(l.target.to_owned());
        }
    }
    DocHeadline {
        id,
        path: path.to_vec(),
        title: h.title().to_owned(),
        level: h.level(),
        todo: h.todo().map(str::to_owned),
        priority: h.priority(),
        tags: h.tags().into_iter().map(str::to_owned).collect(),
        link_targets,
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
            Self::Noop => Ok(()),
        }
    }

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
            Self::Noop => Ok(()),
        }
    }
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
        let org = rewrite_headline_set_todo(doc.org(), &path, self.new.as_deref())
            .map_err(|_| CommandError::Rewrite)?;
        doc.org = org;
        doc.rebuild_index();
        let edit = Edit::SetTodo {
            id: self.id.clone(),
            old,
            new: self.new.clone(),
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
