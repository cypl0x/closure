//! Vault-level storage: directory loader, cross-file block-id index,
//! and a recursive file watcher.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use closure_core::{BlockId, Document};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

/// A loaded vault: every `*.org` file under a directory parsed into
/// [`Document`]s with a shared block-id index plus a precomputed
/// inverted backlink index.
#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
    documents: HashMap<PathBuf, Document>,
    by_id: HashMap<BlockId, PathBuf>,
    /// Inverted index: target id (or full URL) → set of (path, source-id)
    /// pairs whose headline links to it.
    backlinks: HashMap<String, Vec<(PathBuf, BlockId)>>,
}

/// Errors while operating on a vault.
#[derive(Debug, Error)]
pub enum VaultError {
    /// Filesystem error.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// A document failed to parse.
    #[error("parse error in {path}")]
    Parse {
        /// The file that failed.
        path: PathBuf,
    },
    /// Watcher subsystem error.
    #[error("watch: {0}")]
    Watch(String),
}

impl Vault {
    /// Reload the vault from disk: re-walks the root, re-parses every
    /// `*.org` file, and rebuilds the id and backlink indices.
    pub fn reload(&mut self) -> Result<(), VaultError> {
        let fresh = Self::open(&self.root)?;
        self.documents = fresh.documents;
        self.by_id = fresh.by_id;
        self.backlinks = fresh.backlinks;
        Ok(())
    }

    /// Open the vault at `root`, loading every `*.org` file underneath.
    pub fn open(root: &Path) -> Result<Self, VaultError> {
        let root = root.to_path_buf();
        let mut documents: HashMap<PathBuf, Document> = HashMap::new();
        let mut by_id: HashMap<BlockId, PathBuf> = HashMap::new();
        let mut backlinks: HashMap<String, Vec<(PathBuf, BlockId)>> = HashMap::new();
        walk_org(&root, &mut |path| {
            let src = fs::read_to_string(path)?;
            let doc =
                Document::load_str(&src).map_err(|_| VaultError::Parse { path: path.into() })?;
            for h in doc.all_headlines() {
                let id = h.id().clone();
                by_id.insert(id.clone(), path.to_path_buf());
                for target in h.link_targets() {
                    backlinks
                        .entry(target.clone())
                        .or_default()
                        .push((path.to_path_buf(), id.clone()));
                    // Also index `id:<ULID>` links by the bare id so
                    // callers can pass either form.
                    if let Some(stripped) = target.strip_prefix("id:") {
                        backlinks
                            .entry(stripped.to_owned())
                            .or_default()
                            .push((path.to_path_buf(), id.clone()));
                    }
                }
            }
            documents.insert(path.to_path_buf(), doc);
            Ok(())
        })?;
        Ok(Self {
            root,
            documents,
            by_id,
            backlinks,
        })
    }

    /// Inverted backlink lookup. Returns every `(file, source-id)`
    /// whose headline links to `target`. Both `id:<ULID>` and bare
    /// ULID forms are accepted.
    #[must_use]
    pub fn backlinks_of(&self, target: &str) -> &[(PathBuf, BlockId)] {
        static EMPTY: Vec<(PathBuf, BlockId)> = Vec::new();
        self.backlinks.get(target).map_or(&EMPTY, Vec::as_slice)
    }

