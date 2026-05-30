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

    /// Integer mean file word count (`0` when empty).
    #[must_use]
    pub fn mean_file_word_count(&self) -> usize {
        self.word_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Maximum file word count across the vault.
    #[must_use]
    pub fn max_file_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().split_whitespace().count())
            .max()
    }

    /// Minimum file word count across the vault.
    #[must_use]
    pub fn min_file_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().split_whitespace().count())
            .min()
    }

    /// Median file word count across the vault (`None` when empty).
    #[must_use]
    pub fn median_file_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.source().split_whitespace().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Headline count for a single file by path.
    #[must_use]
    pub fn headline_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.all_headlines().count())
    }

    /// Maximum per-file headline count (`None` when no files).
    #[must_use]
    pub fn max_file_headline_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().count())
            .max()
    }

    /// Minimum per-file headline count (`None` when no files).
    #[must_use]
    pub fn min_file_headline_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().count())
            .min()
    }

    /// Integer mean per-file headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_headline_count(&self) -> usize {
        self.headline_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file headline count (`None` when no files).
    #[must_use]
    pub fn median_file_headline_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
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

    /// Source character count for a single file by path. Returns `None`
    /// if the file isn't loaded.
    #[must_use]
    pub fn char_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.source().chars().count())
    }

    /// Total source character count across the vault.
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.source().chars().count())
            .sum()
    }

    /// Integer mean file character count (`0` when no files).
    #[must_use]
    pub fn mean_file_char_count(&self) -> usize {
        self.char_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Maximum per-file character count (`None` when no files).
    #[must_use]
    pub fn max_file_char_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().chars().count())
            .max()
    }

    /// Minimum per-file character count (`None` when no files).
    #[must_use]
    pub fn min_file_char_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().chars().count())
            .min()
    }

    /// Median per-file character count (`None` when no files).
    #[must_use]
    pub fn median_file_char_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.source().chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Total headline-body byte count across the vault.
    #[must_use]
    pub fn total_body_byte_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().len())
            .sum()
    }

    /// Total headline-body byte count for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn body_byte_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().map(|h| h.body_text().len()).sum())
    }

    /// Maximum per-file body byte count (`None` when no files).
    #[must_use]
    pub fn max_file_body_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().len()).sum())
            .max()
    }

    /// Minimum per-file body byte count (`None` when no files).
    #[must_use]
    pub fn min_file_body_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().len()).sum())
            .min()
    }

    /// Integer mean per-file body byte count (`0` when no files).
    #[must_use]
    pub fn mean_file_body_byte_count(&self) -> usize {
        self.total_body_byte_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file body byte count (`None` when no files).
    #[must_use]
    pub fn median_file_body_byte_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().len()).sum())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Maximum per-headline body byte count across the vault.
    #[must_use]
    pub fn max_body_byte_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().len())
            .max()
    }

    /// Minimum per-headline body byte count across the vault.
    #[must_use]
    pub fn min_body_byte_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().len())
            .min()
    }

    /// Integer mean per-headline body byte count (`0` when no headlines).
    #[must_use]
    pub fn mean_body_byte_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_body_byte_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline body byte count (`None` when no headlines).
    #[must_use]
    pub fn median_body_byte_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline body byte counts to occurrence count.
    #[must_use]
    pub fn body_byte_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.body_text().len()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline body byte count (lowest wins ties).
    #[must_use]
    pub fn mode_body_byte_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.body_byte_count_counts() {
            if best.is_none_or(|(_, cc)| c > cc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Percentage of source bytes that are headline body
    /// (`total body bytes * 100 / source bytes`, `0` when empty).
    #[must_use]
    pub fn body_byte_pct(&self) -> usize {
        (self.total_body_byte_count() * 100)
            .checked_div(self.byte_count())
            .unwrap_or(0)
    }

    /// Percentage of source bytes that are headline titles
    /// (`total title bytes * 100 / source bytes`, `0` when empty).
    #[must_use]
    pub fn title_byte_pct(&self) -> usize {
        (self.total_title_byte_len() * 100)
            .checked_div(self.byte_count())
            .unwrap_or(0)
    }

    /// Source bytes that are not headline body (headers, drawers,
    /// preamble, blank lines). Saturating.
    #[must_use]
    pub fn non_body_byte_count(&self) -> usize {
        self.byte_count()
            .saturating_sub(self.total_body_byte_count())
    }

    /// Percentage of source bytes that are not headline body
    /// (`non-body bytes * 100 / source bytes`, `0` when empty).
    #[must_use]
    pub fn non_body_byte_pct(&self) -> usize {
        (self.non_body_byte_count() * 100)
            .checked_div(self.byte_count())
            .unwrap_or(0)
    }

    /// Total link count across every headline in the vault.
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_link_count()).sum()
    }

    /// Total link count for a single file by path. Returns `None` if the
    /// file isn't loaded.
    #[must_use]
    pub fn link_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().total_link_count())
    }

    /// Maximum per-file total link count (`None` when no files).
    #[must_use]
    pub fn max_file_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_link_count())
            .max()
    }

    /// Minimum per-file total link count (`None` when no files).
    #[must_use]
    pub fn min_file_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_link_count())
            .min()
    }

    /// Integer mean per-file total link count (`0` when no files).
    #[must_use]
    pub fn mean_file_link_count(&self) -> usize {
        self.link_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file total link count (`None` when no files).
    #[must_use]
    pub fn median_file_link_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().total_link_count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Total timestamp count across every headline in the vault.
    #[must_use]
    pub fn timestamp_count(&self) -> usize {
        self.documents.values().map(|d| d.org().total_timestamp_count()).sum()
    }

    /// Total timestamp count for a single file by path. Returns `None`
    /// if the file isn't loaded.
    #[must_use]
    pub fn timestamp_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().total_timestamp_count())
    }

    /// Maximum per-file total timestamp count (`None` when no files).
    #[must_use]
    pub fn max_file_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_timestamp_count())
            .max()
    }

    /// Minimum per-file total timestamp count (`None` when no files).
    #[must_use]
    pub fn min_file_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_timestamp_count())
            .min()
    }

    /// Integer mean per-file total timestamp count (`0` when no files).
    #[must_use]
    pub fn mean_file_timestamp_count(&self) -> usize {
        self.timestamp_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file total timestamp count (`None` when no files).
    #[must_use]
    pub fn median_file_timestamp_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().total_timestamp_count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
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

    /// Count of headlines with an `:ID:` property for a single file by
    /// path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn id_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().count_with_id())
    }

    /// Maximum per-file `:ID:` count (`None` when no files).
    #[must_use]
    pub fn max_file_id_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_id())
            .max()
    }

    /// Minimum per-file `:ID:` count (`None` when no files).
    #[must_use]
    pub fn min_file_id_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_id())
            .min()
    }

    /// Integer mean per-file `:ID:` count (`0` when no files).
    #[must_use]
    pub fn mean_file_id_count(&self) -> usize {
        self.id_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file `:ID:` count (`None` when no files).
    #[must_use]
    pub fn median_file_id_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_with_id())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying an `:ID:` property (`0..=100`).
    #[must_use]
    pub fn id_pct(&self) -> usize {
        (self.id_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with no `:ID:` property.
    #[must_use]
    pub fn count_no_id(&self) -> usize {
        self.headline_count() - self.id_count()
    }

    /// Percentage of headlines with no `:ID:` property (`0..=100`).
    #[must_use]
    pub fn no_id_pct(&self) -> usize {
        (self.count_no_id() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of TODO-marked headlines across the vault.
    #[must_use]
    pub fn todo_count(&self) -> usize {
        self.documents.values().map(|d| d.org().count_todos()).sum()
    }

    /// Percentage of headlines carrying a TODO keyword (`0..=100`).
    #[must_use]
    pub fn todo_pct(&self) -> usize {
        (self.todo_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with no TODO keyword.
    #[must_use]
    pub fn count_no_todo(&self) -> usize {
        self.headline_count() - self.todo_count()
    }

    /// Percentage of headlines with no TODO keyword (`0..=100`).
    #[must_use]
    pub fn no_todo_pct(&self) -> usize {
        (self.count_no_todo() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines carrying at least one tag across the vault.
    #[must_use]
    pub fn tagged_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| !h.tags().is_empty())
            .count()
    }

    /// Percentage of headlines carrying at least one tag (`0..=100`).
    #[must_use]
    pub fn tagged_pct(&self) -> usize {
        (self.tagged_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines carrying no tag.
    #[must_use]
    pub fn untagged_count(&self) -> usize {
        self.headline_count() - self.tagged_count()
    }

    /// Percentage of headlines carrying no tag (`0..=100`).
    #[must_use]
    pub fn untagged_pct(&self) -> usize {
        (self.untagged_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of archived headlines across the vault.
    #[must_use]
    pub fn archived_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_archived())
            .sum()
    }

    /// Count of archived headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn archived_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().count_archived())
    }

    /// Maximum per-file archived count (`None` when no files).
    #[must_use]
    pub fn max_file_archived_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_archived())
            .max()
    }

    /// Minimum per-file archived count (`None` when no files).
    #[must_use]
    pub fn min_file_archived_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_archived())
            .min()
    }

    /// Integer mean per-file archived count (`0` when no files).
    #[must_use]
    pub fn mean_file_archived_count(&self) -> usize {
        self.archived_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file archived count (`None` when no files).
    #[must_use]
    pub fn median_file_archived_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_archived())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Count of COMMENT-prefixed headlines across the vault.
    #[must_use]
    pub fn comment_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_comments())
            .sum()
    }

    /// Count of COMMENT-prefixed headlines for a single file by path.
    /// Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn comment_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().count_comments())
    }

    /// Maximum per-file COMMENT count (`None` when no files).
    #[must_use]
    pub fn max_file_comment_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_comments())
            .max()
    }

    /// Minimum per-file COMMENT count (`None` when no files).
    #[must_use]
    pub fn min_file_comment_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_comments())
            .min()
    }

    /// Integer mean per-file COMMENT count (`0` when no files).
    #[must_use]
    pub fn mean_file_comment_count(&self) -> usize {
        self.comment_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file COMMENT count (`None` when no files).
    #[must_use]
    pub fn median_file_comment_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_comments())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines that are archived (`0..=100`).
    #[must_use]
    pub fn archived_pct(&self) -> usize {
        (self.archived_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines that are COMMENT (`0..=100`).
    #[must_use]
    pub fn comment_pct(&self) -> usize {
        (self.comment_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines that are not archived.
    #[must_use]
    pub fn count_non_archived(&self) -> usize {
        self.headline_count() - self.archived_count()
    }

    /// Percentage of headlines that are not archived (`0..=100`).
    #[must_use]
    pub fn non_archived_pct(&self) -> usize {
        (self.count_non_archived() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines that are not COMMENT.
    #[must_use]
    pub fn count_non_comment(&self) -> usize {
        self.headline_count() - self.comment_count()
    }

    /// Percentage of headlines that are not COMMENT (`0..=100`).
    #[must_use]
    pub fn non_comment_pct(&self) -> usize {
        (self.count_non_comment() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with `SCHEDULED:` across the vault.
    #[must_use]
    pub fn scheduled_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_scheduled())
            .sum()
    }

    /// Count of SCHEDULED headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn scheduled_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().count_scheduled())
    }

    /// Maximum per-file SCHEDULED count (`None` when no files).
    #[must_use]
    pub fn max_file_scheduled_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_scheduled())
            .max()
    }

    /// Minimum per-file SCHEDULED count (`None` when no files).
    #[must_use]
    pub fn min_file_scheduled_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_scheduled())
            .min()
    }

    /// Integer mean per-file SCHEDULED count (`0` when no files).
    #[must_use]
    pub fn mean_file_scheduled_count(&self) -> usize {
        self.scheduled_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file SCHEDULED count (`None` when no files).
    #[must_use]
    pub fn median_file_scheduled_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_scheduled())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Count of headlines with `DEADLINE:` across the vault.
    #[must_use]
    pub fn deadline_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_deadline())
            .sum()
    }

    /// Count of DEADLINE headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn deadline_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().count_with_deadline())
    }

    /// Maximum per-file DEADLINE count (`None` when no files).
    #[must_use]
    pub fn max_file_deadline_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_deadline())
            .max()
    }

    /// Minimum per-file DEADLINE count (`None` when no files).
    #[must_use]
    pub fn min_file_deadline_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_deadline())
            .min()
    }

    /// Integer mean per-file DEADLINE count (`0` when no files).
    #[must_use]
    pub fn mean_file_deadline_count(&self) -> usize {
        self.deadline_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file DEADLINE count (`None` when no files).
    #[must_use]
    pub fn median_file_deadline_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_with_deadline())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines that are SCHEDULED (`0..=100`).
    #[must_use]
    pub fn scheduled_pct(&self) -> usize {
        (self.scheduled_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines carrying a DEADLINE (`0..=100`).
    #[must_use]
    pub fn deadline_pct(&self) -> usize {
        (self.deadline_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines carrying a `[#X]` priority cookie.
    #[must_use]
    pub fn with_priority_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.priority().is_some())
            .count()
    }

    /// Count of prioritized headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn with_priority_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| h.priority().is_some()).count())
    }

    /// Maximum per-file prioritized-headline count (`None` when no files).
    #[must_use]
    pub fn max_file_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.priority().is_some()).count())
            .max()
    }

    /// Minimum per-file prioritized-headline count (`None` when no files).
    #[must_use]
    pub fn min_file_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.priority().is_some()).count())
            .min()
    }

    /// Integer mean per-file prioritized-headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_priority_count(&self) -> usize {
        self.with_priority_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file prioritized-headline count (`None` when no files).
    #[must_use]
    pub fn median_file_priority_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.priority().is_some()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying a priority cookie (`0..=100`).
    #[must_use]
    pub fn priority_pct(&self) -> usize {
        (self.with_priority_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines lacking a `[#X]` priority cookie.
    #[must_use]
    pub fn count_no_priority(&self) -> usize {
        self.headline_count() - self.with_priority_count()
    }

    /// Percentage of headlines lacking a priority cookie (`0..=100`).
    #[must_use]
    pub fn no_priority_pct(&self) -> usize {
        (self.count_no_priority() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines that are not SCHEDULED.
    #[must_use]
    pub fn count_unscheduled(&self) -> usize {
        self.headline_count() - self.scheduled_count()
    }

    /// Percentage of headlines that are not SCHEDULED (`0..=100`).
    #[must_use]
    pub fn unscheduled_pct(&self) -> usize {
        (self.count_unscheduled() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines that carry no DEADLINE.
    #[must_use]
    pub fn count_no_deadline(&self) -> usize {
        self.headline_count() - self.deadline_count()
    }

    /// Percentage of headlines that carry no DEADLINE (`0..=100`).
    #[must_use]
    pub fn no_deadline_pct(&self) -> usize {
        (self.count_no_deadline() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with `CLOSED:` across the vault.
    #[must_use]
    pub fn closed_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_closed())
            .sum()
    }

    /// Count of CLOSED headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn closed_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().count_closed())
    }

    /// Maximum per-file CLOSED count (`None` when no files).
    #[must_use]
    pub fn max_file_closed_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_closed())
            .max()
    }

    /// Minimum per-file CLOSED count (`None` when no files).
    #[must_use]
    pub fn min_file_closed_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_closed())
            .min()
    }

    /// Integer mean per-file CLOSED count (`0` when no files).
    #[must_use]
    pub fn mean_file_closed_count(&self) -> usize {
        self.closed_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file CLOSED count (`None` when no files).
    #[must_use]
    pub fn median_file_closed_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_closed())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying a `CLOSED:` stamp (`0..=100`).
    #[must_use]
    pub fn closed_pct(&self) -> usize {
        (self.closed_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines lacking a `CLOSED:` stamp.
    #[must_use]
    pub fn count_unclosed(&self) -> usize {
        self.headline_count() - self.closed_count()
    }

    /// Percentage of headlines lacking a `CLOSED:` stamp (`0..=100`).
    #[must_use]
    pub fn unclosed_pct(&self) -> usize {
        (self.count_unclosed() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
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

    /// Total headline-body word count across the vault.
    #[must_use]
    pub fn total_body_word_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().split_whitespace().count())
            .sum()
    }

    /// Total headline-body word count for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn body_word_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .map(|h| h.body_text().split_whitespace().count())
                .sum()
        })
    }

    /// Maximum per-file body word count (`None` when no files).
    #[must_use]
    pub fn max_file_body_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().split_whitespace().count())
                    .sum()
            })
            .max()
    }

    /// Minimum per-file body word count (`None` when no files).
    #[must_use]
    pub fn min_file_body_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().split_whitespace().count())
                    .sum()
            })
            .min()
    }

    /// Integer mean per-file body word count (`0` when no files).
    #[must_use]
    pub fn mean_file_body_word_count(&self) -> usize {
        self.total_body_word_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file body word count (`None` when no files).
    #[must_use]
    pub fn median_file_body_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().split_whitespace().count())
                    .sum()
            })
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Maximum headline-body word count across the vault.
    #[must_use]
    pub fn max_body_word_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().split_whitespace().count())
            .max()
    }

    /// Minimum headline-body word count across the vault.
    #[must_use]
    pub fn min_body_word_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().split_whitespace().count())
            .min()
    }

    /// Median headline-body word count (`None` when no headlines).
    #[must_use]
    pub fn median_body_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().split_whitespace().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of headline-body word counts to occurrence count.
    #[must_use]
    pub fn body_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.body_text().split_whitespace().count())
                    .or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common headline-body word count (lowest wins ties).
    #[must_use]
    pub fn mode_body_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.body_word_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Mean file byte count across the vault (rounded down).
    #[must_use]
    pub fn mean_byte_count(&self) -> usize {
        self.byte_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Maximum file byte count across the vault.
    #[must_use]
    pub fn max_file_byte_count(&self) -> Option<usize> {
        self.documents.values().map(|d| d.source().len()).max()
    }

    /// Minimum file byte count across the vault.
    #[must_use]
    pub fn min_file_byte_count(&self) -> Option<usize> {
        self.documents.values().map(|d| d.source().len()).min()
    }

    /// Integer mean file byte count (`0` when no files).
    #[must_use]
    pub fn mean_file_byte_count(&self) -> usize {
        self.byte_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median file byte count across the vault (`None` when empty).
    #[must_use]
    pub fn median_file_byte_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.documents.values().map(|d| d.source().len()).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Total line count across every file in the vault.
    #[must_use]
    pub fn total_line_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.source().lines().count())
            .sum()
    }

    /// Integer mean file line count (`0` when empty).
    #[must_use]
    pub fn mean_file_line_count(&self) -> usize {
        self.total_line_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Maximum file line count across the vault.
    #[must_use]
    pub fn max_file_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().lines().count())
            .max()
    }

    /// Minimum file line count across the vault.
    #[must_use]
    pub fn min_file_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.source().lines().count())
            .min()
    }

    /// Median file line count across the vault (`None` when empty).
    #[must_use]
    pub fn median_file_line_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.source().lines().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
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

    /// All `(path, headline_count)` pairs sorted descending by count.
    #[must_use]
    pub fn files_by_headline_count(&self) -> Vec<(&Path, usize)> {
        let mut pairs: Vec<(&Path, usize)> = self
            .iter()
            .map(|(p, d)| (p, d.all_headlines().count()))
            .collect();
        pairs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        pairs
    }

    /// All `(path, byte_count)` pairs sorted descending.
    #[must_use]
    pub fn files_by_byte_count(&self) -> Vec<(&Path, usize)> {
        let mut pairs: Vec<(&Path, usize)> = self
            .iter()
            .map(|(p, d)| (p, d.source().len()))
            .collect();
        pairs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        pairs
    }

    /// All `(path, todo_count)` pairs sorted descending.
    #[must_use]
    pub fn files_by_todo_count(&self) -> Vec<(&Path, usize)> {
        let mut pairs: Vec<(&Path, usize)> = self
            .iter()
            .map(|(p, d)| (p, d.org().count_todos()))
            .collect();
        pairs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        pairs
    }

    /// All `(path, link_count)` pairs sorted descending.
    #[must_use]
    pub fn files_by_link_count(&self) -> Vec<(&Path, usize)> {
        let mut pairs: Vec<(&Path, usize)> = self
            .iter()
            .map(|(p, d)| (p, d.org().total_link_count()))
            .collect();
        pairs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        pairs
    }

    /// All `(path, word_count)` pairs sorted descending.
    #[must_use]
    pub fn files_by_word_count(&self) -> Vec<(&Path, usize)> {
        let mut pairs: Vec<(&Path, usize)> = self
            .iter()
            .map(|(p, d)| (p, d.source().split_whitespace().count()))
            .collect();
        pairs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        pairs
    }

    /// All `(source_id, target_id)` edges across vault.
    #[must_use]
    pub fn id_edges(&self) -> Vec<(String, String)> {
        self.documents
            .values()
            .flat_map(|d| d.org().id_edges())
            .collect()
    }

    /// Total `id:` edge count across the vault.
    #[must_use]
    pub fn id_edge_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().id_edge_count())
            .sum()
    }

    /// Self-loop edge count across the vault.
    #[must_use]
    pub fn self_loop_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().self_loop_count())
            .sum()
    }

    /// Resolved-edge count across vault (target lives in same doc).
    #[must_use]
    pub fn resolved_edge_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().resolved_edge_count())
            .sum()
    }

    /// Number of duplicate ids across the vault.
    #[must_use]
    pub fn duplicate_id_count(&self) -> usize {
        self.duplicate_ids().len()
    }

    /// True iff any drawer id appears more than once across the vault.
    #[must_use]
    pub fn has_duplicate_ids(&self) -> bool {
        self.duplicate_id_count() > 0
    }

    /// Percentage of `:ID:`-carrying headlines whose id is distinct
    /// (`distinct ids * 100 / total ids`, `0` when no ids).
    #[must_use]
    pub fn id_uniqueness_pct(&self) -> usize {
        let total: usize = self
            .iter()
            .map(|(_, d)| d.org().all_ids().len())
            .sum();
        (self.unique_id_count() * 100)
            .checked_div(total)
            .unwrap_or(0)
    }

    /// Total number of distinct drawer ids across the vault.
    #[must_use]
    pub fn unique_id_count(&self) -> usize {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for id in doc.org().all_ids() {
                s.insert(id.to_owned());
            }
        }
        s.len()
    }

    /// Returns drawer ids appearing more than once across the vault.
    #[must_use]
    pub fn duplicate_ids(&self) -> Vec<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for id in doc.org().all_ids() {
                *counts.entry(id.to_owned()).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(k, _)| k)
            .collect()
    }

    /// Cross-vault resolved-edge count: edges whose target resolves
    /// somewhere in the vault (possibly different file).
    #[must_use]
    pub fn cross_resolved_edge_count(&self) -> usize {
        let mut count = 0usize;
        for (_, doc) in self.iter() {
            for (_, t) in doc.org().id_edges() {
                if self.has_id(&BlockId::from_existing(&t)) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Most-referenced id across the vault and its incoming count.
    #[must_use]
    pub fn most_referenced(&self) -> Option<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                for raw in h.link_targets() {
                    let Some(stripped) = raw.strip_prefix("id:") else {
                        continue;
                    };
                    *counts.entry(stripped.to_owned()).or_insert(0) += 1;
                }
            }
        }
        counts.into_iter().max_by_key(|(_, n)| *n)
    }

    /// Total dead-link count across the vault (id: targets that don't
    /// resolve to any vault headline).
    #[must_use]
    pub fn dead_link_count(&self) -> usize {
        let mut count = 0usize;
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                for raw in h.link_targets() {
                    let Some(stripped) = raw.strip_prefix("id:") else {
                        continue;
                    };
                    if !self.has_id(&BlockId::from_existing(stripped)) {
                        count += 1;
                    }
                }
            }
        }
        count
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

    /// Distinct tag list across the vault, sorted.
    #[must_use]
    pub fn distinct_tags(&self) -> Vec<String> {
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

    /// Distinct TODO keyword list across the vault, sorted.
    /// Alias for [`Self::all_todos`].
    #[must_use]
    pub fn distinct_todos(&self) -> Vec<String> {
        self.all_todos()
    }

    /// Distinct priority letter list across the vault, sorted.
    #[must_use]
    pub fn distinct_priorities(&self) -> Vec<char> {
        let mut seen: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                if let Some(p) = h.priority() {
                    seen.insert(p);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Distinct level list across the vault, sorted ascending.
    #[must_use]
    pub fn distinct_levels(&self) -> Vec<u8> {
        let mut seen: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                seen.insert(h.level());
            }
        }
        seen.into_iter().collect()
    }

    /// Count of distinct tags across the vault.
    #[must_use]
    pub fn distinct_tag_count(&self) -> usize {
        self.distinct_tags().len()
    }

    /// Count of distinct TODO keywords across the vault.
    #[must_use]
    pub fn distinct_todo_count(&self) -> usize {
        self.distinct_todos().len()
    }

    /// Count of distinct priority letters across the vault.
    #[must_use]
    pub fn distinct_priority_count(&self) -> usize {
        self.distinct_priorities().len()
    }

    /// Count of distinct levels across the vault.
    #[must_use]
    pub fn distinct_level_count(&self) -> usize {
        self.distinct_levels().len()
    }

    /// Priority letter occurrence counts, sorted descending by count.
    #[must_use]
    pub fn priority_counts(&self) -> Vec<(char, usize)> {
        let mut counts: HashMap<char, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                if let Some(p) = h.priority() {
                    *counts.entry(p).or_insert(0) += 1;
                }
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }

    /// Lexicographically smallest priority letter (highest urgency), or
    /// `None` when no prioritized headline exists.
    #[must_use]
    pub fn min_priority(&self) -> Option<char> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::priority)
            .min()
    }

    /// Lexicographically largest priority letter (lowest urgency), or
    /// `None` when no prioritized headline exists.
    #[must_use]
    pub fn max_priority(&self) -> Option<char> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::priority)
            .max()
    }

    /// Most common priority letter (lowest letter wins ties), or `None`
    /// when no prioritized headline exists.
    #[must_use]
    pub fn mode_priority(&self) -> Option<char> {
        let mut m: std::collections::BTreeMap<char, usize> = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                if let Some(p) = h.priority() {
                    *m.entry(p).or_insert(0) += 1;
                }
            }
        }
        let mut best: Option<(char, usize)> = None;
        for (p, c) in m {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((p, c));
            }
        }
        best.map(|(p, _)| p)
    }

    /// Level occurrence counts, sorted descending by count.
    #[must_use]
    pub fn level_counts(&self) -> Vec<(u8, usize)> {
        let mut counts: HashMap<u8, usize> = HashMap::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                *counts.entry(h.level()).or_insert(0) += 1;
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }

    /// Most-common tag across the vault. Ties broken by lexicographic order.
    #[must_use]
    pub fn most_common_tag(&self) -> Option<String> {
        self.tag_counts().into_iter().next().map(|(k, _)| k)
    }

    /// Least-common tag across the vault (lowest count; ties by name asc).
    #[must_use]
    pub fn least_common_tag(&self) -> Option<String> {
        self.tag_counts()
            .into_iter()
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// Tag occurrence counts as a sorted map.
    #[must_use]
    pub fn tag_count_map(&self) -> std::collections::BTreeMap<String, usize> {
        self.tag_counts().into_iter().collect()
    }

    /// Most-common TODO keyword across the vault.
    #[must_use]
    pub fn most_common_todo(&self) -> Option<String> {
        self.todo_counts().into_iter().next().map(|(k, _)| k)
    }

    /// Most-common priority letter across the vault.
    #[must_use]
    pub fn most_common_priority(&self) -> Option<char> {
        self.priority_counts().into_iter().next().map(|(k, _)| k)
    }

    /// Least-common TODO keyword (lowest count; ties by name asc).
    #[must_use]
    pub fn least_common_todo(&self) -> Option<String> {
        self.todo_counts()
            .into_iter()
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// Least-common priority letter (lowest count; ties by letter asc).
    #[must_use]
    pub fn least_common_priority(&self) -> Option<char> {
        self.priority_counts()
            .into_iter()
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// TODO keyword occurrence counts as a sorted map.
    #[must_use]
    pub fn todo_count_map(&self) -> std::collections::BTreeMap<String, usize> {
        self.todo_counts().into_iter().collect()
    }

    /// Priority occurrence counts as a sorted map.
    #[must_use]
    pub fn priority_count_map(&self) -> std::collections::BTreeMap<char, usize> {
        self.priority_counts().into_iter().collect()
    }

    /// Most-common level across the vault.
    #[must_use]
    pub fn most_common_level(&self) -> Option<u8> {
        self.level_counts().into_iter().next().map(|(k, _)| k)
    }

    /// Maximum headline level across the vault.
    #[must_use]
    pub fn max_level(&self) -> Option<u8> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(closure_core::DocHeadline::level)
            .max()
    }

    /// Minimum headline level across the vault.
    #[must_use]
    pub fn min_level(&self) -> Option<u8> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(closure_core::DocHeadline::level)
            .min()
    }

    /// Sum of headline levels across the vault.
    #[must_use]
    pub fn total_level(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.level() as usize)
            .sum()
    }

    /// Integer mean headline level (`0` when no headlines).
    #[must_use]
    pub fn mean_level(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_level().checked_div(n).unwrap_or(0)
    }

    /// Median headline level (`None` when no headlines).
    #[must_use]
    pub fn median_level(&self) -> Option<u8> {
        let mut v: Vec<u8> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(closure_core::DocHeadline::level)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Most common headline level (lowest wins ties; `None` when empty).
    #[must_use]
    pub fn mode_level(&self) -> Option<u8> {
        let mut counts: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *counts.entry(h.level()).or_insert(0) += 1;
            }
        }
        let mut best: Option<(u8, usize)> = None;
        for (lvl, c) in counts {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((lvl, c));
            }
        }
        best.map(|(lvl, _)| lvl)
    }

    /// `(min, max)` headline level across the vault (`None` when empty).
    #[must_use]
    pub fn level_range(&self) -> Option<(u8, u8)> {
        Some((self.min_level()?, self.max_level()?))
    }

    /// Total root headline count across all documents in the vault.
    #[must_use]
    pub fn root_count(&self) -> usize {
        self.documents.values().map(|d| d.org().roots().len()).sum()
    }

    /// Root headline count for a single file by path. Returns `None` if
    /// the file isn't loaded.
    #[must_use]
    pub fn root_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().roots().len())
    }

    /// Maximum per-file root count (`None` when no files).
    #[must_use]
    pub fn max_file_root_count(&self) -> Option<usize> {
        self.documents.values().map(|d| d.org().roots().len()).max()
    }

    /// Minimum per-file root count (`None` when no files).
    #[must_use]
    pub fn min_file_root_count(&self) -> Option<usize> {
        self.documents.values().map(|d| d.org().roots().len()).min()
    }

    /// Integer mean per-file root count (`0` when no files).
    #[must_use]
    pub fn mean_file_root_count(&self) -> usize {
        self.root_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file root count (`None` when no files).
    #[must_use]
    pub fn median_file_root_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().roots().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Count of leaf headlines (no children) across the vault.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .filter(|h| h.is_leaf())
            .count()
    }

    /// Count of leaf headlines for a single file by path. Returns `None`
    /// if the file isn't loaded.
    #[must_use]
    pub fn leaf_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| h.is_leaf()).count())
    }

    /// Maximum per-file leaf count (`None` when no files).
    #[must_use]
    pub fn max_file_leaf_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| h.is_leaf()).count())
            .max()
    }

    /// Minimum per-file leaf count (`None` when no files).
    #[must_use]
    pub fn min_file_leaf_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| h.is_leaf()).count())
            .min()
    }

    /// Integer mean per-file leaf count (`0` when no files).
    #[must_use]
    pub fn mean_file_leaf_count(&self) -> usize {
        self.leaf_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file leaf count (`None` when no files).
    #[must_use]
    pub fn median_file_leaf_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| h.is_leaf()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines that are leaves (`0..=100`).
    #[must_use]
    pub fn leaf_pct(&self) -> usize {
        (self.leaf_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of branch headlines (at least one child) across the vault.
    #[must_use]
    pub fn branch_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .filter(|h| !h.is_leaf())
            .count()
    }

    /// Count of branch headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn branch_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| !h.is_leaf()).count())
    }

    /// Maximum per-file branch count (`None` when no files).
    #[must_use]
    pub fn max_file_branch_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| !h.is_leaf()).count())
            .max()
    }

    /// Minimum per-file branch count (`None` when no files).
    #[must_use]
    pub fn min_file_branch_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| !h.is_leaf()).count())
            .min()
    }

    /// Integer mean per-file branch count (`0` when no files).
    #[must_use]
    pub fn mean_file_branch_count(&self) -> usize {
        self.branch_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file branch count (`None` when no files).
    #[must_use]
    pub fn median_file_branch_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().into_iter().filter(|h| !h.is_leaf()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines that are branches (`0..=100`).
    #[must_use]
    pub fn branch_pct(&self) -> usize {
        (self.branch_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
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

    /// Paths containing at least one headline with the given priority letter.
    #[must_use]
    pub fn paths_with_priority(&self, letter: char) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.priority() == Some(letter)))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one headline at the given level.
    #[must_use]
    pub fn paths_at_level(&self, level: u8) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.level() == level))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one headline with TODO keyword `kw`.
    #[must_use]
    pub fn paths_with_todo<'a>(&'a self, kw: &str) -> Vec<&'a Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.todo() == Some(kw)))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one headline carrying an `:ID:` property.
    #[must_use]
    pub fn paths_with_id(&self) -> Vec<&Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.property("ID").is_some()))
            .map(|(p, _)| p)
            .collect()
    }

    /// Paths containing at least one headline carrying property `key`.
    #[must_use]
    pub fn paths_with_property<'a>(&'a self, key: &str) -> Vec<&'a Path> {
        self.iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.property(key).is_some()))
            .map(|(p, _)| p)
            .collect()
    }

    /// Number of paths containing at least one headline with the given tag.
    #[must_use]
    pub fn path_count_with_tag(&self, tag: &str) -> usize {
        self.paths_with_tag(tag).len()
    }

    /// Number of paths containing at least one headline with TODO keyword `kw`.
    #[must_use]
    pub fn path_count_with_todo(&self, kw: &str) -> usize {
        self.paths_with_todo(kw).len()
    }

    /// Number of paths containing at least one headline with priority `letter`.
    #[must_use]
    pub fn path_count_with_priority(&self, letter: char) -> usize {
        self.paths_with_priority(letter).len()
    }

    /// Number of paths containing at least one headline at exactly `level`.
    #[must_use]
    pub fn path_count_at_level(&self, level: u8) -> usize {
        self.paths_at_level(level).len()
    }

    /// Number of paths containing at least one headline carrying an `:ID:`.
    #[must_use]
    pub fn path_count_with_id(&self) -> usize {
        self.paths_with_id().len()
    }

    /// Number of paths containing at least one headline carrying property `key`.
    #[must_use]
    pub fn path_count_with_property(&self, key: &str) -> usize {
        self.paths_with_property(key).len()
    }

    /// Total tag occurrences across the vault (counting duplicates).
    #[must_use]
    pub fn total_tag_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.tags().len())
            .sum()
    }

    /// Total tag occurrences for a single file by path. Returns `None`
    /// if the file isn't loaded.
    #[must_use]
    pub fn tag_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().map(|h| h.tags().len()).sum())
    }

    /// Maximum per-file tag occurrence count (`None` when no files).
    #[must_use]
    pub fn max_file_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.tags().len()).sum())
            .max()
    }

    /// Minimum per-file tag occurrence count (`None` when no files).
    #[must_use]
    pub fn min_file_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.tags().len()).sum())
            .min()
    }

    /// Integer mean per-file tag occurrence count (`0` when no files).
    #[must_use]
    pub fn mean_file_tag_count(&self) -> usize {
        self.total_tag_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file tag occurrence count (`None` when no files).
    #[must_use]
    pub fn median_file_tag_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.tags().len()).sum())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Maximum tag length in characters across the vault.
    #[must_use]
    pub fn max_tag_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(closure_core::DocHeadline::tags)
            .map(|t| t.chars().count())
            .max()
    }

    /// Minimum tag length in characters across the vault.
    #[must_use]
    pub fn min_tag_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(closure_core::DocHeadline::tags)
            .map(|t| t.chars().count())
            .min()
    }

    /// Total tag length in characters across the vault.
    #[must_use]
    pub fn total_tag_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(closure_core::DocHeadline::tags)
            .map(|t| t.chars().count())
            .sum()
    }

    /// Integer mean tag length in characters (`0` when no tags).
    #[must_use]
    pub fn mean_tag_len(&self) -> usize {
        self.total_tag_len()
            .checked_div(self.total_tag_count())
            .unwrap_or(0)
    }

    /// Median tag length in characters (`None` when no tags).
    #[must_use]
    pub fn median_tag_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(closure_core::DocHeadline::tags)
            .map(|t| t.chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of tag lengths (chars) to occurrence count.
    #[must_use]
    pub fn tag_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for t in h.tags() {
                    *m.entry(t.chars().count()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most common tag length (lowest wins ties).
    #[must_use]
    pub fn mode_tag_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.tag_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Maximum TODO keyword length in characters across the vault.
    #[must_use]
    pub fn max_todo_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::todo)
            .map(|t| t.chars().count())
            .max()
    }

    /// Minimum TODO keyword length in characters across the vault.
    #[must_use]
    pub fn min_todo_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::todo)
            .map(|t| t.chars().count())
            .min()
    }

    /// Total TODO keyword length in characters across the vault.
    #[must_use]
    pub fn total_todo_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::todo)
            .map(|t| t.chars().count())
            .sum()
    }

    /// Integer mean TODO keyword length in characters (`0` when no TODOs).
    #[must_use]
    pub fn mean_todo_len(&self) -> usize {
        self.total_todo_len()
            .checked_div(self.total_todo_count())
            .unwrap_or(0)
    }

    /// Median TODO keyword length in characters (`None` when no TODOs).
    #[must_use]
    pub fn median_todo_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(closure_core::DocHeadline::todo)
            .map(|t| t.chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of TODO keyword lengths (chars) to occurrence count.
    #[must_use]
    pub fn todo_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                if let Some(t) = h.todo() {
                    *m.entry(t.chars().count()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most common TODO keyword length (lowest wins ties).
    #[must_use]
    pub fn mode_todo_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.todo_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Total priority-set occurrences across the vault.
    #[must_use]
    pub fn total_priority_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.priority().is_some())
            .count()
    }

    /// Total TODO-set occurrences across the vault.
    #[must_use]
    pub fn total_todo_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.todo().is_some())
            .count()
    }

    /// Count of TODO-marked headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn todo_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
    }

    /// Maximum per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn max_file_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
            .max()
    }

    /// Minimum per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn min_file_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
            .min()
    }

    /// Integer mean per-file TODO-marked headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_todo_count(&self) -> usize {
        self.total_todo_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn median_file_todo_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-file TODO-marked headline counts.
    #[must_use]
    pub fn file_todo_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d.all_headlines().filter(|h| h.todo().is_some()).count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file TODO-marked headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_todo_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_todo_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Histogram of per-file prioritized-headline counts.
    #[must_use]
    pub fn file_priority_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d.all_headlines().filter(|h| h.priority().is_some()).count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file prioritized-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_priority_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_priority_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Histogram of per-file leaf counts.
    #[must_use]
    pub fn file_leaf_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .org()
                .iter_headlines()
                .into_iter()
                .filter(|h| h.is_leaf())
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file leaf count (lowest wins ties).
    #[must_use]
    pub fn mode_file_leaf_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_leaf_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Histogram of per-file branch counts.
    #[must_use]
    pub fn file_branch_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .org()
                .iter_headlines()
                .into_iter()
                .filter(|h| !h.is_leaf())
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file branch count (lowest wins ties).
    #[must_use]
    pub fn mode_file_branch_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.file_branch_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Histogram of per-file root-headline counts.
    #[must_use]
    pub fn file_root_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().roots().len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file root-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_root_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (rc, c) in self.file_root_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((rc, c));
            }
        }
        best.map(|(rc, _)| rc)
    }

    /// Percentage of distinct tags among total tag occurrences
    /// (`distinct * 100 / total`, `0` when no tags).
    #[must_use]
    pub fn tag_diversity_pct(&self) -> usize {
        (self.distinct_tag_count() * 100)
            .checked_div(self.total_tag_count())
            .unwrap_or(0)
    }

    /// Percentage of distinct TODO keywords among total TODO occurrences
    /// (`distinct * 100 / total`, `0` when no TODOs).
    #[must_use]
    pub fn todo_diversity_pct(&self) -> usize {
        (self.distinct_todo_count() * 100)
            .checked_div(self.total_todo_count())
            .unwrap_or(0)
    }

    /// Percentage of distinct levels among all headlines
    /// (`distinct levels * 100 / headline count`, `0` when empty).
    #[must_use]
    pub fn level_diversity_pct(&self) -> usize {
        (self.distinct_level_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of distinct property keys among total key occurrences
    /// (`distinct * 100 / total`, `0` when no properties).
    #[must_use]
    pub fn property_key_diversity_pct(&self) -> usize {
        let total: usize = self.property_key_counts().values().sum();
        (self.distinct_property_key_count() * 100)
            .checked_div(total)
            .unwrap_or(0)
    }

    /// Percentage of distinct priorities among total priority occurrences
    /// (`distinct * 100 / total`, `0` when no priorities).
    #[must_use]
    pub fn priority_diversity_pct(&self) -> usize {
        let total: usize = self.priority_counts().into_iter().map(|(_, c)| c).sum();
        (self.distinct_priority_count() * 100)
            .checked_div(total)
            .unwrap_or(0)
    }

    /// Percentage of distinct titles among all headlines
    /// (`distinct titles * 100 / headline count`, `0` when empty).
    #[must_use]
    pub fn title_diversity_pct(&self) -> usize {
        (self.distinct_title_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Total `:ID:` property occurrences across the vault.
    #[must_use]
    pub fn total_id_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.property("ID").is_some())
            .count()
    }

    /// Total property-pair occurrences across the vault.
    #[must_use]
    pub fn total_property_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .sum()
    }

    /// Total property-pair occurrences for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn property_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().map(|h| h.properties().len()).sum())
    }

    /// Maximum per-file property-pair count (`None` when no files).
    #[must_use]
    pub fn max_file_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.properties().len()).sum())
            .max()
    }

    /// Minimum per-file property-pair count (`None` when no files).
    #[must_use]
    pub fn min_file_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.properties().len()).sum())
            .min()
    }

    /// Integer mean per-file property-pair count (`0` when no files).
    #[must_use]
    pub fn mean_file_property_count(&self) -> usize {
        self.total_property_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file property-pair count (`None` when no files).
    #[must_use]
    pub fn median_file_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.properties().len()).sum())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Maximum per-headline property count across the vault.
    #[must_use]
    pub fn max_property_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .max()
    }

    /// Minimum per-headline property count across the vault.
    #[must_use]
    pub fn min_property_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .min()
    }

    /// Integer mean per-headline property count (`0` when no headlines).
    #[must_use]
    pub fn mean_property_count(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_property_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline property count (`None` when no headlines).
    #[must_use]
    pub fn median_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline property counts to occurrence count.
    #[must_use]
    pub fn property_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.properties().len()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline property count (lowest wins ties).
    #[must_use]
    pub fn mode_property_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.property_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum property-key character length across the vault.
    #[must_use]
    pub fn max_property_key_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(k, _)| k.chars().count()))
            .max()
    }

    /// Minimum property-key character length across the vault.
    #[must_use]
    pub fn min_property_key_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(k, _)| k.chars().count()))
            .min()
    }

    /// Total property-key character length across the vault.
    #[must_use]
    pub fn total_property_key_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(k, _)| k.chars().count()))
            .sum()
    }

    /// Integer mean property-key character length (`0` when no properties).
    #[must_use]
    pub fn mean_property_key_len(&self) -> usize {
        let n: usize = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .sum();
        self.total_property_key_len().checked_div(n).unwrap_or(0)
    }

    /// Median property-key character length (`None` when no properties).
    #[must_use]
    pub fn median_property_key_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(k, _)| k.chars().count()))
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of property-key character lengths to occurrence count.
    #[must_use]
    pub fn property_key_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (k, _) in h.properties() {
                    *m.entry(k.chars().count()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most common property-key character length (lowest wins ties).
    #[must_use]
    pub fn mode_property_key_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.property_key_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Maximum property-value character length across the vault.
    #[must_use]
    pub fn max_property_value_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(_, val)| val.chars().count()))
            .max()
    }

    /// Minimum property-value character length across the vault.
    #[must_use]
    pub fn min_property_value_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(_, val)| val.chars().count()))
            .min()
    }

    /// Total property-value character length across the vault.
    #[must_use]
    pub fn total_property_value_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(_, val)| val.chars().count()))
            .sum()
    }

    /// Integer mean property-value character length (`0` when no properties).
    #[must_use]
    pub fn mean_property_value_len(&self) -> usize {
        let n: usize = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.properties().len())
            .sum();
        self.total_property_value_len().checked_div(n).unwrap_or(0)
    }

    /// Median property-value character length (`None` when no properties).
    #[must_use]
    pub fn median_property_value_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(|h| h.properties().iter().map(|(_, val)| val.chars().count()))
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of property-value character lengths to occurrence count.
    #[must_use]
    pub fn property_value_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (_, val) in h.properties() {
                    *m.entry(val.chars().count()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most common property-value character length (lowest wins ties).
    #[must_use]
    pub fn mode_property_value_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.property_value_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Maximum per-headline timestamp count across the vault.
    #[must_use]
    pub fn max_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::timestamp_count)
            .max()
    }

    /// Minimum per-headline timestamp count across the vault.
    #[must_use]
    pub fn min_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::timestamp_count)
            .min()
    }

    /// Integer mean per-headline timestamp count (`0` when no headlines).
    #[must_use]
    pub fn mean_timestamp_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.timestamp_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline timestamp count (`None` when no headlines).
    #[must_use]
    pub fn median_timestamp_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::timestamp_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline timestamp counts to occurrence count.
    #[must_use]
    pub fn timestamp_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.timestamp_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline timestamp count (lowest wins ties).
    #[must_use]
    pub fn mode_timestamp_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.timestamp_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Count of headlines carrying at least one timestamp.
    #[must_use]
    pub fn with_timestamp_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .filter(|h| h.timestamp_count() > 0)
            .count()
    }

    /// Count of headlines carrying at least one timestamp for a single
    /// file by path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn with_timestamp_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| {
            d.org()
                .iter_headlines()
                .into_iter()
                .filter(|h| h.timestamp_count() > 0)
                .count()
        })
    }

    /// Maximum per-file timestamp-carrying headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.timestamp_count() > 0)
                    .count()
            })
            .max()
    }

    /// Minimum per-file timestamp-carrying headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.timestamp_count() > 0)
                    .count()
            })
            .min()
    }

    /// Integer mean per-file timestamp-carrying headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_timestamp_count(&self) -> usize {
        self.with_timestamp_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file timestamp-carrying headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_timestamp_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.timestamp_count() > 0)
                    .count()
            })
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying at least one timestamp (`0..=100`).
    #[must_use]
    pub fn timestamp_pct(&self) -> usize {
        (self.with_timestamp_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Maximum per-headline link count across the vault.
    #[must_use]
    pub fn max_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::link_count)
            .max()
    }

    /// Minimum per-headline link count across the vault.
    #[must_use]
    pub fn min_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::link_count)
            .min()
    }

    /// Integer mean per-headline link count (`0` when no headlines).
    #[must_use]
    pub fn mean_link_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.link_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline link count (`None` when no headlines).
    #[must_use]
    pub fn median_link_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::link_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline link counts to occurrence count.
    #[must_use]
    pub fn link_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.link_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline link count (lowest wins ties).
    #[must_use]
    pub fn mode_link_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.link_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Count of headlines carrying at least one link.
    #[must_use]
    pub fn with_link_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| !h.link_targets().is_empty())
            .count()
    }

    /// Count of headlines carrying at least one link for a single file
    /// by path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn with_link_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| !h.link_targets().is_empty()).count())
    }

    /// Maximum per-file link-carrying headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.link_targets().is_empty()).count())
            .max()
    }

    /// Minimum per-file link-carrying headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.link_targets().is_empty()).count())
            .min()
    }

    /// Integer mean per-file link-carrying headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_link_count(&self) -> usize {
        self.with_link_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file link-carrying headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_link_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.link_targets().is_empty()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying at least one link (`0..=100`).
    #[must_use]
    pub fn link_pct(&self) -> usize {
        (self.with_link_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Maximum per-headline child count across the vault.
    #[must_use]
    pub fn max_child_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(|h| h.children().len())
            .max()
    }

    /// Minimum per-headline child count across the vault.
    #[must_use]
    pub fn min_child_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(|h| h.children().len())
            .min()
    }

    /// Total child-count across all headlines in the vault.
    #[must_use]
    pub fn total_child_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(|h| h.children().len())
            .sum()
    }

    /// Integer mean per-headline child count (`0` when no headlines).
    #[must_use]
    pub fn mean_child_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_child_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline child count (`None` when no headlines).
    #[must_use]
    pub fn median_child_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(|h| h.children().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline child counts to occurrence count.
    #[must_use]
    pub fn child_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.children().len()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline child count (lowest wins ties).
    #[must_use]
    pub fn mode_child_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.child_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Maximum per-headline descendant count across the vault.
    #[must_use]
    pub fn max_descendant_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::descendant_count)
            .max()
    }

    /// Minimum per-headline descendant count across the vault.
    #[must_use]
    pub fn min_descendant_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::descendant_count)
            .min()
    }

    /// Total descendant-count across all headlines in the vault.
    #[must_use]
    pub fn total_descendant_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::descendant_count)
            .sum()
    }

    /// Integer mean per-headline descendant count (`0` when no headlines).
    #[must_use]
    pub fn mean_descendant_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_descendant_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline descendant count (`None` when no headlines).
    #[must_use]
    pub fn median_descendant_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::descendant_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline descendant counts to occurrence count.
    #[must_use]
    pub fn descendant_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.descendant_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline descendant count (lowest wins ties).
    #[must_use]
    pub fn mode_descendant_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (dc, c) in self.descendant_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((dc, c));
            }
        }
        best.map(|(dc, _)| dc)
    }

    /// Maximum per-headline subtree size across the vault.
    #[must_use]
    pub fn max_subtree_size(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_size)
            .max()
    }

    /// Minimum per-headline subtree size across the vault.
    #[must_use]
    pub fn min_subtree_size(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_size)
            .min()
    }

    /// Total subtree-size across all headlines in the vault.
    #[must_use]
    pub fn total_subtree_size(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_size)
            .sum()
    }

    /// Integer mean per-headline subtree size (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_size(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_size().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree size (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_size(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_size)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree sizes to occurrence count.
    #[must_use]
    pub fn subtree_size_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_size()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree size (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_size(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (sz, c) in self.subtree_size_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((sz, c));
            }
        }
        best.map(|(sz, _)| sz)
    }

    /// Maximum per-headline tag count across the vault.
    #[must_use]
    pub fn max_tag_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.tags().len())
            .max()
    }

    /// Minimum per-headline tag count across the vault.
    #[must_use]
    pub fn min_tag_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.tags().len())
            .min()
    }

    /// Integer mean per-headline tag count (`0` when no headlines).
    #[must_use]
    pub fn mean_tag_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_tag_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline tag count (`None` when no headlines).
    #[must_use]
    pub fn median_tag_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.tags().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline tag counts to occurrence count.
    #[must_use]
    pub fn tag_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.tags().len()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline tag count (lowest wins ties).
    #[must_use]
    pub fn mode_tag_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.tag_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-headline subtree word count across the vault.
    #[must_use]
    pub fn max_subtree_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_word_count)
            .max()
    }

    /// Minimum per-headline subtree word count across the vault.
    #[must_use]
    pub fn min_subtree_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_word_count)
            .min()
    }

    /// Sum of per-headline subtree word counts across the vault.
    #[must_use]
    pub fn total_subtree_word_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_word_count)
            .sum()
    }

    /// Integer mean per-headline subtree word count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_word_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_word_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree word count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_word_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree word counts to occurrence count.
    #[must_use]
    pub fn subtree_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_word_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree word count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.subtree_word_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Maximum per-headline subtree byte count across the vault.
    #[must_use]
    pub fn max_subtree_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_byte_count)
            .max()
    }

    /// Minimum per-headline subtree byte count across the vault.
    #[must_use]
    pub fn min_subtree_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_byte_count)
            .min()
    }

    /// Sum of per-headline subtree byte counts across the vault.
    #[must_use]
    pub fn total_subtree_byte_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_byte_count)
            .sum()
    }

    /// Integer mean per-headline subtree byte count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_byte_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_byte_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree byte count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_byte_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_byte_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree byte counts.
    #[must_use]
    pub fn subtree_byte_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_byte_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree byte count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_byte_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.subtree_byte_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Maximum per-headline subtree link count across the vault.
    #[must_use]
    pub fn max_subtree_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_link_count)
            .max()
    }

    /// Minimum per-headline subtree link count across the vault.
    #[must_use]
    pub fn min_subtree_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_link_count)
            .min()
    }

    /// Sum of per-headline subtree link counts across the vault.
    #[must_use]
    pub fn total_subtree_link_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_link_count)
            .sum()
    }

    /// Integer mean per-headline subtree link count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_link_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_link_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree link count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_link_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_link_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree link counts.
    #[must_use]
    pub fn subtree_link_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_link_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree link count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_link_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.subtree_link_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Maximum per-headline subtree tag count across the vault.
    #[must_use]
    pub fn max_subtree_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_tag_count)
            .max()
    }

    /// Minimum per-headline subtree tag count across the vault.
    #[must_use]
    pub fn min_subtree_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_tag_count)
            .min()
    }

    /// Sum of per-headline subtree tag counts across the vault.
    #[must_use]
    pub fn total_subtree_tag_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_tag_count)
            .sum()
    }

    /// Integer mean per-headline subtree tag count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_tag_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_tag_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree tag count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_tag_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_tag_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree tag counts.
    #[must_use]
    pub fn subtree_tag_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_tag_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree tag count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_tag_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.subtree_tag_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-headline subtree TODO count across the vault.
    #[must_use]
    pub fn max_subtree_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_todo_count)
            .max()
    }

    /// Minimum per-headline subtree TODO count across the vault.
    #[must_use]
    pub fn min_subtree_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_todo_count)
            .min()
    }

    /// Sum of per-headline subtree TODO counts across the vault.
    #[must_use]
    pub fn total_subtree_todo_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_todo_count)
            .sum()
    }

    /// Integer mean per-headline subtree TODO count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_todo_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_todo_count().checked_div(n).unwrap_or(0)
    }

    /// Median per-headline subtree TODO count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_todo_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_todo_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree TODO counts.
    #[must_use]
    pub fn subtree_todo_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_todo_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree TODO count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_todo_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.subtree_todo_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-headline subtree priority count across the vault.
    #[must_use]
    pub fn max_subtree_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_priority_count)
            .max()
    }

    /// Minimum per-headline subtree priority count across the vault.
    #[must_use]
    pub fn min_subtree_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_priority_count)
            .min()
    }

    /// Sum of per-headline subtree priority counts across the vault.
    #[must_use]
    pub fn total_subtree_priority_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_priority_count)
            .sum()
    }

    /// Integer mean per-headline subtree priority count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_priority_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_priority_count()
            .checked_div(n)
            .unwrap_or(0)
    }

    /// Median per-headline subtree priority count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_priority_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_priority_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree priority counts.
    #[must_use]
    pub fn subtree_priority_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_priority_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree priority count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_priority_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.subtree_priority_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-headline subtree level count across the vault.
    #[must_use]
    pub fn max_subtree_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_level_count)
            .max()
    }

    /// Minimum per-headline subtree level count across the vault.
    #[must_use]
    pub fn min_subtree_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_level_count)
            .min()
    }

    /// Sum of per-headline subtree level counts across the vault.
    #[must_use]
    pub fn total_subtree_level_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_level_count)
            .sum()
    }

    /// Integer mean per-headline subtree level count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_level_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_level_count()
            .checked_div(n)
            .unwrap_or(0)
    }

    /// Median per-headline subtree level count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_level_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_level_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree level counts.
    #[must_use]
    pub fn subtree_level_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_level_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree level count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_level_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.subtree_level_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Maximum per-headline subtree property count across the vault.
    #[must_use]
    pub fn max_subtree_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_property_count)
            .max()
    }

    /// Minimum per-headline subtree property count across the vault.
    #[must_use]
    pub fn min_subtree_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_property_count)
            .min()
    }

    /// Sum of per-headline subtree property counts across the vault.
    #[must_use]
    pub fn total_subtree_property_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_property_count)
            .sum()
    }

    /// Integer mean per-headline subtree property count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_property_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_property_count()
            .checked_div(n)
            .unwrap_or(0)
    }

    /// Median per-headline subtree property count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_property_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree property counts.
    #[must_use]
    pub fn subtree_property_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_property_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree property count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_property_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.subtree_property_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-headline subtree timestamp count across the vault.
    #[must_use]
    pub fn max_subtree_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_timestamp_count)
            .max()
    }

    /// Minimum per-headline subtree timestamp count across the vault.
    #[must_use]
    pub fn min_subtree_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_timestamp_count)
            .min()
    }

    /// Sum of per-headline subtree timestamp counts across the vault.
    #[must_use]
    pub fn total_subtree_timestamp_count(&self) -> usize {
        self.documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_timestamp_count)
            .sum()
    }

    /// Integer mean per-headline subtree timestamp count (`0` when no headlines).
    #[must_use]
    pub fn mean_subtree_timestamp_count(&self) -> usize {
        let n: usize = self
            .documents
            .values()
            .map(|d| d.org().iter_headlines().len())
            .sum();
        self.total_subtree_timestamp_count()
            .checked_div(n)
            .unwrap_or(0)
    }

    /// Median per-headline subtree timestamp count (`None` when no headlines).
    #[must_use]
    pub fn median_subtree_timestamp_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .flat_map(|d| d.org().iter_headlines())
            .map(closure_org::Headline::subtree_timestamp_count)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of per-headline subtree timestamp counts.
    #[must_use]
    pub fn subtree_timestamp_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.org().iter_headlines() {
                *m.entry(h.subtree_timestamp_count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common per-headline subtree timestamp count (lowest wins ties).
    #[must_use]
    pub fn mode_subtree_timestamp_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.subtree_timestamp_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Count of headlines carrying a non-empty body across the vault.
    #[must_use]
    pub fn with_body_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| !h.body_text().is_empty())
            .count()
    }

    /// Count of headlines carrying a non-empty body for a single file by
    /// path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn with_body_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| !h.body_text().is_empty()).count())
    }

    /// Maximum per-file non-empty-body headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_body_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.body_text().is_empty()).count())
            .max()
    }

    /// Minimum per-file non-empty-body headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_body_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.body_text().is_empty()).count())
            .min()
    }

    /// Integer mean per-file non-empty-body headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_body_count(&self) -> usize {
        self.with_body_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file non-empty-body headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_body_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.body_text().is_empty()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying a non-empty body (`0..=100`).
    #[must_use]
    pub fn body_pct(&self) -> usize {
        (self.with_body_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with an empty body across the vault.
    #[must_use]
    pub fn count_empty_body(&self) -> usize {
        self.headline_count() - self.with_body_count()
    }

    /// Percentage of headlines with an empty body (`0..=100`).
    #[must_use]
    pub fn empty_body_pct(&self) -> usize {
        (self.count_empty_body() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Number of headlines (across all paths) carrying `tag`.
    #[must_use]
    pub fn headline_count_with_tag(&self, tag: &str) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.tags().iter().any(|t| t == tag))
            .count()
    }

    /// Number of headlines (across all paths) with TODO keyword `kw`.
    #[must_use]
    pub fn headline_count_with_todo(&self, kw: &str) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.todo() == Some(kw))
            .count()
    }

    /// Number of headlines (across all paths) with priority `letter`.
    #[must_use]
    pub fn headline_count_with_priority(&self, letter: char) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.priority() == Some(letter))
            .count()
    }

    /// Number of headlines (across all paths) at exactly `level`.
    #[must_use]
    pub fn headline_count_at_level(&self, level: u8) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.level() == level)
            .count()
    }

    /// Number of headlines (across all paths) carrying property `key`.
    #[must_use]
    pub fn headline_count_with_property(&self, key: &str) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.property(key).is_some())
            .count()
    }

    /// Number of paths containing at least one headline.
    #[must_use]
    pub fn nonempty_path_count(&self) -> usize {
        self.iter()
            .filter(|(_, d)| d.all_headlines().count() > 0)
            .count()
    }

    /// Number of paths containing zero headlines.
    #[must_use]
    pub fn empty_path_count(&self) -> usize {
        self.iter()
            .filter(|(_, d)| d.all_headlines().count() == 0)
            .count()
    }

    /// Maximum headline count among paths.
    #[must_use]
    pub fn max_headlines_per_path(&self) -> Option<usize> {
        self.iter().map(|(_, d)| d.all_headlines().count()).max()
    }

    /// Minimum headline count among paths.
    #[must_use]
    pub fn min_headlines_per_path(&self) -> Option<usize> {
        self.iter().map(|(_, d)| d.all_headlines().count()).min()
    }

    /// Total headline count across the vault.
    #[must_use]
    pub fn total_headline_count(&self) -> usize {
        self.iter().map(|(_, d)| d.all_headlines().count()).sum()
    }

    /// All headline titles across the vault (with duplicates).
    #[must_use]
    pub fn all_titles(&self) -> Vec<String> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().to_owned())
            .collect()
    }

    /// Distinct headline titles across the vault, sorted.
    #[must_use]
    pub fn distinct_titles(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                seen.insert(h.title().to_owned());
            }
        }
        seen.into_iter().collect()
    }

    /// Total headline title length in characters across the vault.
    #[must_use]
    pub fn total_title_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().chars().count())
            .sum()
    }

    /// Integer mean headline title length (`0` when no headlines).
    #[must_use]
    pub fn mean_title_len(&self) -> usize {
        let n = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .count();
        self.total_title_len().checked_div(n).unwrap_or(0)
    }

    /// Maximum headline title length in characters across the vault.
    #[must_use]
    pub fn max_title_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().chars().count())
            .max()
    }

    /// Minimum headline title length in characters across the vault.
    #[must_use]
    pub fn min_title_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().chars().count())
            .min()
    }

    /// Median headline title length in characters (`None` when empty).
    #[must_use]
    pub fn median_title_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of headline title lengths (chars) to occurrence count.
    #[must_use]
    pub fn title_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.title().chars().count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common headline title length (lowest length wins ties).
    #[must_use]
    pub fn mode_title_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.title_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Total headline title length in bytes across the vault.
    #[must_use]
    pub fn total_title_byte_len(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().len())
            .sum()
    }

    /// Integer mean headline title byte length (`0` when no headlines).
    #[must_use]
    pub fn mean_title_byte_len(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_title_byte_len().checked_div(n).unwrap_or(0)
    }

    /// Maximum headline title length in bytes across the vault.
    #[must_use]
    pub fn max_title_byte_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().len())
            .max()
    }

    /// Minimum headline title length in bytes across the vault.
    #[must_use]
    pub fn min_title_byte_len(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().len())
            .min()
    }

    /// Median headline title length in bytes (`None` when no headlines).
    #[must_use]
    pub fn median_title_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().len())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of headline title byte lengths to occurrence count.
    #[must_use]
    pub fn title_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.title().len()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common headline title byte length (lowest wins ties).
    #[must_use]
    pub fn mode_title_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.title_byte_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Total whitespace-separated word count across all headline titles.
    #[must_use]
    pub fn total_title_word_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().split_whitespace().count())
            .sum()
    }

    /// Integer mean title word count (`0` when no headlines).
    #[must_use]
    pub fn mean_title_word_count(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_title_word_count().checked_div(n).unwrap_or(0)
    }

    /// Maximum title word count across headlines.
    #[must_use]
    pub fn max_title_word_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().split_whitespace().count())
            .max()
    }

    /// Minimum title word count across headlines.
    #[must_use]
    pub fn min_title_word_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().split_whitespace().count())
            .min()
    }

    /// Median title word count across headlines (`None` when empty).
    #[must_use]
    pub fn median_title_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.title().split_whitespace().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of title word counts to occurrence count.
    #[must_use]
    pub fn title_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.title().split_whitespace().count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common title word count (lowest count wins ties).
    #[must_use]
    pub fn mode_title_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.title_word_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Total headline-body line count across the vault.
    #[must_use]
    pub fn total_body_line_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().lines().count())
            .sum()
    }

    /// Total headline-body line count for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn body_line_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().map(|h| h.body_text().lines().count()).sum())
    }

    /// Maximum per-file body line count (`None` when no files).
    #[must_use]
    pub fn max_file_body_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().lines().count()).sum())
            .max()
    }

    /// Minimum per-file body line count (`None` when no files).
    #[must_use]
    pub fn min_file_body_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().lines().count()).sum())
            .min()
    }

    /// Integer mean per-file body line count (`0` when no files).
    #[must_use]
    pub fn mean_file_body_line_count(&self) -> usize {
        self.total_body_line_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file body line count (`None` when no files).
    #[must_use]
    pub fn median_file_body_line_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.body_text().lines().count()).sum())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Integer mean headline-body line count (`0` when no headlines).
    #[must_use]
    pub fn mean_body_line_count(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_body_line_count().checked_div(n).unwrap_or(0)
    }

    /// Maximum headline-body line count across the vault.
    #[must_use]
    pub fn max_body_line_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().lines().count())
            .max()
    }

    /// Minimum headline-body line count across the vault.
    #[must_use]
    pub fn min_body_line_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().lines().count())
            .min()
    }

    /// Total headline-body char count across the vault.
    #[must_use]
    pub fn total_body_char_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().chars().count())
            .sum()
    }

    /// Total headline-body char count for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn body_char_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .map(|h| h.body_text().chars().count())
                .sum()
        })
    }

    /// Maximum per-file body char count (`None` when no files).
    #[must_use]
    pub fn max_file_body_char_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().chars().count())
                    .sum()
            })
            .max()
    }

    /// Minimum per-file body char count (`None` when no files).
    #[must_use]
    pub fn min_file_body_char_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().chars().count())
                    .sum()
            })
            .min()
    }

    /// Integer mean per-file body char count (`0` when no files).
    #[must_use]
    pub fn mean_file_body_char_count(&self) -> usize {
        self.total_body_char_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file body char count (`None` when no files).
    #[must_use]
    pub fn median_file_body_char_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().chars().count())
                    .sum()
            })
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Integer mean headline-body char count (`0` when no headlines).
    #[must_use]
    pub fn mean_body_char_count(&self) -> usize {
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
        self.total_body_char_count().checked_div(n).unwrap_or(0)
    }

    /// Maximum headline-body char count across the vault.
    #[must_use]
    pub fn max_body_char_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().chars().count())
            .max()
    }

    /// Minimum headline-body char count across the vault.
    #[must_use]
    pub fn min_body_char_count(&self) -> Option<usize> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().chars().count())
            .min()
    }

    /// Median headline-body char count (`None` when no headlines).
    #[must_use]
    pub fn median_body_char_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of headline-body char counts to occurrence count.
    #[must_use]
    pub fn body_char_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.body_text().chars().count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common headline-body char count (lowest wins ties).
    #[must_use]
    pub fn mode_body_char_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.body_char_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Median headline-body line count (`None` when no headlines).
    #[must_use]
    pub fn median_body_line_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .iter()
            .flat_map(|(_, d)| d.all_headlines())
            .map(|h| h.body_text().lines().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of headline-body line counts to occurrence count.
    #[must_use]
    pub fn body_line_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                *m.entry(h.body_text().lines().count()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most common headline-body line count (lowest wins ties).
    #[must_use]
    pub fn mode_body_line_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.body_line_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// All `:ID:` property values across the vault (with duplicates).
    #[must_use]
    pub fn all_id_properties(&self) -> Vec<String> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter_map(|h| h.property("ID").map(str::to_owned))
            .collect()
    }

    /// Distinct `:ID:` property values across the vault, sorted.
    #[must_use]
    pub fn distinct_id_properties(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                if let Some(id) = h.property("ID") {
                    seen.insert(id.to_owned());
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Number of loaded paths in the vault.
    #[must_use]
    pub fn path_count(&self) -> usize {
        self.documents.len()
    }

    /// Count of distinct `:ID:` property values across the vault.
    #[must_use]
    pub fn distinct_id_property_count(&self) -> usize {
        self.distinct_id_properties().len()
    }

    /// Sorted distinct property keys across the vault.
    #[must_use]
    pub fn distinct_property_keys(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (k, _) in h.properties() {
                    seen.insert(k.clone());
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Count of distinct property keys across the vault.
    #[must_use]
    pub fn distinct_property_key_count(&self) -> usize {
        self.distinct_property_keys().len()
    }

    /// Property-key occurrence counts as a sorted map.
    #[must_use]
    pub fn property_key_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (k, _) in h.properties() {
                    *m.entry(k.clone()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most-common property key (lowest key wins ties).
    #[must_use]
    pub fn most_common_property_key(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (k, c) in self.property_key_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((k, c));
            }
        }
        best.map(|(k, _)| k)
    }

    /// All values bound to property `key` across the vault (with duplicates).
    #[must_use]
    pub fn property_values(&self, key: &str) -> Vec<String> {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .flat_map(closure_core::DocHeadline::properties)
            .filter(|(k, _)| k == key)
            .map(|(_, val)| val.clone())
            .collect()
    }

    /// Sorted distinct values bound to property `key` across the vault.
    #[must_use]
    pub fn distinct_property_values(&self, key: &str) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (k, val) in h.properties() {
                    if k == key {
                        seen.insert(val.clone());
                    }
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Count of distinct values bound to property `key`.
    #[must_use]
    pub fn distinct_property_value_count(&self, key: &str) -> usize {
        self.distinct_property_values(key).len()
    }

    /// Value occurrence counts for property `key` as a sorted map.
    #[must_use]
    pub fn property_value_counts(&self, key: &str) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            for h in d.all_headlines() {
                for (k, val) in h.properties() {
                    if k == key {
                        *m.entry(val.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        m
    }

    /// Most-common value bound to property `key` (lowest value wins ties).
    #[must_use]
    pub fn most_common_property_value(&self, key: &str) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (val, c) in self.property_value_counts(key) {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((val, c));
            }
        }
        best.map(|(val, _)| val)
    }

    /// Least-common property key (lowest count; ties by name asc).
    #[must_use]
    pub fn least_common_property_key(&self) -> Option<String> {
        self.property_key_counts()
            .into_iter()
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// Least-common value bound to property `key` (lowest count; ties name asc).
    #[must_use]
    pub fn least_common_property_value(&self, key: &str) -> Option<String> {
        self.property_value_counts(key)
            .into_iter()
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
            .map(|(val, _)| val)
    }

    /// Count of headlines carrying at least one property across the vault.
    #[must_use]
    pub fn with_property_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| !h.properties().is_empty())
            .count()
    }

    /// Count of headlines carrying at least one property for a single
    /// file by path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn with_property_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| !h.properties().is_empty()).count())
    }

    /// Maximum per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.properties().is_empty()).count())
            .max()
    }

    /// Minimum per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.properties().is_empty()).count())
            .min()
    }

    /// Integer mean per-file property-carrying headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_property_count(&self) -> usize {
        self.with_property_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().filter(|h| !h.properties().is_empty()).count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Percentage of headlines carrying at least one property (`0..=100`).
    #[must_use]
    pub fn property_pct(&self) -> usize {
        (self.with_property_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines carrying no property.
    #[must_use]
    pub fn count_no_property(&self) -> usize {
        self.headline_count() - self.with_property_count()
    }

    /// Percentage of headlines carrying no property (`0..=100`).
    #[must_use]
    pub fn no_property_pct(&self) -> usize {
        (self.count_no_property() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with an empty title across the vault.
    #[must_use]
    pub fn empty_title_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.title().is_empty())
            .count()
    }

    /// Percentage of headlines with an empty title (`0..=100`).
    #[must_use]
    pub fn empty_title_pct(&self) -> usize {
        (self.empty_title_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with a non-empty title.
    #[must_use]
    pub fn count_nonempty_title(&self) -> usize {
        self.headline_count() - self.empty_title_count()
    }

    /// Percentage of headlines with a non-empty title (`0..=100`).
    #[must_use]
    pub fn nonempty_title_pct(&self) -> usize {
        (self.count_nonempty_title() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of distinct titles across the vault.
    #[must_use]
    pub fn distinct_title_count(&self) -> usize {
        self.distinct_titles().len()
    }

    /// Number of headlines whose title duplicates another
    /// (total headlines minus distinct titles).
    #[must_use]
    pub fn duplicate_title_count(&self) -> usize {
        self.headline_count() - self.distinct_title_count()
    }

    /// True iff any title appears on more than one headline.
    #[must_use]
    pub fn has_duplicate_titles(&self) -> bool {
        self.duplicate_title_count() > 0
    }

    /// Percentage of headlines whose title is distinct (`0..=100`).
    #[must_use]
    pub fn unique_title_pct(&self) -> usize {
        (self.distinct_title_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Path with the maximum headline count. Ties resolved by lexicographic path.
    #[must_use]
    pub fn path_with_max_headlines(&self) -> Option<&Path> {
        let mut best: Option<(&Path, usize)> = None;
        for (p, d) in self.iter() {
            let n = d.all_headlines().count();
            best = match best {
                Some((bp, bn)) if bn > n || (bn == n && bp < p) => Some((bp, bn)),
                _ => Some((p, n)),
            };
        }
        best.map(|(p, _)| p)
    }

    /// Path with the minimum headline count. Ties resolved by lexicographic path.
    #[must_use]
    pub fn path_with_min_headlines(&self) -> Option<&Path> {
        let mut best: Option<(&Path, usize)> = None;
        for (p, d) in self.iter() {
            let n = d.all_headlines().count();
            best = match best {
                Some((bp, bn)) if bn < n || (bn == n && bp < p) => Some((bp, bn)),
                _ => Some((p, n)),
            };
        }
        best.map(|(p, _)| p)
    }

    /// Median headline count per path (integer, lower-middle for even sets).
    #[must_use]
    pub fn median_headlines_per_path(&self) -> Option<usize> {
        let mut counts: Vec<usize> =
            self.iter().map(|(_, d)| d.all_headlines().count()).collect();
        if counts.is_empty() {
            return None;
        }
        counts.sort_unstable();
        let mid = counts.len() / 2;
        if counts.len() % 2 == 1 {
            Some(counts[mid])
        } else {
            Some(counts[mid - 1].midpoint(counts[mid]))
        }
    }

    /// Histogram of per-path headline counts to occurrence count.
    #[must_use]
    pub fn headlines_per_path_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, d) in self.iter() {
            *m.entry(d.all_headlines().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-path headline count (lowest wins ties).
    #[must_use]
    pub fn mode_headlines_per_path(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (hc, c) in self.headlines_per_path_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((hc, c));
            }
        }
        best.map(|(hc, _)| hc)
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