    /// Root directory of the vault.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Number of `*.org` files loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Whether the vault has zero documents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// All loaded file paths in sorted order.
    #[must_use]
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.documents.keys().cloned().collect();
        v.sort();
        v
    }

    /// Iterate the stored file paths as borrowed references (no order).
    pub fn paths_iter(&self) -> impl Iterator<Item = &Path> {
        self.documents.keys().map(PathBuf::as_path)
    }

    /// Iterate `(path, document)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &Document)> {
        self.documents.iter().map(|(p, d)| (p.as_path(), d))
    }

    /// Every distinct tag string used across the vault, sorted.
    #[must_use]
    pub fn all_tags(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                for t in h.tags() {
                    seen.insert(t.clone());
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Total headline count across every document in the vault.
    #[must_use]
    pub fn headline_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.all_headlines().count())
            .sum()
    }

    /// Adjacency list: every (source-id → list of target-ids) link
    /// inside the vault, restricted to `id:` targets that resolve to a
    /// loaded headline. Useful for graph rendering / shortest-path
    /// search across notes.
    #[must_use]
    pub fn link_graph(&self) -> HashMap<BlockId, Vec<BlockId>> {
        let mut out: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                let src_id = h.id().clone();
                let mut targets: Vec<BlockId> = Vec::new();
                for raw in h.link_targets() {
                    let candidate = raw.strip_prefix("id:").unwrap_or(raw);
                    let bid = BlockId::from_existing(candidate);
                    if self.by_id.contains_key(&bid) {
                        targets.push(bid);
                    }
                }
                if !targets.is_empty() {
                    out.entry(src_id).or_default().extend(targets);
                }
            }
        }
        out
    }

    /// Total whitespace-separated word count across every source byte
    /// in the vault.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.source().split_whitespace().count())
            .sum()
    }

    /// Word count for a single file by path. Returns `None` if the
    /// file isn't loaded.
    #[must_use]
    pub fn word_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.source().split_whitespace().count())
    }

    /// Headline count for a single file by path.
    #[must_use]
    pub fn headline_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.all_headlines().count())
    }

    /// Source byte length for a single file by path.
    #[must_use]
    pub fn byte_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.source().len())
    }

    /// Total byte count across the vault.
    #[must_use]
    pub fn byte_count(&self) -> usize {
        self.documents.values().map(|d| d.source().len()).sum()
    }

    /// Total link count across every headline in the vault.
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_link_count()).sum()
    }

    /// Total timestamp count across every headline in the vault.
    #[must_use]
    pub fn timestamp_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_timestamp_count()).sum()
    }

    /// Total cookie count across the vault.
    #[must_use]
    pub fn cookie_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_cookie_count()).sum()
    }

    /// Total footnote count across the vault.
    #[must_use]
    pub fn footnote_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_footnote_count()).sum()
    }

    /// Total macro count across the vault.
    #[must_use]
    pub fn macro_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_macro_count()).sum()
    }

    /// Count of headlines with an `:ID:` property across the vault.
    #[must_use]
    pub fn id_count(&self) -> usize {
        self.documents.values().map(|d| d.org().count_with_id()).sum()
    }

    /// Count of TODO-marked headlines across the vault.
    #[must_use]
    pub fn todo_count(&self) -> usize {
        self.documents.values().map(|d| d.org().count_todos()).sum()
    }

    /// Count of archived headlines across the vault.
    #[must_use]
    pub fn archived_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_archived())
            .sum()
    }

    /// Count of COMMENT-prefixed headlines across the vault.
    #[must_use]
    pub fn comment_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_comments())
            .sum()
    }

    /// Count of headlines with `SCHEDULED:` across the vault.
    #[must_use]
    pub fn scheduled_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_scheduled())
            .sum()
    }

    /// Count of headlines with `DEADLINE:` across the vault.
    #[must_use]
    pub fn deadline_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_deadline())
            .sum()
    }

    /// Count of headlines with `CLOSED:` across the vault.
    #[must_use]
    pub fn closed_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_closed())
            .sum()
    }

    /// Mean body word count across the vault (rounded down).
    #[must_use]
    pub fn mean_body_word_count(&self) -> usize {
        let total: usize = self
            .documents
            .values()
            .map(|d| d.org().total_body_word_count())
            .sum();
        total.checked_div(self.headline_count()).unwrap_or(0)
    }

    /// Mean file byte count across the vault (rounded down).
    #[must_use]
    pub fn mean_byte_count(&self) -> usize {
        self.byte_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Mean headline count per file across the vault (rounded down).
    #[must_use]
    pub fn mean_headlines_per_file(&self) -> usize {
        self.headline_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Largest file (path + byte count) in the vault.
    #[must_use]
    pub fn largest_file(&self) -> Option<(&Path, usize)> {
        self.documents
            .iter()
            .map(|(p, d)| (p.as_path(), d.source().len()))
            .max_by_key(|(_, n)| *n)
    }

    /// File with the most headlines.
    #[must_use]
    pub fn busiest_file(&self) -> Option<(&Path, usize)> {
        self.documents
            .iter()
            .map(|(p, d)| (p.as_path(), d.all_headlines().count()))
            .max_by_key(|(_, n)| *n)
    }

    /// Smallest file by byte count.
    #[must_use]
    pub fn smallest_file(&self) -> Option<(&Path, usize)> {
        self.documents
            .iter()
            .map(|(p, d)| (p.as_path(), d.source().len()))
            .min_by_key(|(_, n)| *n)
    }

    /// Quietest file (fewest headlines).
    #[must_use]
    pub fn quietest_file(&self) -> Option<(&Path, usize)> {
        self.documents
            .iter()
            .map(|(p, d)| (p.as_path(), d.all_headlines().count()))
            .min_by_key(|(_, n)| *n)
    }

    /// Empty files (no headlines).
    #[must_use]
    pub fn empty_files(&self) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().count() == 0)
            .map(|(p, _)| p)
            .collect()
    }

    /// Number of empty files in the vault.
    #[must_use]
    pub fn empty_file_count(&self) -> usize {
        self.empty_files().len()
    }

    /// Map of `path → 64-bit FNV-1a content hash` for every loaded
    /// document. Useful for change-detection caches that need to know
    /// which files have shifted since a previous snapshot.
    #[must_use]
    pub fn source_hashes(&self) -> HashMap<PathBuf, u64> {
        self.documents
            .iter()
            .map(|(p, d)| (p.clone(), d.source_hash()))
            .collect()
    }

    /// Tag occurrence counts across the vault, sorted descending by count.
    #[must_use]
    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                for t in h.tags() {
                    *counts.entry(t.clone()).or_insert(0) += 1;
                }
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }

    /// TODO keyword occurrence counts, sorted descending by count.
    #[must_use]
    pub fn todo_counts(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                if let Some(t) = h.todo() {
                    *counts.entry(t.to_owned()).or_insert(0) += 1;
                }
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }

    /// Every distinct TODO keyword used across the vault, sorted.
    #[must_use]
    pub fn all_todos(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                if let Some(t) = h.todo() {
                    seen.insert(t.to_owned());
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Lookup a document by its full filesystem path.
    #[must_use]
    pub fn document(&self, path: &Path) -> Option<&Document> {
        self.documents.get(path)
    }

    /// Lookup a document by a path relative to the vault root.
    #[must_use]
    pub fn document_relative(&self, relative: &Path) -> Option<&Document> {
        self.documents.get(&self.root.join(relative))
    }

    /// First headline whose title equals `needle` (case-insensitive).
    /// Returns the matching headline and its containing file path.
    #[must_use]
    pub fn find_by_title(&self, needle: &str) -> Option<(&closure_core::DocHeadline, &Path)> {
        for (path, doc) in self.iter() {
            for h in doc.all_headlines() {
                if h.title().eq_ignore_ascii_case(needle) {
                    return Some((h, path));
                }
            }
        }
        None
    }

    /// True iff the vault contains a headline with the given id.
    #[must_use]
    pub fn has_id(&self, id: &BlockId) -> bool {
        self.by_id.contains_key(id)
    }

    /// Every distinct path containing at least one headline tagged
    /// :ARCHIVE:.
    #[must_use]
    pub fn archive_paths(&self) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.tags().iter().any(|t| t == "ARCHIVE")))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one TODO-marked headline.
    #[must_use]
    pub fn paths_with_todos(&self) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.todo().is_some()))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one headline with the given tag.
    #[must_use]
    pub fn paths_with_tag<'a>(&'a self, tag: &str) -> Vec<&'a Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.tags().iter().any(|t| t == tag)))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths whose source contains the substring `needle` anywhere.
    #[must_use]
    pub fn paths_containing(&self, needle: &str) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.source().contains(needle))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths whose source contains `needle` (case-insensitive).
    #[must_use]
    pub fn paths_containing_ignore_case(&self, needle: &str) -> Vec<&Path> {
        let lower = needle.to_lowercase();
        self.iter()
            .filter(|(_, d)| d.source().to_lowercase().contains(&lower))
            .map(|(p, _)| p)
            .collect()
    }

    /// Lookup a headline and its owning file by block id.
    #[must_use]
    pub fn find_by_id(&self, id: &BlockId) -> Option<(&closure_core::DocHeadline, &Path)> {
        let path = self.by_id.get(id)?;
        let doc = self.documents.get(path)?;
        let h = doc.headline_by_id(id)?;
        Some((h, path.as_path()))
    }

    /// Atomically write `source` to `path`. Writes to a sibling
    /// `<name>.tmp` first then renames.
    pub fn save(&self, path: &Path, source: &str) -> Result<(), VaultError> {
        let tmp = path.with_extension("org.tmp");
        fs::write(&tmp, source)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Atomically write with a `.bak` backup of the existing content.
    /// If `path` exists, it is copied to `<name>.org.bak` before the
    /// new write. The new write itself is atomic via the same
    /// tmp-then-rename dance as [`Self::save`].
    pub fn save_with_backup(&self, path: &Path, source: &str) -> Result<(), VaultError> {
        if path.exists() {
            let bak = path.with_extension("org.bak");
            fs::copy(path, &bak)?;
        }
        self.save(path, source)
    }

    /// Delete a `*.org` file from the vault. Removes the on-disk
    /// file plus its entries in the documents and `by_id` maps.
    /// Errors if the file isn't currently loaded.
    pub fn delete_file(&mut self, path: &Path) -> Result<(), VaultError> {
        if !self.documents.contains_key(path) {
            return Err(VaultError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            )));
        }
        fs::remove_file(path)?;
        self.documents.remove(path);
        self.by_id.retain(|_, p| p != path);
        Ok(())
    }

    /// Rename a file inside the vault. Updates documents and `by_id`
    /// maps to point at the new path.
    pub fn rename_file(&mut self, from: &Path, to_relative: &Path) -> Result<PathBuf, VaultError> {
        let to = self.root.join(to_relative);
        let doc = self.documents.remove(from).ok_or_else(|| {
            VaultError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                from.display().to_string(),
            ))
        })?;
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(from, &to)?;
        for path in self.by_id.values_mut() {
            if path == from {
                path.clone_from(&to);
            }
        }
        self.documents.insert(to.clone(), doc);
        Ok(to)
    }

    /// Create a new `*.org` file under `relative` (relative to the
    /// vault root) with `source`. Refuses to overwrite an existing
    /// file. Returns the absolute path.
    pub fn create_file(&mut self, relative: &Path, source: &str) -> Result<PathBuf, VaultError> {
        let path = self.root.join(relative);
        if path.exists() {
            return Err(VaultError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                path.display().to_string(),
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, source)?;
        let doc =
            Document::load_str(source).map_err(|_| VaultError::Parse { path: path.clone() })?;
        for id in doc.all_block_ids() {
            self.by_id.insert(id, path.clone());
        }
        self.documents.insert(path.clone(), doc);
        Ok(path)
    }

    /// Start a recursive file watcher rooted at the vault. Returns a
    /// [`VaultWatcher`] whose `recv` blocks for the next change event.
    /// The watcher must be kept alive — dropping it stops the inotify
    /// (or platform equivalent) listener.
    pub fn watch(&self) -> Result<VaultWatcher, VaultError> {
        let (tx, rx): (Sender<VaultEvent>, Receiver<VaultEvent>) = channel();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            for path in event.paths {
                if path.extension().and_then(|e| e.to_str()) != Some("org") {
                    continue;
                }
                let kind = match event.kind {
                    EventKind::Create(_) => VaultEventKind::Created,
                    EventKind::Modify(_) => VaultEventKind::Modified,
                    EventKind::Remove(_) => VaultEventKind::Removed,
                    _ => continue,
                };
                let _ = tx.send(VaultEvent { path, kind });
            }
        })
        .map_err(|e| VaultError::Watch(e.to_string()))?;
        let mut w = watcher;
        w.watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| VaultError::Watch(e.to_string()))?;
        Ok(VaultWatcher { _watcher: w, rx })
    }
}

/// Active file-watcher handle. Drop to stop watching.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<VaultEvent>,
}

impl VaultWatcher {
    /// Block for the next event.
    #[allow(clippy::missing_errors_doc)]
    pub fn recv(&self) -> Result<VaultEvent, VaultError> {
        self.rx.recv().map_err(|e| VaultError::Watch(e.to_string()))
    }

    /// Try to receive an event without blocking.
    #[must_use]
    pub fn try_recv(&self) -> Option<VaultEvent> {
        self.rx.try_recv().ok()
    }
}

/// A change event for an `*.org` file in the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEvent {
    /// Affected file.
    pub path: PathBuf,
    /// What happened.
    pub kind: VaultEventKind,
}

/// Coarse classification of a vault file event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultEventKind {
    /// File created.
    Created,
    /// File modified.
    Modified,
    /// File removed.
    Removed,
}

fn walk_org<F>(dir: &Path, f: &mut F) -> io::Result<()>
where
    F: FnMut(&Path) -> Result<(), VaultError>,
{
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk_org(&path, f)?;
        } else if ty.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("org")
            && let Err(e) = f(&path)
        {
            return Err(io::Error::other(e.to_string()));
        }
    }
    Ok(())
}
