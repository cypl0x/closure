//! Vault-level storage: directory loader, cross-file block-id index,
//! and a recursive file watcher.

#![forbid(unsafe_code)]

mod clipboard;
#[cfg(feature = "clipboard")]
pub use clipboard::SystemClipboard;
pub use clipboard::{Clipboard, MemoryClipboard};

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use closure_core::{
    AddSibling, BlockId, Command, Demote, Document, MoveSubtree, Promote, RemoveSubtree,
    RenameHeadline, SetBody, SetPriority, SetProperty, SetTags, SetTodo,
};
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
    /// Kill ring: cut subtree sources, most-recent last.
    kill_ring: Vec<String>,
    /// Monotone change token — see [`Vault::revision`].
    revision: u64,
}

/// An org-capture template: where a new entry lands and what it
/// looks like. The title is supplied per capture; the entry always
/// receives a fresh `:ID:` drawer (I2).
#[derive(Debug, Clone)]
pub struct CaptureTemplate {
    /// Target file, relative to the vault root. Created when missing.
    pub target: PathBuf,
    /// Text inserted between the stars and the title, e.g. `"TODO "`.
    pub headline_prefix: String,
    /// Skeleton lines appended below the property drawer.
    pub body: String,
}

/// Extract a `YYYY-MM-DD` date from an org timestamp such as
/// `<2026-06-13 Fri>` or `[2026-06-13]`. `None` when no 10-char
/// `dddd-dd-dd` prefix is present.
/// Turn `YYYY-MM-DD HH:MM` into the stamp body org writes inside a
/// `CLOCK:` entry: `2026-07-28 Tue 09:15` (Q3-V3).
fn org_clock_stamp(now: &str) -> Result<String, VaultError> {
    let trimmed = now.trim();
    let (date, time) = trimmed.split_once(' ').unwrap_or((trimmed, "00:00"));
    let mut parts = date.split('-');
    let bad = || VaultError::Command(format!("`{now}` is not `YYYY-MM-DD HH:MM`"));
    let y: i64 = parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let m: u32 = parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let d: u32 = parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let day = closure_shell_weekday(y, m, d);
    Ok(format!("{y:04}-{m:02}-{d:02} {day} {time}"))
}

/// Org's three-letter weekday for a civil date.
///
/// The same arithmetic the shells' calendar uses; duplicated rather
/// than depended on because the store sits *below* the shells (I7 —
/// arrows point up).
fn closure_shell_weekday(y: i64, m: u32, d: u32) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    NAMES[usize::try_from(days.rem_euclid(7)).unwrap_or(0)]
}

/// Minutes between two `CLOCK:` stamp bodies (`2026-07-28 Tue 09:15`).
fn clock_span_minutes(start: &str, end: &str) -> Option<u64> {
    let parse = |s: &str| -> Option<i64> {
        let mut it = s.split_whitespace();
        let date = it.next()?;
        let time = it.next_back()?;
        let mut d = date.split('-');
        let y: i64 = d.next()?.parse().ok()?;
        let m: i64 = d.next()?.parse().ok()?;
        let day: i64 = d.next()?.parse().ok()?;
        let (h, min) = time.split_once(':')?;
        let (h, min): (i64, i64) = (h.parse().ok()?, min.parse().ok()?);
        let (y2, m2) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
        let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
        let yoe = y2 - era * 400;
        let mp = (m2 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(days * 24 * 60 + h * 60 + min)
    };
    let (a, b) = (parse(start)?, parse(end)?);
    u64::try_from(b - a).ok()
}

/// Shift every heading in a subtree's source by `delta` stars, so a
/// tree filed under a deeper parent nests the way it read before
/// (Q3-V1).
///
/// Only lines that are headings move: a body line that begins with a
/// star is escaped on disk (`,*`, org's own convention), so it is not
/// one and must not grow a star.
fn shift_subtree_levels(subtree: &str, delta: i16) -> String {
    if delta == 0 {
        return subtree.to_owned();
    }
    let mut out = String::with_capacity(subtree.len());
    for line in subtree.split_inclusive('\n') {
        let stars = line.chars().take_while(|&c| c == '*').count();
        let is_heading = stars > 0
            && line[stars..]
                .chars()
                .next()
                .is_some_and(|c| c == ' ' || c == '\t');
        if is_heading {
            let level = i16::try_from(stars).unwrap_or(1) + delta;
            let level = usize::try_from(level.max(1)).unwrap_or(1);
            out.push_str(&"*".repeat(level));
            out.push_str(&line[stars..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Whether a subtree's source carries the `:ID:` `id` — the guard
/// against filing a tree into one of its own branches.
fn subtree_contains(subtree: &str, id: &str) -> bool {
    subtree.lines().any(|l| {
        l.trim()
            .strip_prefix(":ID:")
            .is_some_and(|v| v.trim() == id)
    })
}

fn agenda_date(timestamp: &str) -> Option<String> {
    let body = timestamp.trim_start_matches(['<', '[']);
    let date: String = body.chars().take(10).collect();
    let ok = date.len() == 10
        && date.as_bytes().iter().enumerate().all(|(i, &b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    ok.then_some(date)
}

/// Which planning line produced an [`AgendaEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgendaKind {
    /// `SCHEDULED:` planning line.
    Scheduled,
    /// `DEADLINE:` planning line.
    Deadline,
}

/// One agenda row: a planned headline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaEntry {
    /// File containing the headline.
    pub path: PathBuf,
    /// Stable block id.
    pub id: String,
    /// Headline title.
    pub title: String,
    /// Scheduled or deadline.
    pub kind: AgendaKind,
    /// `YYYY-MM-DD` extracted from the org timestamp.
    pub date: String,
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
    /// The vault root does not exist.
    ///
    /// Distinguished from a bare [`Self::Io`] because the io error for
    /// a missing directory is `No such file or directory (os error 2)`
    /// and names nothing — not the path it tried, not what would make
    /// one. A user whose vault lives on another machine reads that and
    /// concludes the shell is broken.
    #[error("no vault at {path} — `closure init-vault {path}` makes one there")]
    NoVault {
        /// The path that was asked for.
        path: PathBuf,
    },
    /// The vault root exists but is a file.
    #[error("{path} is a file, not a vault directory")]
    NotADirectory {
        /// The path that was asked for.
        path: PathBuf,
    },
    /// Watcher subsystem error.
    #[error("watch: {0}")]
    Watch(String),
    /// No headline with this block id exists in the vault.
    #[error("unknown block id: {0}")]
    UnknownId(String),
    /// Undo/redo could not walk the history.
    #[error("undo: {0}")]
    Undo(String),
    /// A kernel command refused the edit.
    #[error("command: {0}")]
    Command(String),

    /// Code-block evaluation refused: the block's language is not in the
    /// vault's `eval_trust` allowlist (C1a default-deny security gate).
    #[error("eval blocked: `{lang}` not in eval_trust (add it to config.org to allow)")]
    EvalBlocked {
        /// The block's language as written.
        lang: String,
    },
}

/// FNV-1a hash of a string, matching `closure_org::OrgDoc::source_hash`
/// so an on-disk file can be hash-compared without re-parsing.
/// Outline paths of every headline under `path`, depth first.
///
/// Structural indices, not byte offsets, so they survive the rewrites
/// applied to the headlines they name (a properties drawer inserted
/// into one moves no path).
fn descendant_paths(org: &closure_org::OrgDoc, path: &[usize]) -> Vec<Vec<usize>> {
    fn walk(h: &closure_org::Headline, base: &[usize], out: &mut Vec<Vec<usize>>) {
        for (i, child) in h.children().iter().enumerate() {
            let mut p = base.to_vec();
            p.push(i);
            out.push(p.clone());
            walk(child, &p, out);
        }
    }
    let Some(first) = path.first() else {
        return Vec::new();
    };
    let Some(mut head) = org.roots().get(*first) else {
        return Vec::new();
    };
    for i in &path[1..] {
        let Some(next) = head.children().get(*i) else {
            return Vec::new();
        };
        head = next;
    }
    let mut out = Vec::new();
    walk(head, path, &mut out);
    out
}

fn fnv1a(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

impl Vault {
    /// Reload the vault from disk: re-walks the root, re-parses every
    /// `*.org` file, and rebuilds the id and backlink indices.
    pub fn reload(&mut self) -> Result<(), VaultError> {
        let fresh = Self::open(&self.root)?;
        self.documents = fresh.documents;
        self.by_id = fresh.by_id;
        self.backlinks = fresh.backlinks;
        // Every document was replaced; the fresh vault's own counter
        // restarts at 0, so bump ours rather than adopt it (I3: the
        // token must never go backwards).
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// Incrementally reload from disk: re-parse only files whose
    /// on-disk content hash differs from the cached document, add new
    /// files, drop deleted ones. Indices are rebuilt only when
    /// something changed. Returns the number of files re-parsed or
    /// added (0 ⇒ full rescan avoided).
    ///
    /// # Errors
    ///
    /// [`VaultError::Io`] / [`VaultError::Parse`] on read/parse
    /// failures.
    pub fn reload_incremental(&mut self) -> Result<usize, VaultError> {
        let mut on_disk: Vec<PathBuf> = Vec::new();
        walk_org(&self.root, &mut |path| {
            on_disk.push(path.to_path_buf());
            Ok(())
        })?;
        let mut changed = 0usize;
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for path in on_disk {
            seen.insert(path.clone());
            let src = fs::read_to_string(&path)?;
            let same = self
                .documents
                .get(&path)
                .is_some_and(|doc| doc.source_hash() == fnv1a(&src));
            if same {
                continue;
            }
            let doc =
                Document::load_str(&src).map_err(|_| VaultError::Parse { path: path.clone() })?;
            self.documents.insert(path, doc);
            changed += 1;
        }
        // Drop documents whose files vanished.
        let removed: Vec<PathBuf> = self
            .documents
            .keys()
            .filter(|p| !seen.contains(*p))
            .cloned()
            .collect();
        for path in &removed {
            self.documents.remove(path);
        }
        if changed > 0 || !removed.is_empty() {
            self.rebuild_indices();
        }
        Ok(changed)
    }

    /// Re-validate the vault's `config.org` (if present) using the
    /// typed CUE-style loader from `closure-config`.
    ///
    /// Returns the rich `ConfigError` (carrying line context from our
    /// validators) on failure. This enables "validate-on-save":
    /// shells or daemons can call this on watcher `Modified` events for
    /// config files and surface the error (e.g. in TUI status line).
    ///
    /// If no `config.org` exists, this succeeds (consistent with
    /// optional config).
    pub fn revalidate_config(&self) -> Result<(), closure_config::ConfigError> {
        let cfg_path = self.root.join("config.org");
        if !cfg_path.exists() {
            return Ok(());
        }
        // from_path performs the full load-time validation (unknown keys,
        // bad values, and now our threaded line info).
        closure_config::Config::from_path(&cfg_path).map(|_| ())
    }

    /// The vault's `eval_trust` allowlist (languages permitted to
    /// execute), from `config.org`. Absent or invalid config yields an
    /// empty list — default-deny, the C1a security default: an
    /// unreadable policy never silently permits execution.
    ///
    /// Public because the shells now run blocks straight out of an open
    /// buffer as well as out of a saved file, and a second route to
    /// evaluation must consult the same policy rather than invent one.
    #[must_use]
    pub fn eval_trust(&self) -> Vec<String> {
        let cfg_path = self.root.join("config.org");
        if !cfg_path.exists() {
            return Vec::new();
        }
        closure_config::Config::from_path(&cfg_path)
            .map(|c| c.eval_trust)
            .unwrap_or_default()
    }

    /// The configured TODO keywords, in order.
    ///
    /// Reads `config.org`'s `todo_keywords`; falls back to the
    /// `closure-config` default (`TODO`, `DONE`) when there is no config
    /// file or it fails to load. Used by editor-facing surfaces (LSP
    /// completion) that offer the vault's keyword vocabulary.
    #[must_use]
    pub fn todo_keywords(&self) -> Vec<String> {
        let cfg_path = self.root.join("config.org");
        if !cfg_path.exists() {
            return closure_config::Config::default().todo_keywords;
        }
        closure_config::Config::from_path(&cfg_path).map_or_else(
            |_| closure_config::Config::default().todo_keywords,
            |c| c.todo_keywords,
        )
    }

    /// Rebuild the id and backlink indices from the current documents.
    fn rebuild_indices(&mut self) {
        self.by_id.clear();
        self.backlinks.clear();
        let paths: Vec<PathBuf> = self.documents.keys().cloned().collect();
        for path in paths {
            self.reindex_file(&path);
        }
    }

    /// Open the vault at `root`, loading every `*.org` file underneath.
    pub fn open(root: &Path) -> Result<Self, VaultError> {
        // Refuse a root that is not there before walking it. The walk's
        // own io error says `No such file or directory (os error 2)`
        // and names neither the path nor a way out, which is a bad
        // answer to the most ordinary mistake there is: pointing at a
        // vault that lives on your other machine.
        if !root.exists() {
            return Err(VaultError::NoVault {
                path: root.to_path_buf(),
            });
        }
        if !root.is_dir() {
            return Err(VaultError::NotADirectory {
                path: root.to_path_buf(),
            });
        }
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
            kill_ring: Vec::new(),
            revision: 0,
        })
    }

    /// Monotone change token for the loaded documents.
    ///
    /// Stable across every read, strictly increasing across every
    /// mutation — kernel commands, undo/redo, file-level operations
    /// and reloads alike. Shells memoise derived views (outline rows,
    /// agendas, backlink tables) against it: an unchanged revision
    /// means a cached derivation is still exact, which is what keeps
    /// the render path off a full vault walk per frame.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Inverted backlink lookup. Returns every `(file, source-id)`
    /// whose headline links to `target`. Both `id:<ULID>` and bare
    /// ULID forms are accepted.
    #[must_use]
    pub fn backlinks_of(&self, target: &str) -> &[(PathBuf, BlockId)] {
        static EMPTY: Vec<(PathBuf, BlockId)> = Vec::new();
        self.backlinks.get(target).map_or(&EMPTY, Vec::as_slice)
    }

    /// Capture a new templated entry: render a top-level headline
    /// with a fresh `:ID:` drawer (I2), append it to the template's
    /// target file (creating the file when missing), persist to disk,
    /// and fold the new document into the vault's indexes. Existing
    /// bytes are preserved as an exact prefix (I1).
    ///
    /// # Errors
    ///
    /// [`VaultError::Io`] on read/write failures, [`VaultError::Parse`]
    /// when the appended result does not parse (disk is left
    /// untouched in that case).
    pub fn capture(
        &mut self,
        template: &CaptureTemplate,
        title: &str,
    ) -> Result<BlockId, VaultError> {
        let target = self.root.join(&template.target);
        let existing = match fs::read_to_string(&target) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let id = BlockId::fresh();
        let mut out = existing;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "* {prefix}{title}\n:PROPERTIES:\n:ID: {id}\n:END:",
            prefix = template.headline_prefix,
            id = id.as_str()
        );
        if !template.body.is_empty() {
            out.push_str(&template.body);
            if !template.body.ends_with('\n') {
                out.push('\n');
            }
        }
        let doc = Document::load_str(&out).map_err(|_| VaultError::Parse {
            path: target.clone(),
        })?;
        fs::write(&target, doc.source())?;
        for h in doc.all_headlines() {
            if h.id() == &id {
                self.by_id.insert(id.clone(), target.clone());
                for link in h.link_targets() {
                    self.backlinks
                        .entry(link.clone())
                        .or_default()
                        .push((target.clone(), id.clone()));
                    if let Some(stripped) = link.strip_prefix("id:") {
                        self.backlinks
                            .entry(stripped.to_owned())
                            .or_default()
                            .push((target.clone(), id.clone()));
                    }
                }
            }
        }
        self.documents.insert(target.clone(), doc);
        // Capture built its own index entries above and so used to
        // return without reindexing — which also meant without moving
        // the revision. Every shell memoises its row list against that
        // token, so a captured item stayed invisible until something
        // else happened to change the vault.
        self.reindex_file(&target);
        Ok(id)
    }

    /// Set a headline's body, filing any headlines typed into it as
    /// real children of it.
    ///
    /// The body editor shows a body; a line starting with `*` in one is
    /// a headline the moment it goes back to disk. Escaping it (org's
    /// comma) is right for prose that happens to start with a star and
    /// wrong for what people actually mean, which is "this belongs
    /// under this". See [`closure_org::rewrite_body_with_children`] for
    /// the rebasing rule and why existing children are untouched.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when `id` names no headline,
    /// [`VaultError::Parse`] if the result would not parse, and IO
    /// failures from the write.
    /// Every child of `id`, verbatim, for a body editor that shows the
    /// whole subtree ([`closure_org::children_source`]).
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when nothing has that id.
    pub fn children_source(&self, id: &BlockId) -> Result<String, VaultError> {
        let path = self
            .by_id
            .get(id)
            .cloned()
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let doc = self
            .documents
            .get(&path)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let outline_path = doc
            .path_of(id)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        closure_org::children_source(doc.org(), &outline_path)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))
    }

    /// Replace everything under `id` — body and children — with what a
    /// body editor showing the whole subtree now holds (I8).
    ///
    /// [`Self::set_body_with_children`] adds children without being
    /// able to remove them, which is right for an editor that cannot
    /// show them and wrong for one that can.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for a missing block, [`VaultError::
    /// Parse`] when the result will not parse.
    pub fn set_subtree(
        &mut self,
        id: &BlockId,
        body: &str,
        children_src: &str,
    ) -> Result<(), VaultError> {
        let path = self
            .by_id
            .get(id)
            .cloned()
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let doc = self
            .documents
            .get(&path)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let outline_path = doc
            .path_of(id)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let mut org =
            closure_org::rewrite_subtree_content(doc.org(), &outline_path, body, children_src)
                .map_err(|_| VaultError::Parse { path: path.clone() })?;
        // A headline typed into the buffer is a headline like any
        // other, and to everything above the parser a headline *is* its
        // id. `ensure_id` leaves an existing one alone, so a child read
        // out of the file and written back keeps its identity.
        for child in descendant_paths(&org, &outline_path) {
            org = closure_org::rewrite_headline_ensure_id(&org, &child, BlockId::fresh().as_str())
                .map_err(|_| VaultError::Parse { path: path.clone() })?;
        }
        let source = org.source().to_owned();
        self.set_source(&path, &source)
    }

    /// Set a body, filing headlines typed into it as children, leaving
    /// existing children in place.
    ///
    /// [`Self::set_subtree`] is the one to reach for when the buffer
    /// shows the children too.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for a missing block, [`VaultError::
    /// Parse`] when the result will not parse.
    pub fn set_body_with_children(
        &mut self,
        id: &BlockId,
        body: &str,
        children_src: &str,
    ) -> Result<(), VaultError> {
        let path = self
            .by_id
            .get(id)
            .cloned()
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let doc = self
            .documents
            .get(&path)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let outline_path = doc
            .path_of(id)
            .ok_or_else(|| VaultError::UnknownId(id.to_string()))?;
        let mut org =
            closure_org::rewrite_body_with_children(doc.org(), &outline_path, body, children_src)
                .map_err(|_| VaultError::Parse { path: path.clone() })?;
        // A headline typed into a body is a headline like any other,
        // and what a headline *is* to everything above the parser is
        // its id: sync addresses blocks by id, and so do links, the
        // undo tree and every row cache. Parsed without one it still
        // gets an id — a fresh ULID, in memory, for this run only — so
        // it worked perfectly until the file was read a second time and
        // then it was a different block. Stamped here, on the way to
        // disk, exactly like a capture. `ensure_id` leaves an existing
        // one alone, so a pasted subtree keeps the identity it arrived
        // with.
        for child in descendant_paths(&org, &outline_path) {
            org = closure_org::rewrite_headline_ensure_id(&org, &child, BlockId::fresh().as_str())
                .map_err(|_| VaultError::Parse { path: path.clone() })?;
        }
        let source = org.source().to_owned();
        self.set_source(&path, &source)
    }

    /// Capture a new headline as the last child of `parent`.
    ///
    /// The flat [`Self::capture`] files everything at the top of one
    /// inbox; this files it where the user is looking, which is what
    /// an outliner is for. Same shape as the flat capture — a fresh
    /// id, written through the document so the file on disk is what a
    /// second reader would parse (I1/I2) — and the same reindex, so
    /// the row lists memoised against the revision rebuild.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when `parent` names no headline,
    /// [`VaultError::Parse`] if the rewrite would not parse, and IO
    /// failures from the write.
    pub fn capture_under(
        &mut self,
        parent: &BlockId,
        prefix: &str,
        title: &str,
    ) -> Result<BlockId, VaultError> {
        let path = self
            .by_id
            .get(parent)
            .cloned()
            .ok_or_else(|| VaultError::UnknownId(parent.to_string()))?;
        let doc = self
            .documents
            .get(&path)
            .ok_or_else(|| VaultError::UnknownId(parent.to_string()))?;
        let outline_path = doc
            .path_of(parent)
            .ok_or_else(|| VaultError::UnknownId(parent.to_string()))?;
        let id = BlockId::fresh();
        let org = closure_org::rewrite_add_child_with_id(
            doc.org(),
            &outline_path,
            &format!("{prefix}{title}"),
            id.as_str(),
        )
        .map_err(|_| VaultError::Parse { path: path.clone() })?;
        let source = org.source().to_owned();
        self.set_source(&path, &source)?;
        Ok(id)
    }

    /// Rename a headline through the kernel [`RenameHeadline`]
    /// command (undoable, I3; the block id stays put, I2) and persist
    /// the document to disk.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when no headline carries `id`,
    /// [`VaultError::Command`] when the kernel refuses the edit,
    /// [`VaultError::Io`] on write failures.
    pub fn rename_headline(&mut self, id: &BlockId, title: &str) -> Result<(), VaultError> {
        let cmd = RenameHeadline::new(id.clone(), title.to_owned());
        self.apply_to_block(id, &cmd)
    }

    /// Insert a new sibling headline after `after` through the kernel
    /// [`AddSibling`] command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn add_sibling(&mut self, after: &BlockId, title: &str) -> Result<(), VaultError> {
        let cmd = AddSibling::new(after.clone(), title.to_owned());
        self.apply_to_block(after, &cmd)
    }

    /// The most recently cut subtree source, if the kill ring is
    /// non-empty.
    #[must_use]
    pub fn ring_top(&self) -> Option<&str> {
        self.kill_ring.last().map(String::as_str)
    }

    /// Push arbitrary text onto the kill ring.
    ///
    /// Not everything worth copying is a subtree — a sync ticket, a
    /// block id, a rendered table. They belong on the same ring as
    /// everything else, or a shell needs a second clipboard that
    /// nothing else can read.
    pub fn push_kill_ring(&mut self, text: String) {
        self.kill_ring.push(text);
    }

    /// Cut the subtree rooted at `id`: push its source onto the kill
    /// ring, then remove it (kernel [`RemoveSubtree`], undoable I3)
    /// and persist. Cut+paste is a move, so the id stays unique (I2).
    ///
    /// # Errors
    ///
    /// [`VaultError::Command`] for a missing block, otherwise the
    /// [`Self::remove_subtree`] contract.
    pub fn cut(&mut self, path: &Path, id: &BlockId) -> Result<(), VaultError> {
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        // Addressed by position, not by the `:ID:` in the text.
        // `OrgDoc::subtree_of` reads the property out of the source, so
        // a headline that has never been given a drawer — most of them,
        // in a file a person typed — could not be cut at all, while
        // every other operation addressed it fine. Both orders are
        // depth-first, which is what makes the index shared.
        let ix = doc
            .all_block_ids()
            .iter()
            .position(|b| b == id)
            .ok_or_else(|| VaultError::Command(format!("no headline {id}")))?;
        let source = doc
            .org()
            .headline_at_dfs_index(ix)
            .ok_or_else(|| VaultError::Command(format!("no headline {id}")))?
            .subtree_source()
            .to_owned();
        self.remove_subtree(id)?;
        self.kill_ring.push(source);
        Ok(())
    }

    /// Paste the kill-ring top after the subtree rooted at `after` in
    /// `path`, span-preserving. Not yet undoable (rewrite-based, like
    /// block edits).
    ///
    /// # Errors
    ///
    /// [`VaultError::Command`] for an empty ring, an unknown target,
    /// or a result that fails to parse; [`VaultError::Io`] on write.
    pub fn paste(&mut self, path: &Path, after: &BlockId) -> Result<(), VaultError> {
        let source = self
            .kill_ring
            .last()
            .cloned()
            .ok_or_else(|| VaultError::Command("kill ring is empty".into()))?;
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        let after_path = doc
            .path_of(after)
            .ok_or_else(|| VaultError::Command(format!("no headline {after}")))?;
        let new_org = closure_org::rewrite_splice_subtree_after(doc.org(), &after_path, &source)
            .map_err(|e| VaultError::Command(e.to_string()))?;
        let new_src = closure_org::print(&new_org);
        let new_doc =
            Document::load_str(&new_src).map_err(|_| VaultError::Parse { path: path.into() })?;
        fs::write(path, new_doc.source())?;
        self.documents.insert(path.to_path_buf(), new_doc);
        self.reindex_file(path);
        Ok(())
    }

    /// Mirror the kill-ring top *out* to an external clipboard (D7).
    ///
    /// A no-op when the ring is empty. Additive — the ring is unchanged,
    /// so `paste` still works whether or not a clipboard is wired.
    pub fn mirror_ring_top_to_clipboard(&self, clip: &mut dyn Clipboard) {
        if let Some(top) = self.ring_top() {
            clip.set(top);
        }
    }

    /// Pull external clipboard text *in*, pushing it onto the kill ring
    /// so the next `paste` inserts it (D7). A no-op when the clipboard is
    /// empty. Lets content from another app enter the vault through the
    /// same span-preserving paste path.
    pub fn pull_clipboard_to_ring(&mut self, clip: &dyn Clipboard) {
        if let Some(text) = clip.get() {
            self.kill_ring.push(text);
        }
    }

    /// Move headline `id`'s subtree to right after `after`'s subtree
    /// through the kernel [`MoveSubtree`] command (undoable, I3) and
    /// persist.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn move_after(&mut self, id: &BlockId, after: &BlockId) -> Result<(), VaultError> {
        let cmd = MoveSubtree::new(id.clone(), after.clone());
        self.apply_to_block(id, &cmd)
    }

    /// Promote a headline one level (fewer stars) through the kernel
    /// [`Promote`] command (undoable, I3) and persist.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn promote(&mut self, id: &BlockId) -> Result<(), VaultError> {
        let cmd = Promote::new(id.clone());
        self.apply_to_block(id, &cmd)
    }

    /// Demote a headline one level (more stars) through the kernel
    /// [`Demote`] command (undoable, I3) and persist.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn demote(&mut self, id: &BlockId) -> Result<(), VaultError> {
        let cmd = Demote::new(id.clone());
        self.apply_to_block(id, &cmd)
    }

    /// Replace a headline's body text through the kernel [`SetBody`]
    /// command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_body(&mut self, id: &BlockId, body: &str) -> Result<(), VaultError> {
        let cmd = SetBody::new(id.clone(), body.to_owned());
        self.apply_to_block(id, &cmd)
    }

    /// Set (or overwrite) a `:KEY: value` property through the kernel
    /// [`SetProperty`] command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_property(&mut self, id: &BlockId, key: &str, value: &str) -> Result<(), VaultError> {
        let cmd = SetProperty::new(id.clone(), key.to_owned(), value.to_owned());
        self.apply_to_block(id, &cmd)
    }

    /// Set (or, with `None`, clear) the TODO keyword through the kernel
    /// [`SetTodo`] command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_todo(&mut self, id: &BlockId, keyword: Option<&str>) -> Result<(), VaultError> {
        let cmd = SetTodo::new(id.clone(), keyword.map(ToOwned::to_owned));
        self.apply_to_block(id, &cmd)
    }

    /// Set (or, with `None`, clear) the priority cookie through the
    /// kernel [`SetPriority`] command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_priority(&mut self, id: &BlockId, priority: Option<char>) -> Result<(), VaultError> {
        let cmd = SetPriority::new(id.clone(), priority);
        self.apply_to_block(id, &cmd)
    }

    /// Start a clock on `id` — org's `org-clock-in`.
    ///
    /// `now` is `YYYY-MM-DD HH:MM`; the store reads no clock, for the
    /// same reason the date picker does not. The entry goes at the top
    /// of the headline's `:LOGBOOK:` drawer, which is created if it has
    /// none, and any clock running elsewhere is closed first: two open
    /// clocks are two answers to "what are you doing", and org keeps
    /// one.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for a headline that is not there, plus
    /// whatever the body write fails with.
    pub fn clock_in(&mut self, id: &BlockId, now: &str) -> Result<(), VaultError> {
        if let Some((running, _)) = self.running_clock() {
            let running = BlockId::from_existing(&running);
            self.clock_out(&running, now)?;
        }
        let body = self.body_of(id)?;
        let stamp = org_clock_stamp(now)?;
        let line = format!("CLOCK: [{stamp}]");
        let updated = if body.contains(":LOGBOOK:") {
            body.replacen(":LOGBOOK:\n", &format!(":LOGBOOK:\n{line}\n"), 1)
        } else {
            format!(":LOGBOOK:\n{line}\n:END:\n{body}")
        };
        self.set_body(id, &updated)
    }

    /// Close the clock running on `id` — org's `org-clock-out`.
    ///
    /// # Errors
    ///
    /// [`VaultError::Command`] when no clock is open on that headline.
    pub fn clock_out(&mut self, id: &BlockId, now: &str) -> Result<(), VaultError> {
        let body = self.body_of(id)?;
        let stamp = org_clock_stamp(now)?;
        let Some(open) = body
            .lines()
            .find(|l| l.trim_start().starts_with("CLOCK:") && !l.contains("--"))
        else {
            return Err(VaultError::Command("no clock is running here".into()));
        };
        let started = open
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(inside, _)| inside.to_owned())
            .ok_or_else(|| VaultError::Command("that clock entry has no start".into()))?;
        let minutes = clock_span_minutes(&started, &stamp)
            .ok_or_else(|| VaultError::Command("that clock entry has no readable start".into()))?;
        let closed = format!(
            "{}--[{stamp}] =>  {}:{:02}",
            open.trim_end(),
            minutes / 60,
            minutes % 60
        );
        let updated = body.replacen(open, &closed, 1);
        self.set_body(id, &updated)
    }

    /// Drop the open clock entry on `id` without recording any time —
    /// org's `org-clock-cancel`.
    ///
    /// # Errors
    ///
    /// [`VaultError::Command`] when no clock is open on that headline.
    pub fn clock_cancel(&mut self, id: &BlockId) -> Result<(), VaultError> {
        let body = self.body_of(id)?;
        let Some(open) = body
            .lines()
            .find(|l| l.trim_start().starts_with("CLOCK:") && !l.contains("--"))
        else {
            return Err(VaultError::Command("no clock is running here".into()));
        };
        let open_line = format!("{open}\n");
        let mut updated = body.replacen(&open_line, "", 1);
        // A logbook with nothing left in it is furniture.
        if updated.contains(":LOGBOOK:\n:END:\n") {
            updated = updated.replacen(":LOGBOOK:\n:END:\n", "", 1);
        }
        self.set_body(id, &updated)
    }

    /// The `(block id, start stamp)` of the one clock that is running,
    /// if any — what a status bar shows.
    #[must_use]
    pub fn running_clock(&self) -> Option<(String, String)> {
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                for line in h.body_text().lines() {
                    if line.trim_start().starts_with("CLOCK:") && !line.contains("--") {
                        let start = line
                            .split_once('[')
                            .and_then(|(_, rest)| rest.split_once(']'))
                            .map(|(inside, _)| inside.to_owned())?;
                        return Some((h.id().to_string(), start));
                    }
                }
            }
        }
        None
    }

    /// A headline's body text, or [`VaultError::UnknownId`].
    fn body_of(&self, id: &BlockId) -> Result<String, VaultError> {
        self.find_by_id(id)
            .map(|(h, _)| closure_org::unescape_body(h.body_text()))
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))
    }

    /// Move the subtree rooted at `id` under `target`, as its last
    /// child — org's `org-refile`, across files included.
    ///
    /// The subtree is re-levelled so its root sits one under the target
    /// and the shape inside it is kept, and it lands after whatever
    /// children the target already had. Ids are carried verbatim (I2):
    /// a refiled note is the same note, and every `id:` link into it
    /// still resolves.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when either end is missing, and
    /// [`VaultError::Command`] when the move would put a subtree inside
    /// itself — which would take the target with it.
    pub fn refile(&mut self, id: &BlockId, target: &BlockId) -> Result<(), VaultError> {
        if id == target {
            return Err(VaultError::Command(
                "a headline cannot be filed under itself".into(),
            ));
        }
        let (_, source_path) = self
            .find_by_id(id)
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        let source_path = source_path.to_path_buf();
        let (target_headline, target_path) = self
            .find_by_id(target)
            .ok_or_else(|| VaultError::UnknownId(target.as_str().to_owned()))?;
        let target_level = target_headline.level();
        let target_path = target_path.to_path_buf();
        let doc = self
            .documents
            .get(&source_path)
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        let subtree = doc
            .org()
            .subtree_of(id.as_str())
            .ok_or_else(|| VaultError::Command(format!("no headline {id}")))?
            .to_owned();
        // A subtree that contains the target would be filing a tree
        // under one of its own branches: the tree comes out of the file
        // with the target inside it, and nothing is left to file into.
        if source_path == target_path && subtree_contains(&subtree, target.as_str()) {
            return Err(VaultError::Command(
                "that target is inside the subtree being filed".into(),
            ));
        }
        let level = doc
            .headline_by_id(id)
            .map_or(1, closure_core::DocHeadline::level);
        let shifted =
            shift_subtree_levels(&subtree, i16::from(target_level + 1) - i16::from(level));

        self.remove_subtree(id)?;
        // Re-read the target: removing the subtree may have rewritten
        // the very file it lives in, so the path it had is stale.
        let doc = self
            .documents
            .get(&target_path)
            .ok_or_else(|| VaultError::UnknownId(target.as_str().to_owned()))?;
        let after_path = doc
            .path_of(target)
            .ok_or_else(|| VaultError::Command(format!("no headline {target}")))?;
        let new_org = closure_org::rewrite_splice_subtree_after(doc.org(), &after_path, &shifted)
            .map_err(|e| VaultError::Command(e.to_string()))?;
        let new_src = closure_org::print(&new_org);
        let new_doc = Document::load_str(&new_src).map_err(|_| VaultError::Parse {
            path: target_path.clone(),
        })?;
        fs::write(&target_path, new_doc.source())?;
        self.documents.insert(target_path.clone(), new_doc);
        self.reindex_file(&target_path);
        Ok(())
    }

    /// Move the subtree rooted at `id` into its file's archive sibling
    /// (`notes.org` → `notes.org_archive`), stamping where it came from
    /// and when — org's `org-archive-subtree`.
    ///
    /// `today` is the date to stamp (`YYYY-MM-DD`); the store reads no
    /// clock, for the same reason the date picker does not.
    ///
    /// Returns the archive file's path.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] when the headline is missing, plus the
    /// write/parse failures of the files involved.
    pub fn archive_subtree(&mut self, id: &BlockId, today: &str) -> Result<PathBuf, VaultError> {
        let (_, source_path) = self
            .find_by_id(id)
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        let source_path = source_path.to_path_buf();
        // Stamped *before* the move, so the properties travel with the
        // subtree rather than needing a second edit at the far end.
        let origin = source_path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.set_property(id, "ARCHIVE_TIME", today)?;
        self.set_property(id, "ARCHIVE_FILE", &origin)?;

        let doc = self
            .documents
            .get(&source_path)
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        let subtree = doc
            .org()
            .subtree_of(id.as_str())
            .ok_or_else(|| VaultError::Command(format!("no headline {id}")))?
            .to_owned();

        let mut archive_name = source_path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        archive_name.push_str("_archive");
        let archive_path = source_path.with_file_name(archive_name);

        let existing = self
            .documents
            .get(&archive_path)
            .map(closure_core::Document::source)
            .unwrap_or_default();
        let mut combined = existing;
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&subtree);

        self.remove_subtree(id)?;
        let archived = Document::load_str(&combined).map_err(|_| VaultError::Parse {
            path: archive_path.clone(),
        })?;
        fs::write(&archive_path, archived.source())?;
        self.documents.insert(archive_path.clone(), archived);
        self.reindex_file(&archive_path);
        Ok(archive_path)
    }

    /// Make sure `path` declares `keywords` with a `#+TODO:` line, so
    /// the keywords a vault is configured with are keywords the parser
    /// — and Emacs — actually reads.
    ///
    /// Org keeps this in the file rather than in a config, and so do we:
    /// a note written with `NEXT` is only a `NEXT` note in a file that
    /// says what `NEXT` is. The line is written once, at the top, and
    /// only when the file does not already declare every keyword; the
    /// last keyword goes after the `|`, which is org's way of saying
    /// "this one means finished".
    ///
    /// Returns whether the file was changed.
    ///
    /// # Errors
    ///
    /// Propagates the write/parse failures of [`Self::set_source`].
    pub fn ensure_todo_keywords(
        &mut self,
        path: &Path,
        keywords: &[String],
    ) -> Result<bool, VaultError> {
        if keywords.is_empty() {
            return Ok(false);
        }
        let Some(source) = self.documents.get(path).map(closure_core::Document::source) else {
            return Ok(false);
        };
        let declared = closure_org::declared_todo_keywords(&source);
        if keywords.iter().all(|k| declared.contains(k)) {
            return Ok(false);
        }
        let (unfinished, finished) = keywords.split_at(keywords.len() - 1);
        let line = format!(
            "#+TODO: {} | {}\n",
            unfinished.join(" "),
            finished.join(" ")
        );
        // Replace an existing declaration rather than stacking a second
        // one: two `#+TODO:` lines are two sequences in org, which is
        // not what a config change means.
        let mut rest = String::with_capacity(source.len());
        for l in source.lines() {
            let t = l.trim_start();
            if t.len() >= 7 && t[..7].eq_ignore_ascii_case("#+TODO:") {
                continue;
            }
            rest.push_str(l);
            rest.push('\n');
        }
        self.set_source(path, &format!("{line}{rest}"))?;
        Ok(true)
    }

    /// Set (or clear) the headline's planning timestamps through the
    /// kernel [`SetPlanning`] command (undoable, I3) and persist.
    ///
    /// Each field is replaced by what is passed: `None` clears it, so
    /// the caller reads the current triple first when it means to keep
    /// one. Stamps carry their own delimiters (`<2026-07-30 Thu>` or
    /// `[2026-07-30 Thu]`) — this writes what org would write, and does
    /// not invent brackets around whatever it is handed.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_planning(
        &mut self,
        id: &BlockId,
        scheduled: Option<&str>,
        deadline: Option<&str>,
        closed: Option<&str>,
    ) -> Result<(), VaultError> {
        let cmd = closure_core::SetPlanning::new(
            id.clone(),
            scheduled.map(ToOwned::to_owned),
            deadline.map(ToOwned::to_owned),
            closed.map(ToOwned::to_owned),
        );
        self.apply_to_block(id, &cmd)
    }

    /// Replace the headline's tag list through the kernel [`SetTags`]
    /// command (undoable, I3) and persist to disk. An empty slice clears
    /// all tags.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn set_tags(&mut self, id: &BlockId, tags: &[String]) -> Result<(), VaultError> {
        let cmd = SetTags::new(id.clone(), tags.to_vec());
        self.apply_to_block(id, &cmd)
    }

    /// Remove the subtree rooted at `id` through the kernel
    /// [`RemoveSubtree`] command (undoable, I3) and persist to disk.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::rename_headline`].
    pub fn remove_subtree(&mut self, id: &BlockId) -> Result<(), VaultError> {
        let cmd = RemoveSubtree::new(id.clone());
        self.apply_to_block(id, &cmd)
    }

    /// Execute one LLM-facing tool line and return a text result.
    ///
    /// Tools: `list-files`, `read <file>`, `search <text>`,
    /// `capture <title>`, `rename <id> <title>`,
    /// `set-property <id> <key> <value>`. Mutations route through the
    /// same kernel-command methods as every shell (I8); failures come
    /// back as `ERROR …` text, never panics.
    pub fn run_tool(&mut self, line: &str) -> String {
        const HELP: &str = "available tools: list-files | read <file> | search <text> | \
                            capture <title> | rename <id> <title> | \
                            set-property <id> <key> <value>";
        let line = line.trim();
        let (tool, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match tool {
            "view-state" => {
                let todos = self.all_todos();
                let tags = self.all_tags();
                format!(
                    "vault snapshot:\nfiles: {}\nheadlines: {}\nTODO keywords: {}\ntags: {}",
                    self.len(),
                    self.headline_count(),
                    if todos.is_empty() {
                        "(none)".to_owned()
                    } else {
                        todos.join(", ")
                    },
                    if tags.is_empty() {
                        "(none)".to_owned()
                    } else {
                        tags.join(", ")
                    },
                )
            }
            "list-files" => self
                .paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            "read" => self.document_relative(Path::new(rest)).map_or_else(
                || format!("ERROR no such file: {rest}"),
                closure_core::Document::source,
            ),
            "search" => {
                let needle = rest.to_lowercase();
                let mut out = String::new();
                for (path, doc) in self.iter() {
                    for h in doc.all_headlines() {
                        if h.title().to_lowercase().contains(&needle) {
                            let _ = writeln!(out, "{}\t{}\t{}", h.id(), h.title(), path.display());
                        }
                    }
                }
                out
            }
            "capture" if !rest.is_empty() => {
                let template = CaptureTemplate {
                    target: PathBuf::from("inbox.org"),
                    headline_prefix: "TODO ".to_owned(),
                    body: String::new(),
                };
                match self.capture(&template, rest) {
                    Ok(id) => format!("OK captured {id}"),
                    Err(e) => format!("ERROR {e}"),
                }
            }
            "rename" => {
                let Some((id, title)) = rest.split_once(' ') else {
                    return format!("ERROR rename needs <id> <title>; {HELP}");
                };
                match self.rename_headline(&BlockId::from_existing(id), title.trim()) {
                    Ok(()) => format!("OK renamed {id}"),
                    Err(e) => format!("ERROR {e}"),
                }
            }
            "set-property" => {
                let mut parts = rest.splitn(3, ' ');
                let (Some(id), Some(key), Some(value)) = (parts.next(), parts.next(), parts.next())
                else {
                    return format!("ERROR set-property needs <id> <key> <value>; {HELP}");
                };
                match self.set_property(&BlockId::from_existing(id), key, value) {
                    Ok(()) => format!("OK set {key}"),
                    Err(e) => format!("ERROR {e}"),
                }
            }
            _ => format!("ERROR unknown tool `{tool}`; {HELP}"),
        }
    }

    /// Evaluate the Nth doc-wide code block of `path` through
    /// closure-eval, honouring `:var` bindings and `:results silent`;
    /// non-silent stdout is attached as `#+RESULTS:` span-preserving
    /// (I1), persisted, and reindexed. Returns the block's stdout.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for unknown paths,
    /// [`VaultError::Command`] for missing blocks, unsupported
    /// languages, or evaluation failures, [`VaultError::Io`] on write
    /// failures.
    pub fn eval_block(&mut self, path: &Path, index: usize) -> Result<String, VaultError> {
        use closure_eval::Backend as _;
        let trust = self.eval_trust();
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        let org = doc.org();
        let blocks = org.code_blocks();
        let node = blocks.get(index).ok_or_else(|| {
            VaultError::Command(format!("no code block #{index} in {}", path.display()))
        })?;
        let cb = node
            .as_code_block()
            .ok_or_else(|| VaultError::Command("not a code block".into()))?;
        let lang = cb.language.unwrap_or("shell");
        if !closure_eval::eval_allowed(&trust, lang) {
            return Err(VaultError::EvalBlocked {
                lang: lang.to_owned(),
            });
        }
        let header = closure_eval::HeaderArgs::parse(cb.args.unwrap_or(""));
        let program = format!(
            "{}{}",
            closure_eval::var_prelude(lang, &header.vars),
            cb.content
        );
        let bounds = closure_eval::Bounds::default();
        let out = match lang {
            "shell" | "sh" | "bash" => closure_eval::ShellBackend.eval_bounded(&program, bounds),
            "python" => closure_eval::PythonBackend.eval_bounded(&program, bounds),
            #[cfg(feature = "wasmtime")]
            "wasm" => {
                use closure_eval::Backend as _;
                closure_eval::WasmBackend.eval(&program)
            }
            other => {
                return Err(VaultError::Command(format!("no backend for `{other}`")));
            }
        }
        .map_err(|e| VaultError::Command(e.to_string()))?;
        if !header.is_silent() {
            let new_org =
                closure_org::rewrite_attach_results_to_code_block(org, index, &out.stdout)
                    .map_err(|e| VaultError::Command(e.to_string()))?;
            let new_src = closure_org::print(&new_org);
            let new_doc = Document::load_str(&new_src)
                .map_err(|_| VaultError::Parse { path: path.into() })?;
            fs::write(path, new_doc.source())?;
            self.documents.insert(path.to_path_buf(), new_doc);
            self.reindex_file(path);
        }
        Ok(out.stdout)
    }

    /// Collect every SCHEDULED/DEADLINE headline across the vault as
    /// [`AgendaEntry`] rows, sorted by date then title. Dates are the
    /// `YYYY-MM-DD` prefix of the org timestamp (lexical sort = date
    /// order).
    #[must_use]
    pub fn agenda(&self) -> Vec<AgendaEntry> {
        let mut out: Vec<AgendaEntry> = Vec::new();
        for (path, doc) in self.iter() {
            for h in doc.all_headlines() {
                for (kind, ts) in [
                    (AgendaKind::Scheduled, h.scheduled()),
                    (AgendaKind::Deadline, h.deadline()),
                ] {
                    if let Some(date) = ts.and_then(agenda_date) {
                        out.push(AgendaEntry {
                            path: path.to_path_buf(),
                            id: h.id().to_string(),
                            title: h.title().to_owned(),
                            kind,
                            date,
                        });
                    }
                }
            }
        }
        out.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.title.cmp(&b.title)));
        out
    }

    /// Clocked minutes per headline across the vault (Q5-O3): every
    /// headline whose body carries closed `CLOCK:` intervals
    /// ([`closure_org::clock_entries`]), with the interval minutes
    /// summed — open clocks contribute nothing. Sorted
    /// minutes-descending then by title (I6). Read-only.
    #[must_use]
    pub fn clock_minutes(&self) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> = Vec::new();
        for (_, doc) in self.iter() {
            for h in doc.all_headlines() {
                let total: u64 = closure_org::clock_entries(h.body_text())
                    .iter()
                    .filter_map(|c| c.minutes)
                    .sum();
                if total > 0 {
                    out.push((h.title().to_owned(), total));
                }
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Agenda entries on or before `date` (`YYYY-MM-DD`), sorted.
    #[must_use]
    pub fn agenda_until(&self, date: &str) -> Vec<AgendaEntry> {
        self.agenda()
            .into_iter()
            .filter(|e| e.date.as_str() <= date)
            .collect()
    }

    /// Parse the `#+BEGIN_SRC closure-cron` block of `path` into
    /// scheduled jobs (`<cron-expr> <command line>` per line). Empty
    /// when the file has no such block.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for unknown paths,
    /// [`VaultError::Command`] for a malformed cron line.
    pub fn cron_jobs(&self, path: &Path) -> Result<Vec<closure_cron::Job>, VaultError> {
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        for node in doc.org().code_blocks() {
            if let Some(cb) = node.as_code_block()
                && cb.language == Some("closure-cron")
            {
                return closure_cron::parse_jobs(cb.content)
                    .map_err(|e| VaultError::Command(e.to_string()));
            }
        }
        Ok(Vec::new())
    }

    /// Tangle `path`: write each code block carrying `:tangle
    /// <target>` to that target (relative to the file's directory),
    /// concatenating blocks that share a target in source order.
    /// Returns the written target paths. Literate-programming export.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for unknown paths, [`VaultError::Io`]
    /// on write failures.
    pub fn tangle(&self, path: &Path) -> Result<Vec<PathBuf>, VaultError> {
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut bytarget: Vec<(PathBuf, String)> = Vec::new();
        for node in doc.org().code_blocks() {
            let Some(cb) = node.as_code_block() else {
                continue;
            };
            let header = closure_eval::HeaderArgs::parse(cb.args.unwrap_or(""));
            let Some(target) = header.tangle else {
                continue;
            };
            let abs = base.join(&target);
            match bytarget.iter_mut().find(|(p, _)| *p == abs) {
                Some((_, acc)) => acc.push_str(cb.content),
                None => bytarget.push((abs, cb.content.to_owned())),
            }
        }
        let mut written = Vec::with_capacity(bytarget.len());
        for (abs, content) in bytarget {
            fs::write(&abs, content)?;
            written.push(abs);
        }
        Ok(written)
    }

    /// Replace the content of the Nth doc-wide code block of `path`
    /// (fences preserved, I1), persist, and reindex. Backs
    /// org-edit-special.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for unknown paths,
    /// [`VaultError::Command`] for a missing block or a result that
    /// fails to parse, [`VaultError::Io`] on write failures.
    pub fn set_block_content(
        &mut self,
        path: &Path,
        index: usize,
        content: &str,
    ) -> Result<(), VaultError> {
        let doc = self
            .documents
            .get(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        let new_org = closure_org::rewrite_code_block_content(doc.org(), index, content)
            .map_err(|e| VaultError::Command(e.to_string()))?;
        let new_src = closure_org::print(&new_org);
        let new_doc =
            Document::load_str(&new_src).map_err(|_| VaultError::Parse { path: path.into() })?;
        fs::write(path, new_doc.source())?;
        self.documents.insert(path.to_path_buf(), new_doc);
        self.reindex_file(path);
        Ok(())
    }

    /// Undo the most recent edit in `path`'s document (undo-tree,
    /// I3), persist, and reindex.
    ///
    /// # Errors
    ///
    /// [`VaultError::UnknownId`] for unknown paths, [`VaultError::Undo`]
    /// when the history is empty, [`VaultError::Io`] on write failure.
    pub fn undo_in(&mut self, path: &Path) -> Result<(), VaultError> {
        let doc = self
            .documents
            .get_mut(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        doc.undo().map_err(|e| VaultError::Undo(e.to_string()))?;
        fs::write(path, doc.source())?;
        self.reindex_file(path);
        Ok(())
    }

    /// Apply one form-encoded command (Q9 / Q6-W1): the shared
    /// dispatch behind the web `/command` endpoint AND journal replay
    /// — `cmd=<name>&id=<block>&arg=<value>`, every arm the registry
    /// command path (I8, undoable I3).
    ///
    /// # Errors
    ///
    /// [`VaultError::Undo`] with an `unknown command` message for an
    /// unrecognised `cmd`; the underlying vault error otherwise.
    pub fn apply_form_command(&mut self, form: &str) -> Result<(), VaultError> {
        let param = |k: &str| {
            form.split('&')
                .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                .map(form_decode)
                .unwrap_or_default()
        };
        let (cmd, id, arg) = (param("cmd"), param("id"), param("arg"));
        let bid = closure_core::BlockId::from_existing(&id);
        match cmd.as_str() {
            "rename" => self.rename_headline(&bid, &arg),
            "set-todo" => self.set_todo(&bid, if arg.is_empty() { None } else { Some(&arg) }),
            "set-tags" => {
                let tags: Vec<String> = arg.split_whitespace().map(ToOwned::to_owned).collect();
                self.set_tags(&bid, &tags)
            }
            "set-body" => self.set_body(&bid, &arg),
            "add-sibling" => self.add_sibling(&bid, if arg.is_empty() { "untitled" } else { &arg }),
            "remove-subtree" | "delete" => self.remove_subtree(&bid),
            "toggle-todo" => {
                let next = match self.find_by_id(&bid).and_then(|(h, _)| h.todo()) {
                    Some(_) => None,
                    None => Some("TODO"),
                };
                self.set_todo(&bid, next)
            }
            "promote" => self.promote(&bid),
            "demote" => self.demote(&bid),
            "undo" | "redo" => match self.find_by_id(&bid).map(|(_, p)| p.to_path_buf()) {
                Some(p) if cmd == "undo" => self.undo_in(&p),
                Some(p) => self.redo_in(&p),
                None => Err(VaultError::UnknownId(id)),
            },
            other => Err(VaultError::Undo(format!("unknown command: {other}"))),
        }
    }

    /// Jump `path`'s undo cursor to the history node at `index`
    /// ([`closure_core::Document::jump_in_history`], Q2 — insertion
    /// order, the `history_view` row order), persist, and reindex.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::undo_in`].
    pub fn jump_history_in(&mut self, path: &Path, index: usize) -> Result<(), VaultError> {
        let doc = self
            .documents
            .get_mut(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        doc.jump_in_history(index)
            .map_err(|e| VaultError::Undo(e.to_string()))?;
        fs::write(path, doc.source())?;
        self.reindex_file(path);
        Ok(())
    }

    /// Re-apply the most recently undone edit in `path`'s document,
    /// persist, and reindex.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::undo_in`].
    pub fn redo_in(&mut self, path: &Path) -> Result<(), VaultError> {
        let doc = self
            .documents
            .get_mut(path)
            .ok_or_else(|| VaultError::UnknownId(path.display().to_string()))?;
        doc.redo(None)
            .map_err(|e| VaultError::Undo(e.to_string()))?;
        fs::write(path, doc.source())?;
        self.reindex_file(path);
        Ok(())
    }

    /// Apply a kernel command to the document containing `id`,
    /// persist it, and rebuild the file's id/backlink index entries.
    fn apply_to_block(&mut self, id: &BlockId, cmd: &dyn Command) -> Result<(), VaultError> {
        let path = self
            .by_id
            .get(id)
            .cloned()
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        let doc = self
            .documents
            .get_mut(&path)
            .ok_or_else(|| VaultError::UnknownId(id.as_str().to_owned()))?;
        cmd.apply(doc)
            .map_err(|e| VaultError::Command(e.to_string()))?;
        fs::write(&path, doc.source())?;
        self.reindex_file(&path);
        Ok(())
    }

    /// Drop and rebuild every id/backlink index entry derived from
    /// `path`'s document.
    fn reindex_file(&mut self, path: &Path) {
        // The single chokepoint every document mutation funnels
        // through (kernel commands, capture, eval, undo/redo, jump),
        // so the revision bump lives here rather than in each of the
        // twenty public mutators.
        self.revision = self.revision.wrapping_add(1);
        self.by_id.retain(|_, p| p != path);
        for v in self.backlinks.values_mut() {
            v.retain(|(p, _)| p != path);
        }
        self.backlinks.retain(|_, v| !v.is_empty());
        let mut harvested: Vec<(BlockId, Vec<String>)> = Vec::new();
        if let Some(doc) = self.documents.get(path) {
            for h in doc.all_headlines() {
                harvested.push((h.id().clone(), h.link_targets().to_vec()));
            }
        }
        for (id, links) in harvested {
            self.by_id.insert(id.clone(), path.to_path_buf());
            for link in links {
                if let Some(stripped) = link.strip_prefix("id:") {
                    self.backlinks
                        .entry(stripped.to_owned())
                        .or_default()
                        .push((path.to_path_buf(), id.clone()));
                }
                self.backlinks
                    .entry(link)
                    .or_default()
                    .push((path.to_path_buf(), id.clone()));
            }
        }
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
        // Sorted by path, deliberately. Every list the shells paint —
        // the outline, the block list, the database view, the agenda —
        // is derived by walking this, and the backing map's order is
        // the hash seed's, which differs per process. Without the sort
        // the same vault lists its headlines differently on every
        // launch, and "the row I want is third" never holds.
        let mut entries: Vec<(&Path, &Document)> = self
            .documents
            .iter()
            .map(|(p, d)| (p.as_path(), d))
            .collect();
        entries.sort_unstable_by_key(|(p, _)| *p);
        entries.into_iter()
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
        self.documents.get(path).map(|d| d.source().chars().count())
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
        self.documents
            .values()
            .map(|d| d.org().total_link_count())
            .sum()
    }

    /// Total link count for a single file by path. Returns `None` if the
    /// file isn't loaded.
    #[must_use]
    pub fn link_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.org().total_link_count())
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
        self.documents
            .values()
            .map(|d| d.org().total_timestamp_count())
            .sum()
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
        self.documents
            .values()
            .map(|d| d.org().total_cookie_count())
            .sum()
    }

    /// Total footnote count across the vault.
    #[must_use]
    pub fn footnote_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().total_footnote_count())
            .sum()
    }

    /// Total macro count across the vault.
    #[must_use]
    pub fn macro_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().total_macro_count())
            .sum()
    }

    /// Cookie count for a single file by path.
    #[must_use]
    pub fn cookie_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().total_cookie_count())
    }

    /// Footnote count for a single file by path.
    #[must_use]
    pub fn footnote_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().total_footnote_count())
    }

    /// Macro count for a single file by path.
    #[must_use]
    pub fn macro_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().total_macro_count())
    }

    /// Maximum per-file cookie count.
    #[must_use]
    pub fn max_file_cookie_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_cookie_count())
            .max()
    }

    /// Minimum per-file cookie count.
    #[must_use]
    pub fn min_file_cookie_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_cookie_count())
            .min()
    }

    /// Sum of per-file cookie counts.
    #[must_use]
    pub fn total_file_cookie_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().total_cookie_count())
            .sum()
    }

    /// Integer mean per-file cookie count.
    #[must_use]
    pub fn mean_file_cookie_count(&self) -> usize {
        self.total_file_cookie_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file cookie count.
    #[must_use]
    pub fn median_file_cookie_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().total_cookie_count())
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

    /// Histogram of per-file cookie counts.
    #[must_use]
    pub fn file_cookie_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().total_cookie_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file cookie count (lowest wins ties).
    #[must_use]
    pub fn mode_file_cookie_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_cookie_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Maximum per-file footnote count.
    #[must_use]
    pub fn max_file_footnote_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_footnote_count())
            .max()
    }

    /// Minimum per-file footnote count.
    #[must_use]
    pub fn min_file_footnote_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_footnote_count())
            .min()
    }

    /// Sum of per-file footnote counts.
    #[must_use]
    pub fn total_file_footnote_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().total_footnote_count())
            .sum()
    }

    /// Integer mean per-file footnote count.
    #[must_use]
    pub fn mean_file_footnote_count(&self) -> usize {
        self.total_file_footnote_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file footnote count.
    #[must_use]
    pub fn median_file_footnote_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().total_footnote_count())
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

    /// Histogram of per-file footnote counts.
    #[must_use]
    pub fn file_footnote_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().total_footnote_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file footnote count.
    #[must_use]
    pub fn mode_file_footnote_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (fc, c) in self.file_footnote_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((fc, c));
            }
        }
        best.map(|(fc, _)| fc)
    }

    /// Maximum per-file macro count.
    #[must_use]
    pub fn max_file_macro_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_macro_count())
            .max()
    }

    /// Minimum per-file macro count.
    #[must_use]
    pub fn min_file_macro_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().total_macro_count())
            .min()
    }

    /// Sum of per-file macro counts.
    #[must_use]
    pub fn total_file_macro_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().total_macro_count())
            .sum()
    }

    /// Integer mean per-file macro count.
    #[must_use]
    pub fn mean_file_macro_count(&self) -> usize {
        self.total_file_macro_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file macro count.
    #[must_use]
    pub fn median_file_macro_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().total_macro_count())
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

    /// Histogram of per-file macro counts.
    #[must_use]
    pub fn file_macro_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().total_macro_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file macro count.
    #[must_use]
    pub fn mode_file_macro_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (mc, c) in self.file_macro_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((mc, c));
            }
        }
        best.map(|(mc, _)| mc)
    }

    /// Count of files that contain at least one cookie.
    #[must_use]
    pub fn files_with_cookies(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().total_cookie_count() > 0)
            .count()
    }

    /// Count of files that contain at least one footnote.
    #[must_use]
    pub fn files_with_footnotes(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().total_footnote_count() > 0)
            .count()
    }

    /// Count of files that contain at least one macro.
    #[must_use]
    pub fn files_with_macros(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().total_macro_count() > 0)
            .count()
    }

    /// Percentage of files that contain at least one cookie (`0..=100`).
    #[must_use]
    pub fn files_with_cookies_pct(&self) -> usize {
        (self.files_with_cookies() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files that contain at least one footnote (`0..=100`).
    #[must_use]
    pub fn files_with_footnotes_pct(&self) -> usize {
        (self.files_with_footnotes() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files that contain at least one macro (`0..=100`).
    #[must_use]
    pub fn files_with_macros_pct(&self) -> usize {
        (self.files_with_macros() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files that contain at least one headline.
    #[must_use]
    pub fn files_with_headlines(&self) -> usize {
        self.documents
            .values()
            .filter(|d| !d.org().iter_headlines().is_empty())
            .count()
    }

    /// Percentage of files containing at least one headline (`0..=100`).
    #[must_use]
    pub fn files_with_headlines_pct(&self) -> usize {
        (self.files_with_headlines() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one headline with non-empty body.
    #[must_use]
    pub fn files_with_body(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| !h.body_text().is_empty()))
            .count()
    }

    /// Percentage of files containing at least one headline with non-empty body.
    #[must_use]
    pub fn files_with_body_pct(&self) -> usize {
        (self.files_with_body() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one headline with a property.
    #[must_use]
    pub fn files_with_properties(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| !h.properties().is_empty()))
            .count()
    }

    /// Percentage of files containing at least one headline with a property.
    #[must_use]
    pub fn files_with_properties_pct(&self) -> usize {
        (self.files_with_properties() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one link.
    #[must_use]
    pub fn files_with_links(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| !h.link_targets().is_empty()))
            .count()
    }

    /// Percentage of files containing at least one link (`0..=100`).
    #[must_use]
    pub fn files_with_links_pct(&self) -> usize {
        (self.files_with_links() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one timestamp.
    #[must_use]
    pub fn files_with_timestamps(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().total_timestamp_count() > 0)
            .count()
    }

    /// Percentage of files containing at least one timestamp (`0..=100`).
    #[must_use]
    pub fn files_with_timestamps_pct(&self) -> usize {
        (self.files_with_timestamps() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one tagged headline.
    #[must_use]
    pub fn files_with_tags(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| !h.tags().is_empty()))
            .count()
    }

    /// Percentage of files containing at least one tagged headline (`0..=100`).
    #[must_use]
    pub fn files_with_tags_pct(&self) -> usize {
        (self.files_with_tags() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one headline with a TODO keyword.
    #[must_use]
    pub fn files_with_todo(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| h.todo().is_some()))
            .count()
    }

    /// Percentage of files containing at least one TODO-marked headline (`0..=100`).
    #[must_use]
    pub fn files_with_todo_pct(&self) -> usize {
        (self.files_with_todo() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one prioritized headline.
    #[must_use]
    pub fn files_with_priority(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.all_headlines().any(|h| h.priority().is_some()))
            .count()
    }

    /// Percentage of files containing at least one prioritized headline (`0..=100`).
    #[must_use]
    pub fn files_with_priority_pct(&self) -> usize {
        (self.files_with_priority() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one `:ID:` property.
    #[must_use]
    pub fn files_with_id(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_with_id() > 0)
            .count()
    }

    /// Percentage of files containing at least one `:ID:` property (`0..=100`).
    #[must_use]
    pub fn files_with_id_pct(&self) -> usize {
        (self.files_with_id() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one archived headline.
    #[must_use]
    pub fn files_with_archived(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_archived() > 0)
            .count()
    }

    /// Percentage of files containing at least one archived headline (`0..=100`).
    #[must_use]
    pub fn files_with_archived_pct(&self) -> usize {
        (self.files_with_archived() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one SCHEDULED headline.
    #[must_use]
    pub fn files_with_scheduled(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_scheduled() > 0)
            .count()
    }

    /// Percentage of files containing at least one SCHEDULED headline (`0..=100`).
    #[must_use]
    pub fn files_with_scheduled_pct(&self) -> usize {
        (self.files_with_scheduled() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one DEADLINE headline.
    #[must_use]
    pub fn files_with_deadline(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_with_deadline() > 0)
            .count()
    }

    /// Percentage of files containing at least one DEADLINE headline (`0..=100`).
    #[must_use]
    pub fn files_with_deadline_pct(&self) -> usize {
        (self.files_with_deadline() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one CLOSED headline.
    #[must_use]
    pub fn files_with_closed(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_closed() > 0)
            .count()
    }

    /// Percentage of files containing at least one CLOSED headline (`0..=100`).
    #[must_use]
    pub fn files_with_closed_pct(&self) -> usize {
        (self.files_with_closed() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one headline with planning info.
    #[must_use]
    pub fn files_with_planning(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_with_planning() > 0)
            .count()
    }

    /// Percentage of files containing at least one headline with planning info.
    #[must_use]
    pub fn files_with_planning_pct(&self) -> usize {
        (self.files_with_planning() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing at least one COMMENT-prefixed headline.
    #[must_use]
    pub fn files_with_comment(&self) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_comments() > 0)
            .count()
    }

    /// Percentage of files containing at least one COMMENT-prefixed headline.
    #[must_use]
    pub fn files_with_comment_pct(&self) -> usize {
        (self.files_with_comment() * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// True iff any link appears across the vault.
    #[must_use]
    pub fn has_link(&self) -> bool {
        self.with_link_count() > 0
    }

    /// True iff any link appears (alias of [`Self::has_link`]).
    #[must_use]
    pub fn has_any_link(&self) -> bool {
        self.has_link()
    }

    /// True iff any timestamp appears (alias of [`Self::has_timestamp`]).
    #[must_use]
    pub fn has_any_timestamp(&self) -> bool {
        self.has_timestamp()
    }

    /// True iff any archived headline appears (alias of [`Self::has_archived`]).
    #[must_use]
    pub fn has_any_archived(&self) -> bool {
        self.has_archived()
    }

    /// True iff any SCHEDULED headline appears (alias of [`Self::has_scheduled`]).
    #[must_use]
    pub fn has_any_scheduled(&self) -> bool {
        self.has_scheduled()
    }

    /// True iff any DEADLINE headline appears (alias of [`Self::has_deadline`]).
    #[must_use]
    pub fn has_any_deadline(&self) -> bool {
        self.has_deadline()
    }

    /// True iff any CLOSED headline appears (alias of [`Self::has_closed`]).
    #[must_use]
    pub fn has_any_closed(&self) -> bool {
        self.has_closed()
    }

    /// True iff any COMMENT headline appears (alias of [`Self::has_comment`]).
    #[must_use]
    pub fn has_any_comment(&self) -> bool {
        self.has_comment()
    }

    /// True iff any planning info appears (alias of [`Self::has_planning`]).
    #[must_use]
    pub fn has_any_planning(&self) -> bool {
        self.has_planning()
    }

    /// True iff any footnote appears (alias of [`Self::has_footnote`]).
    #[must_use]
    pub fn has_any_footnote(&self) -> bool {
        self.has_footnote()
    }

    /// True iff any macro appears (alias of [`Self::has_macro`]).
    #[must_use]
    pub fn has_any_macro(&self) -> bool {
        self.has_macro()
    }

    /// True iff any cookie appears (alias of [`Self::has_cookie`]).
    #[must_use]
    pub fn has_any_cookie(&self) -> bool {
        self.has_cookie()
    }

    /// Count of tag occurrences across the vault (alias of [`Self::total_tag_count`]).
    #[must_use]
    pub fn count_tags(&self) -> usize {
        self.total_tag_count()
    }

    /// Count of link-target occurrences (alias of [`Self::link_count`]).
    #[must_use]
    pub fn count_links(&self) -> usize {
        self.link_count()
    }

    /// Count of timestamp occurrences (alias of [`Self::timestamp_count`]).
    #[must_use]
    pub fn count_timestamps(&self) -> usize {
        self.timestamp_count()
    }

    /// Count of headlines across the vault (alias of [`Self::headline_count`]).
    #[must_use]
    pub fn count_headlines(&self) -> usize {
        self.headline_count()
    }

    /// Count of files (alias of [`Self::len`]).
    #[must_use]
    pub fn count_files(&self) -> usize {
        self.len()
    }

    /// Total word count across the vault (alias of [`Self::word_count`]).
    #[must_use]
    pub fn count_words(&self) -> usize {
        self.word_count()
    }

    /// Total byte count across the vault (alias of [`Self::byte_count`]).
    #[must_use]
    pub fn count_bytes(&self) -> usize {
        self.byte_count()
    }

    /// Total character count across the vault (alias of [`Self::char_count`]).
    #[must_use]
    pub fn count_chars(&self) -> usize {
        self.char_count()
    }

    /// Total source line count across the vault (alias of [`Self::line_count`]).
    #[must_use]
    pub fn count_lines(&self) -> usize {
        self.line_count()
    }

    /// Count of paths (alias of [`Self::path_count`]).
    #[must_use]
    pub fn count_paths(&self) -> usize {
        self.path_count()
    }

    /// Count of headlines with `:ID:` (alias of [`Self::id_count`]).
    #[must_use]
    pub fn with_id_count(&self) -> usize {
        self.id_count()
    }

    /// Count of archived headlines (alias of [`Self::archived_count`]).
    #[must_use]
    pub fn with_archived_count(&self) -> usize {
        self.archived_count()
    }

    /// Count of SCHEDULED headlines (alias of [`Self::scheduled_count`]).
    #[must_use]
    pub fn with_scheduled_count(&self) -> usize {
        self.scheduled_count()
    }

    /// Count of DEADLINE headlines (alias of [`Self::deadline_count`]).
    #[must_use]
    pub fn with_deadline_count(&self) -> usize {
        self.deadline_count()
    }

    /// Count of CLOSED headlines (alias of [`Self::closed_count`]).
    #[must_use]
    pub fn with_closed_count(&self) -> usize {
        self.closed_count()
    }

    /// Count of COMMENT headlines (alias of [`Self::comment_count`]).
    #[must_use]
    pub fn with_comment_count(&self) -> usize {
        self.comment_count()
    }

    /// True iff any timestamp appears across the vault.
    #[must_use]
    pub fn has_timestamp(&self) -> bool {
        self.timestamp_count() > 0
    }

    /// True iff any archived headline appears across the vault.
    #[must_use]
    pub fn has_archived(&self) -> bool {
        self.archived_count() > 0
    }

    /// True iff any COMMENT headline appears across the vault.
    #[must_use]
    pub fn has_comment(&self) -> bool {
        self.comment_count() > 0
    }

    /// True iff any prioritized headline appears across the vault.
    #[must_use]
    pub fn has_any_priority(&self) -> bool {
        self.with_priority_count() > 0
    }

    /// True iff any TODO-marked headline appears across the vault.
    #[must_use]
    pub fn has_any_todo(&self) -> bool {
        self.with_todo_count() > 0
    }

    /// True iff any headline with planning info appears across the vault.
    #[must_use]
    pub fn has_planning(&self) -> bool {
        self.planning_count() > 0
    }

    /// True iff any headline with an `:ID:` property appears across the vault.
    #[must_use]
    pub fn has_any_id(&self) -> bool {
        self.id_count() > 0
    }

    /// True iff any cookie appears across the vault.
    #[must_use]
    pub fn has_cookie(&self) -> bool {
        self.cookie_count() > 0
    }

    /// True iff any footnote appears across the vault.
    #[must_use]
    pub fn has_footnote(&self) -> bool {
        self.footnote_count() > 0
    }

    /// True iff any macro appears across the vault.
    #[must_use]
    pub fn has_macro(&self) -> bool {
        self.macro_count() > 0
    }

    /// True iff any SCHEDULED headline appears across the vault.
    #[must_use]
    pub fn has_scheduled(&self) -> bool {
        self.scheduled_count() > 0
    }

    /// True iff any DEADLINE headline appears across the vault.
    #[must_use]
    pub fn has_deadline(&self) -> bool {
        self.deadline_count() > 0
    }

    /// True iff any CLOSED headline appears across the vault.
    #[must_use]
    pub fn has_closed(&self) -> bool {
        self.closed_count() > 0
    }

    /// True iff any tagged headline appears across the vault.
    #[must_use]
    pub fn has_any_tag(&self) -> bool {
        self.tagged_count() > 0
    }

    /// True iff any property-carrying headline appears across the vault.
    #[must_use]
    pub fn has_any_property(&self) -> bool {
        self.with_property_count() > 0
    }

    /// Count of headlines at the given level across the vault.
    #[must_use]
    pub fn count_at_level(&self, level: u8) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_at_level(level))
            .sum()
    }

    /// Count of headlines tagged with `tag` across the vault.
    #[must_use]
    pub fn count_tagged(&self, tag: &str) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_tagged(tag))
            .sum()
    }

    /// Count of headlines with TODO `keyword` across the vault.
    #[must_use]
    pub fn count_todo(&self, keyword: &str) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_todo(keyword))
            .sum()
    }

    /// Count of headlines whose title contains `needle` across the vault.
    #[must_use]
    pub fn count_title_contains(&self, needle: &str) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_title_contains(needle))
            .sum()
    }

    /// True iff any headline carries priority `letter` across the vault.
    #[must_use]
    pub fn has_priority_letter(&self, letter: char) -> bool {
        self.documents
            .values()
            .any(|d| d.org().has_priority(letter))
    }

    /// True iff any headline carries TODO `keyword` across the vault.
    #[must_use]
    pub fn has_todo_keyword(&self, keyword: &str) -> bool {
        self.count_todo(keyword) > 0
    }

    /// True iff any headline is tagged `tag` across the vault.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.count_tagged(tag) > 0
    }

    /// True iff any headline carries property `key` across the vault.
    #[must_use]
    pub fn has_property_key(&self, key: &str) -> bool {
        self.documents
            .values()
            .any(|d| d.all_headlines().any(|h| h.property(key).is_some()))
    }

    /// True iff any headline carries `:ID:` equal to `value` across the vault.
    #[must_use]
    pub fn has_id_value(&self, value: &str) -> bool {
        self.documents
            .values()
            .any(|d| d.all_headlines().any(|h| h.property("ID") == Some(value)))
    }

    /// True iff any headline exists at the given level across the vault.
    #[must_use]
    pub fn has_level(&self, level: u8) -> bool {
        self.count_at_level(level) > 0
    }

    /// True iff any headline title contains `needle` across the vault.
    #[must_use]
    pub fn has_title_contains(&self, needle: &str) -> bool {
        self.count_title_contains(needle) > 0
    }

    /// True iff any headline carries a link to `target` across the vault.
    #[must_use]
    pub fn has_link_target(&self, target: &str) -> bool {
        self.documents.values().any(|d| {
            d.all_headlines()
                .any(|h| h.link_targets().iter().any(|t| t == target))
        })
    }

    /// True iff any headline title exactly equals `title` across the vault.
    #[must_use]
    pub fn has_title_exact(&self, title: &str) -> bool {
        self.documents
            .values()
            .any(|d| d.all_headlines().any(|h| h.title() == title))
    }

    /// Count of files containing a headline tagged `tag`.
    #[must_use]
    pub fn files_with_tag(&self, tag: &str) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_tagged(tag) > 0)
            .count()
    }

    /// Count of files containing a headline with TODO `keyword`.
    #[must_use]
    pub fn files_with_todo_keyword(&self, keyword: &str) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_todo(keyword) > 0)
            .count()
    }

    /// Count of files containing a headline with priority `letter`.
    #[must_use]
    pub fn files_with_priority_letter(&self, letter: char) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().has_priority(letter))
            .count()
    }

    /// Count of files containing a headline with property `key`.
    #[must_use]
    pub fn files_with_property_key(&self, key: &str) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().has_property_key(key))
            .count()
    }

    /// Count of files containing a headline at the given level.
    #[must_use]
    pub fn files_with_level(&self, level: u8) -> usize {
        self.documents
            .values()
            .filter(|d| d.org().count_at_level(level) > 0)
            .count()
    }

    /// Percentage of files containing a headline tagged `tag` (`0..=100`).
    #[must_use]
    pub fn files_with_tag_pct(&self, tag: &str) -> usize {
        (self.files_with_tag(tag) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files containing a headline with TODO `keyword` (`0..=100`).
    #[must_use]
    pub fn files_with_todo_keyword_pct(&self, keyword: &str) -> usize {
        (self.files_with_todo_keyword(keyword) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files containing a headline with priority `letter` (`0..=100`).
    #[must_use]
    pub fn files_with_priority_letter_pct(&self, letter: char) -> usize {
        (self.files_with_priority_letter(letter) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files containing a headline with property `key` (`0..=100`).
    #[must_use]
    pub fn files_with_property_key_pct(&self, key: &str) -> usize {
        (self.files_with_property_key(key) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files containing a headline at the given level (`0..=100`).
    #[must_use]
    pub fn files_with_level_pct(&self, level: u8) -> usize {
        (self.files_with_level(level) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Paths of files containing a headline with property `key`.
    #[must_use]
    pub fn paths_with_property_key(&self, key: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.org().has_property_key(key))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Paths of files containing a headline whose title contains `needle`.
    #[must_use]
    pub fn paths_with_title_contains(&self, needle: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.org().has_title_contains(needle))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Paths of files containing a headline whose title equals `title`.
    #[must_use]
    pub fn paths_with_title_exact(&self, title: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.org().has_title_exact(title))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Paths of files containing a link to `target`.
    #[must_use]
    pub fn paths_with_link_target(&self, target: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.org().has_link_target(target))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Paths of files containing a headline with `:ID:` equal to `value`.
    #[must_use]
    pub fn paths_with_id_value(&self, value: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.org().has_id_value(value))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Count of files containing a headline whose title contains `needle`.
    #[must_use]
    pub fn files_with_title_contains(&self, needle: &str) -> usize {
        self.paths_with_title_contains(needle).len()
    }

    /// Percentage of files containing a headline whose title contains `needle`.
    #[must_use]
    pub fn files_with_title_contains_pct(&self, needle: &str) -> usize {
        (self.files_with_title_contains(needle) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing a link to `target`.
    #[must_use]
    pub fn files_with_link_target(&self, target: &str) -> usize {
        self.paths_with_link_target(target).len()
    }

    /// Percentage of files containing a link to `target` (`0..=100`).
    #[must_use]
    pub fn files_with_link_target_pct(&self, target: &str) -> usize {
        (self.files_with_link_target(target) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of files containing a headline whose title equals `title`.
    #[must_use]
    pub fn files_with_title_exact(&self, title: &str) -> usize {
        self.paths_with_title_exact(title).len()
    }

    /// Count of files containing a headline with `:ID:` equal to `value`.
    #[must_use]
    pub fn files_with_id_value(&self, value: &str) -> usize {
        self.paths_with_id_value(value).len()
    }

    /// Percentage of files containing a headline whose title equals `title` (`0..=100`).
    #[must_use]
    pub fn files_with_title_exact_pct(&self, title: &str) -> usize {
        (self.files_with_title_exact(title) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Percentage of files containing a headline with `:ID:` equal to `value` (`0..=100`).
    #[must_use]
    pub fn files_with_id_value_pct(&self, value: &str) -> usize {
        (self.files_with_id_value(value) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of headlines whose title exactly equals `title` across the vault.
    #[must_use]
    pub fn count_title_exact(&self, title: &str) -> usize {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.title() == title).count())
            .sum()
    }

    /// Count of link-target occurrences equal to `target` across the vault.
    #[must_use]
    pub fn count_link_target(&self, target: &str) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.link_targets().iter().filter(|t| *t == target).count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Count of headlines with `:ID:` equal to `value` across the vault.
    #[must_use]
    pub fn count_id_value(&self, value: &str) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| h.property("ID") == Some(value))
                    .count()
            })
            .sum()
    }

    /// Count of headlines carrying property `key` across the vault.
    #[must_use]
    pub fn count_property_key(&self, key: &str) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| h.property(key).is_some())
                    .count()
            })
            .sum()
    }

    /// Percentage of headlines tagged `tag` (`0..=100`).
    #[must_use]
    pub fn tag_pct(&self, tag: &str) -> usize {
        (self.count_tagged(tag) * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with TODO `keyword` (`0..=100`).
    #[must_use]
    pub fn todo_keyword_pct(&self, keyword: &str) -> usize {
        (self.count_todo(keyword) * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with priority `letter` (`0..=100`).
    #[must_use]
    pub fn priority_letter_pct(&self, letter: char) -> usize {
        let count: usize = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| h.priority() == Some(letter))
                    .count()
            })
            .sum();
        (count * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines at the given level (`0..=100`).
    #[must_use]
    pub fn level_pct(&self, level: u8) -> usize {
        (self.count_at_level(level) * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of distinct TODO keywords across the vault.
    #[must_use]
    pub fn distinct_todo_keyword_count(&self) -> usize {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                if let Some(t) = h.todo() {
                    s.insert(t.to_owned());
                }
            }
        }
        s.len()
    }

    /// Count of distinct priority letters across the vault.
    #[must_use]
    pub fn distinct_priority_letter_count(&self) -> usize {
        let mut s: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                if let Some(p) = h.priority() {
                    s.insert(p);
                }
            }
        }
        s.len()
    }

    /// Sorted distinct TODO-keywords across the vault.
    #[must_use]
    pub fn distinct_todo_keywords(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                if let Some(t) = h.todo() {
                    s.insert(t.to_owned());
                }
            }
        }
        s.into_iter().collect()
    }

    /// Sorted distinct priority letters across the vault.
    #[must_use]
    pub fn distinct_priority_letters(&self) -> Vec<char> {
        let mut s: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                if let Some(p) = h.priority() {
                    s.insert(p);
                }
            }
        }
        s.into_iter().collect()
    }

    /// All priority letters across the vault (with duplicates).
    #[must_use]
    pub fn all_priorities(&self) -> Vec<char> {
        let mut out = Vec::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                if let Some(p) = h.priority() {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Source text for a single file by path.
    #[must_use]
    pub fn source_of(&self, path: &Path) -> Option<String> {
        self.documents.get(path).map(closure_core::Document::source)
    }

    /// Concatenation of all source texts across the vault.
    #[must_use]
    pub fn total_source(&self) -> String {
        let mut out = String::new();
        for d in self.documents.values() {
            out.push_str(&d.source());
        }
        out
    }

    /// Total source line count across the vault.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.source().lines().count())
            .sum()
    }

    /// Source line count for a single file by path.
    #[must_use]
    pub fn line_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| d.source().lines().count())
    }

    /// Histogram of headline-title frequency across the vault.
    #[must_use]
    pub fn title_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                *m.entry(h.title().to_owned()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most frequently appearing headline title (lowest name wins ties).
    #[must_use]
    pub fn most_common_title(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (t, c) in self.title_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((t, c));
            }
        }
        best.map(|(t, _)| t)
    }

    /// Least frequently appearing headline title (lowest name wins ties).
    #[must_use]
    pub fn least_common_title(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (t, c) in self.title_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c < *bc) {
                best = Some((t, c));
            }
        }
        best.map(|(t, _)| t)
    }

    /// All headline body texts across the vault.
    #[must_use]
    pub fn all_bodies(&self) -> Vec<String> {
        let mut out = Vec::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                out.push(h.body_text().to_owned());
            }
        }
        out
    }

    /// Concatenation of all headline body texts across the vault.
    #[must_use]
    pub fn total_body_text(&self) -> String {
        let mut out = String::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                out.push_str(h.body_text());
            }
        }
        out
    }

    /// Count of headlines with at least one link across the vault.
    #[must_use]
    pub fn count_with_link(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_link())
            .sum()
    }

    /// Count of headlines with at least one property across the vault.
    #[must_use]
    pub fn count_with_property(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_property())
            .sum()
    }

    /// Count of headlines with at least one timestamp across the vault.
    #[must_use]
    pub fn count_with_timestamp(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_timestamp())
            .sum()
    }

    /// Count of headlines with any TODO keyword set across the vault.
    #[must_use]
    pub fn count_with_todo(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_todo())
            .sum()
    }

    /// Percentage of headlines with a priority cookie (`0..=100`).
    #[must_use]
    pub fn with_priority_pct(&self) -> usize {
        (self.with_priority_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with at least one property (`0..=100`).
    #[must_use]
    pub fn with_property_pct(&self) -> usize {
        (self.with_property_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with `:ID:` property (`0..=100`).
    #[must_use]
    pub fn with_id_pct(&self) -> usize {
        (self.id_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of SCHEDULED headlines across the vault (alias of `scheduled_count`).
    #[must_use]
    pub fn count_scheduled(&self) -> usize {
        self.scheduled_count()
    }

    /// Count of archived headlines across the vault (alias of `archived_count`).
    #[must_use]
    pub fn count_archived(&self) -> usize {
        self.archived_count()
    }

    /// Count of COMMENT-prefixed headlines across the vault (alias of `comment_count`).
    #[must_use]
    pub fn count_comments(&self) -> usize {
        self.comment_count()
    }

    /// Count of headlines with DEADLINE timestamp (alias of `deadline_count`).
    #[must_use]
    pub fn count_with_deadline(&self) -> usize {
        self.deadline_count()
    }

    /// Count of CLOSED headlines (alias of `closed_count`).
    #[must_use]
    pub fn count_closed(&self) -> usize {
        self.closed_count()
    }

    /// Count of headlines with planning info (alias of `planning_count`).
    #[must_use]
    pub fn count_with_planning(&self) -> usize {
        self.planning_count()
    }

    /// Count of headlines with `:ID:` (alias of `id_count`).
    #[must_use]
    pub fn count_with_id(&self) -> usize {
        self.id_count()
    }

    /// Count of headlines with priority cookie (alias of `with_priority_count`).
    #[must_use]
    pub fn count_with_priority(&self) -> usize {
        self.with_priority_count()
    }

    /// True iff any headline carries property `key` equal to `value`.
    #[must_use]
    pub fn has_property_value(&self, key: &str, value: &str) -> bool {
        self.documents
            .values()
            .any(|d| d.all_headlines().any(|h| h.property(key) == Some(value)))
    }

    /// Count of headlines carrying property `key` equal to `value`.
    #[must_use]
    pub fn count_property_value(&self, key: &str, value: &str) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| h.property(key) == Some(value))
                    .count()
            })
            .sum()
    }

    /// Paths of files containing a headline with property `key` equal to `value`.
    #[must_use]
    pub fn paths_with_property_value(&self, key: &str, value: &str) -> Vec<&Path> {
        self.documents
            .iter()
            .filter(|(_, d)| d.all_headlines().any(|h| h.property(key) == Some(value)))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// True iff `path` is loaded in the vault.
    #[must_use]
    pub fn has_path(&self, path: &Path) -> bool {
        self.documents.contains_key(path)
    }

    /// True iff `needle` appears in any document's source across the vault.
    #[must_use]
    pub fn contains_text(&self, needle: &str) -> bool {
        self.documents.values().any(|d| d.source().contains(needle))
    }

    /// Count of `needle` occurrences across all document sources.
    #[must_use]
    pub fn count_text(&self, needle: &str) -> usize {
        if needle.is_empty() {
            return 0;
        }
        self.documents
            .values()
            .map(|d| d.source().matches(needle).count())
            .sum()
    }

    /// Count of files whose source contains `needle`.
    #[must_use]
    pub fn files_containing(&self, needle: &str) -> usize {
        self.documents
            .values()
            .filter(|d| d.source().contains(needle))
            .count()
    }

    /// Percentage of files whose source contains `needle` (`0..=100`).
    #[must_use]
    pub fn files_containing_pct(&self, needle: &str) -> usize {
        (self.files_containing(needle) * 100)
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Count of paths containing `needle` in any headline title.
    #[must_use]
    pub fn paths_containing_count(&self, needle: &str) -> usize {
        self.paths_containing(needle).len()
    }

    /// Count of paths containing `needle` (case-insensitive) in any headline title.
    #[must_use]
    pub fn paths_containing_ignore_case_count(&self, needle: &str) -> usize {
        self.paths_containing_ignore_case(needle).len()
    }

    /// Count of headlines with an `:ID:` property across the vault.
    #[must_use]
    pub fn id_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_id())
            .sum()
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
        self.with_priority_count()
            .checked_div(self.len())
            .unwrap_or(0)
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
        let mut pairs: Vec<(&Path, usize)> =
            self.iter().map(|(p, d)| (p, d.source().len())).collect();
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
        let total: usize = self.iter().map(|(_, d)| d.org().all_ids().len()).sum();
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
        self.documents.get(path).map(|d| {
            d.org()
                .iter_headlines()
                .into_iter()
                .filter(|h| h.is_leaf())
                .count()
        })
    }

    /// Maximum per-file leaf count (`None` when no files).
    #[must_use]
    pub fn max_file_leaf_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.is_leaf())
                    .count()
            })
            .max()
    }

    /// Minimum per-file leaf count (`None` when no files).
    #[must_use]
    pub fn min_file_leaf_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.is_leaf())
                    .count()
            })
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
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| h.is_leaf())
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
        self.documents.get(path).map(|d| {
            d.org()
                .iter_headlines()
                .into_iter()
                .filter(|h| !h.is_leaf())
                .count()
        })
    }

    /// Maximum per-file branch count (`None` when no files).
    #[must_use]
    pub fn max_file_branch_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| !h.is_leaf())
                    .count()
            })
            .max()
    }

    /// Minimum per-file branch count (`None` when no files).
    #[must_use]
    pub fn min_file_branch_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| !h.is_leaf())
                    .count()
            })
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
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .filter(|h| !h.is_leaf())
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
            .filter(|(_, d)| {
                d.all_headlines()
                    .any(|h| h.tags().iter().any(|t| t == "ARCHIVE"))
            })
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

    /// Histogram of per-file tag occurrence counts.
    #[must_use]
    pub fn file_tag_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d.all_headlines().map(|h| h.tags().len()).sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file tag occurrence count (lowest wins ties).
    #[must_use]
    pub fn mode_file_tag_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_tag_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Histogram of per-file link counts.
    #[must_use]
    pub fn file_link_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d.all_headlines().map(|h| h.link_targets().len()).sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file link count (lowest wins ties).
    #[must_use]
    pub fn mode_file_link_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_link_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Histogram of per-file timestamp counts.
    #[must_use]
    pub fn file_timestamp_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().total_timestamp_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file timestamp count (lowest wins ties).
    #[must_use]
    pub fn mode_file_timestamp_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_timestamp_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Histogram of per-file headline counts.
    #[must_use]
    pub fn file_headline_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().iter_headlines().len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_headline_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (hc, c) in self.file_headline_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((hc, c));
            }
        }
        best.map(|(hc, _)| hc)
    }

    /// Histogram of per-file non-empty-body headline counts.
    #[must_use]
    pub fn file_with_body_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .all_headlines()
                .filter(|h| !h.body_text().is_empty())
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file non-empty-body headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_with_body_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.file_with_body_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Histogram of per-file property-carrying headline counts.
    #[must_use]
    pub fn file_with_property_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .all_headlines()
                .filter(|h| !h.properties().is_empty())
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file property-carrying headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_with_property_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_with_property_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Histogram of per-file word counts.
    #[must_use]
    pub fn file_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.source().split_whitespace().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file word count (lowest wins ties).
    #[must_use]
    pub fn mode_file_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.file_word_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Histogram of per-file byte counts.
    #[must_use]
    pub fn file_byte_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.source().len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file byte count (lowest wins ties).
    #[must_use]
    pub fn mode_file_byte_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.file_byte_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Histogram of per-file char counts.
    #[must_use]
    pub fn file_char_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.source().chars().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file char count (lowest wins ties).
    #[must_use]
    pub fn mode_file_char_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_char_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Histogram of per-file `:ID:`-carrying headline counts.
    #[must_use]
    pub fn file_id_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_with_id()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file `:ID:`-carrying headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_id_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (ic, c) in self.file_id_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((ic, c));
            }
        }
        best.map(|(ic, _)| ic)
    }

    /// Histogram of per-file link-carrying headline counts.
    #[must_use]
    pub fn file_with_link_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .all_headlines()
                .filter(|h| !h.link_targets().is_empty())
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file link-carrying headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_with_link_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_with_link_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Histogram of per-file timestamp-carrying headline counts.
    #[must_use]
    pub fn file_with_timestamp_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d
                .org()
                .iter_headlines()
                .into_iter()
                .filter(|h| h.timestamp_count() > 0)
                .count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file timestamp-carrying headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_with_timestamp_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_with_timestamp_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Histogram of per-file body byte counts.
    #[must_use]
    pub fn file_body_byte_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d.all_headlines().map(|h| h.body_text().len()).sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file body byte count (lowest wins ties).
    #[must_use]
    pub fn mode_file_body_byte_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.file_body_byte_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Histogram of per-file body line counts.
    #[must_use]
    pub fn file_body_line_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.body_text().lines().count())
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file body line count (lowest wins ties).
    #[must_use]
    pub fn mode_file_body_line_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_body_line_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Histogram of per-file body char counts.
    #[must_use]
    pub fn file_body_char_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.body_text().chars().count())
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file body char count (lowest wins ties).
    #[must_use]
    pub fn mode_file_body_char_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_body_char_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Histogram of per-file body word counts.
    #[must_use]
    pub fn file_body_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.body_text().split_whitespace().count())
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file body word count (lowest wins ties).
    #[must_use]
    pub fn mode_file_body_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.file_body_word_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Maximum per-file summed title byte length (`None` when no files).
    #[must_use]
    pub fn max_file_title_byte_len(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.title().len()).sum())
            .max()
    }

    /// Minimum per-file summed title byte length (`None` when no files).
    #[must_use]
    pub fn min_file_title_byte_len(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.title().len()).sum())
            .min()
    }

    /// Sum of per-file summed title byte lengths.
    #[must_use]
    pub fn total_file_title_byte_len(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.title().len()).sum::<usize>())
            .sum()
    }

    /// Integer mean per-file summed title byte length (`0` when no files).
    #[must_use]
    pub fn mean_file_title_byte_len(&self) -> usize {
        self.total_file_title_byte_len()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed title byte length (`None` when no files).
    #[must_use]
    pub fn median_file_title_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.all_headlines().map(|h| h.title().len()).sum())
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

    /// Histogram of per-file summed title byte lengths.
    #[must_use]
    pub fn file_title_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d.all_headlines().map(|h| h.title().len()).sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed title byte length (lowest wins ties).
    #[must_use]
    pub fn mode_file_title_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.file_title_byte_len_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Maximum per-file summed title word count (`None` when no files).
    #[must_use]
    pub fn max_file_title_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.title().split_whitespace().count())
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed title word count (`None` when no files).
    #[must_use]
    pub fn min_file_title_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.title().split_whitespace().count())
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed title word counts.
    #[must_use]
    pub fn total_file_title_word_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.title().split_whitespace().count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed title word count (`0` when no files).
    #[must_use]
    pub fn mean_file_title_word_count(&self) -> usize {
        self.total_file_title_word_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed title word count (`None` when no files).
    #[must_use]
    pub fn median_file_title_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.title().split_whitespace().count())
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

    /// Histogram of per-file summed title word counts.
    #[must_use]
    pub fn file_title_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.title().split_whitespace().count())
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed title word count (lowest wins ties).
    #[must_use]
    pub fn mode_file_title_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.file_title_word_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Maximum per-file summed headline descendant count (`None` when no files).
    #[must_use]
    pub fn max_file_descendant_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::descendant_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline descendant count (`None` when no files).
    #[must_use]
    pub fn min_file_descendant_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::descendant_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline descendant counts.
    #[must_use]
    pub fn total_file_descendant_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::descendant_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline descendant count (`0` when no files).
    #[must_use]
    pub fn mean_file_descendant_count(&self) -> usize {
        self.total_file_descendant_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline descendant count (`None` when no files).
    #[must_use]
    pub fn median_file_descendant_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::descendant_count)
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

    /// Histogram of per-file summed headline descendant counts.
    #[must_use]
    pub fn file_descendant_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::descendant_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline descendant count (lowest wins ties).
    #[must_use]
    pub fn mode_file_descendant_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (dc, c) in self.file_descendant_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((dc, c));
            }
        }
        best.map(|(dc, _)| dc)
    }

    /// Maximum per-file summed headline subtree size (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_size(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_size)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree size (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_size(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_size)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree sizes.
    #[must_use]
    pub fn total_file_subtree_size(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_size)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree size (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_size(&self) -> usize {
        self.total_file_subtree_size()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree size (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_size(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_size)
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

    /// Histogram of per-file summed headline subtree sizes.
    #[must_use]
    pub fn file_subtree_size_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_size)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree size (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_size(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (sz, c) in self.file_subtree_size_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((sz, c));
            }
        }
        best.map(|(sz, _)| sz)
    }

    /// Maximum per-file headline-level peak (`None` when no files).
    #[must_use]
    pub fn max_file_max_level(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .max()
                    .unwrap_or(0)
            })
            .max()
    }

    /// Minimum per-file headline-level peak (`None` when no files).
    #[must_use]
    pub fn min_file_max_level(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .max()
                    .unwrap_or(0)
            })
            .min()
    }

    /// Sum of per-file headline-level peaks.
    #[must_use]
    pub fn total_file_max_level(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .max()
                    .unwrap_or(0)
            })
            .sum()
    }

    /// Integer mean per-file headline-level peak (`0` when no files).
    #[must_use]
    pub fn mean_file_max_level(&self) -> usize {
        self.total_file_max_level()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file headline-level peak (`None` when no files).
    #[must_use]
    pub fn median_file_max_level(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .max()
                    .unwrap_or(0)
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

    /// Histogram of per-file headline-level peaks.
    #[must_use]
    pub fn file_max_level_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.level() as usize)
                .max()
                .unwrap_or(0);
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file headline-level peak (lowest wins ties).
    #[must_use]
    pub fn mode_file_max_level(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lvl, c) in self.file_max_level_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lvl, c));
            }
        }
        best.map(|(lvl, _)| lvl)
    }

    /// Maximum per-file headline-level floor (`None` when no files).
    #[must_use]
    pub fn max_file_min_level(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .min()
                    .unwrap_or(0)
            })
            .max()
    }

    /// Minimum per-file headline-level floor (`None` when no files).
    #[must_use]
    pub fn min_file_min_level(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .min()
                    .unwrap_or(0)
            })
            .min()
    }

    /// Sum of per-file headline-level floors.
    #[must_use]
    pub fn total_file_min_level(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .min()
                    .unwrap_or(0)
            })
            .sum()
    }

    /// Integer mean per-file headline-level floor (`0` when no files).
    #[must_use]
    pub fn mean_file_min_level(&self) -> usize {
        self.total_file_min_level()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file headline-level floor (`None` when no files).
    #[must_use]
    pub fn median_file_min_level(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.level() as usize)
                    .min()
                    .unwrap_or(0)
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

    /// Histogram of per-file headline-level floors.
    #[must_use]
    pub fn file_min_level_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .all_headlines()
                .map(|h| h.level() as usize)
                .min()
                .unwrap_or(0);
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file headline-level floor (lowest wins ties).
    #[must_use]
    pub fn mode_file_min_level(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lvl, c) in self.file_min_level_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lvl, c));
            }
        }
        best.map(|(lvl, _)| lvl)
    }

    /// Maximum per-file distinct tag count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_tags().len())
            .max()
    }

    /// Minimum per-file distinct tag count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_tags().len())
            .min()
    }

    /// Sum of per-file distinct tag counts.
    #[must_use]
    pub fn total_file_distinct_tag_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().distinct_tags().len())
            .sum()
    }

    /// Integer mean per-file distinct tag count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_tag_count(&self) -> usize {
        self.total_file_distinct_tag_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct tag count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_tag_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().distinct_tags().len())
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

    /// Histogram of per-file distinct tag counts.
    #[must_use]
    pub fn file_distinct_tag_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().distinct_tags().len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct tag count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_tag_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_distinct_tag_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-file distinct property-key count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_property_key_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_property_keys().len())
            .max()
    }

    /// Minimum per-file distinct property-key count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_property_key_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_property_keys().len())
            .min()
    }

    /// Sum of per-file distinct property-key counts.
    #[must_use]
    pub fn total_file_distinct_property_key_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().distinct_property_keys().len())
            .sum()
    }

    /// Integer mean per-file distinct property-key count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_property_key_count(&self) -> usize {
        self.total_file_distinct_property_key_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct property-key count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_property_key_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().distinct_property_keys().len())
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

    /// Histogram of per-file distinct property-key counts.
    #[must_use]
    pub fn file_distinct_property_key_count_counts(
        &self,
    ) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().distinct_property_keys().len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct property-key count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_property_key_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_distinct_property_key_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Histogram of per-file archived-headline counts.
    #[must_use]
    pub fn file_archived_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_archived()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file archived-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_archived_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (ac, c) in self.file_archived_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((ac, c));
            }
        }
        best.map(|(ac, _)| ac)
    }

    /// Maximum per-file distinct TODO-keyword count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_todo_count())
            .max()
    }

    /// Minimum per-file distinct TODO-keyword count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_todo_count())
            .min()
    }

    /// Sum of per-file distinct TODO-keyword counts.
    #[must_use]
    pub fn total_file_distinct_todo_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().distinct_todo_count())
            .sum()
    }

    /// Integer mean per-file distinct TODO-keyword count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_todo_count(&self) -> usize {
        self.total_file_distinct_todo_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct TODO-keyword count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_todo_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().distinct_todo_count())
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

    /// Histogram of per-file distinct TODO-keyword counts.
    #[must_use]
    pub fn file_distinct_todo_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().distinct_todo_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct TODO-keyword count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_todo_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_distinct_todo_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-file distinct priority count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_priority_count())
            .max()
    }

    /// Minimum per-file distinct priority count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_priority_count())
            .min()
    }

    /// Sum of per-file distinct priority counts.
    #[must_use]
    pub fn total_file_distinct_priority_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().distinct_priority_count())
            .sum()
    }

    /// Integer mean per-file distinct priority count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_priority_count(&self) -> usize {
        self.total_file_distinct_priority_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct priority count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_priority_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().distinct_priority_count())
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

    /// Histogram of per-file distinct priority counts.
    #[must_use]
    pub fn file_distinct_priority_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().distinct_priority_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct priority count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_priority_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_distinct_priority_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-file distinct headline-level count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_level_count())
            .max()
    }

    /// Minimum per-file distinct headline-level count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().distinct_level_count())
            .min()
    }

    /// Sum of per-file distinct headline-level counts.
    #[must_use]
    pub fn total_file_distinct_level_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().distinct_level_count())
            .sum()
    }

    /// Integer mean per-file distinct headline-level count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_level_count(&self) -> usize {
        self.total_file_distinct_level_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct headline-level count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_level_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().distinct_level_count())
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

    /// Histogram of per-file distinct headline-level counts.
    #[must_use]
    pub fn file_distinct_level_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().distinct_level_count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct headline-level count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_level_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_distinct_level_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Maximum per-file distinct `:ID:` value count (`None` when no files).
    #[must_use]
    pub fn max_file_distinct_id_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for h in d.all_headlines() {
                    if let Some(id) = h.property("ID") {
                        s.insert(id.to_owned());
                    }
                }
                s.len()
            })
            .max()
    }

    /// Minimum per-file distinct `:ID:` value count (`None` when no files).
    #[must_use]
    pub fn min_file_distinct_id_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for h in d.all_headlines() {
                    if let Some(id) = h.property("ID") {
                        s.insert(id.to_owned());
                    }
                }
                s.len()
            })
            .min()
    }

    /// Sum of per-file distinct `:ID:` value counts.
    #[must_use]
    pub fn total_file_distinct_id_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for h in d.all_headlines() {
                    if let Some(id) = h.property("ID") {
                        s.insert(id.to_owned());
                    }
                }
                s.len()
            })
            .sum()
    }

    /// Integer mean per-file distinct `:ID:` value count (`0` when no files).
    #[must_use]
    pub fn mean_file_distinct_id_count(&self) -> usize {
        self.total_file_distinct_id_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file distinct `:ID:` value count (`None` when no files).
    #[must_use]
    pub fn median_file_distinct_id_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for h in d.all_headlines() {
                    if let Some(id) = h.property("ID") {
                        s.insert(id.to_owned());
                    }
                }
                s.len()
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

    /// Histogram of per-file distinct `:ID:` value counts.
    #[must_use]
    pub fn file_distinct_id_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in d.all_headlines() {
                if let Some(id) = h.property("ID") {
                    s.insert(id.to_owned());
                }
            }
            *m.entry(s.len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file distinct `:ID:` value count (lowest wins ties).
    #[must_use]
    pub fn mode_file_distinct_id_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (ic, c) in self.file_distinct_id_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((ic, c));
            }
        }
        best.map(|(ic, _)| ic)
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
        self.with_timestamp_count()
            .checked_div(self.len())
            .unwrap_or(0)
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

    /// Count of headlines with any TODO keyword set, across the vault.
    #[must_use]
    pub fn with_todo_count(&self) -> usize {
        self.iter()
            .flat_map(|(_, d)| d.all_headlines())
            .filter(|h| h.todo().is_some())
            .count()
    }

    /// Count of TODO-marked headlines for a single file by path. Returns
    /// `None` if the file isn't loaded.
    #[must_use]
    pub fn with_todo_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
    }

    /// Maximum per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
            .max()
    }

    /// Minimum per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.all_headlines().filter(|h| h.todo().is_some()).count())
            .min()
    }

    /// Integer mean per-file TODO-marked headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_todo_count(&self) -> usize {
        self.with_todo_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file TODO-marked headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_todo_count(&self) -> Option<usize> {
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
    pub fn file_with_todo_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c = d.all_headlines().filter(|h| h.todo().is_some()).count();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file TODO-marked headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_with_todo_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_with_todo_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Histogram of per-file SCHEDULED-headline counts.
    #[must_use]
    pub fn file_scheduled_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_scheduled()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file SCHEDULED-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_scheduled_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (sc, c) in self.file_scheduled_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((sc, c));
            }
        }
        best.map(|(sc, _)| sc)
    }

    /// Histogram of per-file DEADLINE-headline counts.
    #[must_use]
    pub fn file_deadline_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_with_deadline()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file DEADLINE-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_deadline_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (dc, c) in self.file_deadline_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((dc, c));
            }
        }
        best.map(|(dc, _)| dc)
    }

    /// Histogram of per-file CLOSED-headline counts.
    #[must_use]
    pub fn file_closed_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_closed()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file CLOSED-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_closed_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_closed_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Histogram of per-file COMMENT-headline counts.
    #[must_use]
    pub fn file_comment_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_comments()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file COMMENT-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_comment_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_comment_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Count of headlines with planning info across the vault.
    #[must_use]
    pub fn planning_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_with_planning())
            .sum()
    }

    /// Count of headlines with planning info for a single file by path.
    /// Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn planning_count_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .get(path)
            .map(|d| d.org().count_with_planning())
    }

    /// Maximum per-file planning-headline count (`None` when no files).
    #[must_use]
    pub fn max_file_planning_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_planning())
            .max()
    }

    /// Minimum per-file planning-headline count (`None` when no files).
    #[must_use]
    pub fn min_file_planning_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().count_with_planning())
            .min()
    }

    /// Integer mean per-file planning-headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_planning_count(&self) -> usize {
        self.planning_count().checked_div(self.len()).unwrap_or(0)
    }

    /// Median per-file planning-headline count (`None` when no files).
    #[must_use]
    pub fn median_file_planning_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().count_with_planning())
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

    /// Histogram of per-file planning-headline counts.
    #[must_use]
    pub fn file_planning_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().count_with_planning()).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file planning-headline count (lowest wins ties).
    #[must_use]
    pub fn mode_file_planning_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_planning_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-file subtree depth (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_depth(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().max_subtree_depth().unwrap_or(0))
            .max()
    }

    /// Minimum per-file subtree depth (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_depth(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| d.org().max_subtree_depth().unwrap_or(0))
            .min()
    }

    /// Sum of per-file subtree depths.
    #[must_use]
    pub fn total_file_subtree_depth(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().max_subtree_depth().unwrap_or(0))
            .sum()
    }

    /// Integer mean per-file subtree depth (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_depth(&self) -> usize {
        self.total_file_subtree_depth()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file subtree depth (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_depth(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| d.org().max_subtree_depth().unwrap_or(0))
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

    /// Histogram of per-file subtree depths.
    #[must_use]
    pub fn file_subtree_depth_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            *m.entry(d.org().max_subtree_depth().unwrap_or(0))
                .or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file subtree depth (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_depth(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (d, c) in self.file_subtree_depth_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((d, c));
            }
        }
        best.map(|(d, _)| d)
    }

    /// Percentage of headlines with at least one link (`0..=100`).
    #[must_use]
    pub fn with_link_pct(&self) -> usize {
        (self.with_link_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with at least one timestamp (`0..=100`).
    #[must_use]
    pub fn with_timestamp_pct(&self) -> usize {
        (self.with_timestamp_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with a TODO keyword (`0..=100`).
    #[must_use]
    pub fn with_todo_pct(&self) -> usize {
        (self.with_todo_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines without any link across the vault.
    #[must_use]
    pub fn count_no_link(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_no_link())
            .sum()
    }

    /// Count of headlines without any timestamp across the vault.
    #[must_use]
    pub fn count_no_timestamp(&self) -> usize {
        self.documents
            .values()
            .map(|d| d.org().count_no_timestamp())
            .sum()
    }

    /// Percentage of headlines without links (`0..=100`).
    #[must_use]
    pub fn no_link_pct(&self) -> usize {
        (self.count_no_link() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines without timestamps (`0..=100`).
    #[must_use]
    pub fn no_timestamp_pct(&self) -> usize {
        (self.count_no_timestamp() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of non-archived headlines across the vault.
    #[must_use]
    pub fn count_no_archived(&self) -> usize {
        self.headline_count() - self.archived_count()
    }

    /// Percentage of non-archived headlines (`0..=100`).
    #[must_use]
    pub fn no_archived_pct(&self) -> usize {
        (self.count_no_archived() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of non-scheduled headlines across the vault.
    #[must_use]
    pub fn count_no_scheduled(&self) -> usize {
        self.headline_count() - self.scheduled_count()
    }

    /// Percentage of non-scheduled headlines (`0..=100`).
    #[must_use]
    pub fn no_scheduled_pct(&self) -> usize {
        (self.count_no_scheduled() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of non-COMMENT headlines across the vault.
    #[must_use]
    pub fn count_no_comment(&self) -> usize {
        self.headline_count() - self.comment_count()
    }

    /// Percentage of non-COMMENT headlines (`0..=100`).
    #[must_use]
    pub fn no_comment_pct(&self) -> usize {
        (self.count_no_comment() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines without planning info across the vault.
    #[must_use]
    pub fn count_no_planning(&self) -> usize {
        self.headline_count() - self.planning_count()
    }

    /// Percentage of headlines without planning info (`0..=100`).
    #[must_use]
    pub fn no_planning_pct(&self) -> usize {
        (self.count_no_planning() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of non-CLOSED headlines across the vault.
    #[must_use]
    pub fn count_no_closed(&self) -> usize {
        self.headline_count() - self.closed_count()
    }

    /// Percentage of non-CLOSED headlines (`0..=100`).
    #[must_use]
    pub fn no_closed_pct(&self) -> usize {
        (self.count_no_closed() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines that are roots (`0..=100`).
    #[must_use]
    pub fn root_pct(&self) -> usize {
        (self.root_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Percentage of headlines with planning info (`0..=100`).
    #[must_use]
    pub fn planning_pct(&self) -> usize {
        (self.planning_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Maximum per-file summed headline subtree word count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_word_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree word count (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_word_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_word_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree word counts.
    #[must_use]
    pub fn total_file_subtree_word_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_word_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree word count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_word_count(&self) -> usize {
        self.total_file_subtree_word_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree word count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_word_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_word_count)
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

    /// Histogram of per-file summed headline subtree word counts.
    #[must_use]
    pub fn file_subtree_word_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_word_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree word count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_word_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (wc, c) in self.file_subtree_word_count_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((wc, c));
            }
        }
        best.map(|(wc, _)| wc)
    }

    /// Maximum per-file summed headline subtree byte count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_byte_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree byte count (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_byte_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_byte_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree byte counts.
    #[must_use]
    pub fn total_file_subtree_byte_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_byte_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree byte count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_byte_count(&self) -> usize {
        self.total_file_subtree_byte_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree byte count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_byte_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_byte_count)
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

    /// Histogram of per-file summed headline subtree byte counts.
    #[must_use]
    pub fn file_subtree_byte_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_byte_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree byte count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_byte_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (bc, c) in self.file_subtree_byte_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((bc, c));
            }
        }
        best.map(|(bc, _)| bc)
    }

    /// Maximum per-file summed headline subtree link count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_link_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree link count (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_link_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree link counts.
    #[must_use]
    pub fn total_file_subtree_link_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_link_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree link count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_link_count(&self) -> usize {
        self.total_file_subtree_link_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree link count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_link_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_link_count)
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

    /// Histogram of per-file summed headline subtree link counts.
    #[must_use]
    pub fn file_subtree_link_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_link_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree link count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_link_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_subtree_link_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Maximum per-file summed headline subtree tag count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_tag_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree tag count (`None` when no files).
    #[must_use]
    pub fn min_file_subtree_tag_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_tag_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree tag counts.
    #[must_use]
    pub fn total_file_subtree_tag_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_tag_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree tag count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_tag_count(&self) -> usize {
        self.total_file_subtree_tag_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree tag count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_tag_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_tag_count)
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

    /// Histogram of per-file summed headline subtree tag counts.
    #[must_use]
    pub fn file_subtree_tag_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_tag_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree tag count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_tag_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_subtree_tag_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-file summed headline subtree TODO count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_todo_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree TODO count.
    #[must_use]
    pub fn min_file_subtree_todo_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_todo_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree TODO counts.
    #[must_use]
    pub fn total_file_subtree_todo_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_todo_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree TODO count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_todo_count(&self) -> usize {
        self.total_file_subtree_todo_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree TODO count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_todo_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_todo_count)
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

    /// Histogram of per-file summed headline subtree TODO counts.
    #[must_use]
    pub fn file_subtree_todo_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_todo_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree TODO count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_todo_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_subtree_todo_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-file summed headline subtree priority count (`None` when no files).
    #[must_use]
    pub fn max_file_subtree_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_priority_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree priority count.
    #[must_use]
    pub fn min_file_subtree_priority_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_priority_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree priority counts.
    #[must_use]
    pub fn total_file_subtree_priority_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_priority_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree priority count (`0` when no files).
    #[must_use]
    pub fn mean_file_subtree_priority_count(&self) -> usize {
        self.total_file_subtree_priority_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree priority count (`None` when no files).
    #[must_use]
    pub fn median_file_subtree_priority_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_priority_count)
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

    /// Histogram of per-file summed headline subtree priority counts.
    #[must_use]
    pub fn file_subtree_priority_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_priority_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree priority count (lowest wins ties).
    #[must_use]
    pub fn mode_file_subtree_priority_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_subtree_priority_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-file summed headline subtree property count.
    #[must_use]
    pub fn max_file_subtree_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_property_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree property count.
    #[must_use]
    pub fn min_file_subtree_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_property_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree property counts.
    #[must_use]
    pub fn total_file_subtree_property_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_property_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree property count.
    #[must_use]
    pub fn mean_file_subtree_property_count(&self) -> usize {
        self.total_file_subtree_property_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree property count.
    #[must_use]
    pub fn median_file_subtree_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_property_count)
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

    /// Histogram of per-file summed headline subtree property counts.
    #[must_use]
    pub fn file_subtree_property_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_property_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree property count.
    #[must_use]
    pub fn mode_file_subtree_property_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (pc, c) in self.file_subtree_property_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((pc, c));
            }
        }
        best.map(|(pc, _)| pc)
    }

    /// Maximum per-file summed headline subtree timestamp count.
    #[must_use]
    pub fn max_file_subtree_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_timestamp_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree timestamp count.
    #[must_use]
    pub fn min_file_subtree_timestamp_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_timestamp_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree timestamp counts.
    #[must_use]
    pub fn total_file_subtree_timestamp_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_timestamp_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree timestamp count.
    #[must_use]
    pub fn mean_file_subtree_timestamp_count(&self) -> usize {
        self.total_file_subtree_timestamp_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree timestamp count.
    #[must_use]
    pub fn median_file_subtree_timestamp_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_timestamp_count)
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

    /// Histogram of per-file summed headline subtree timestamp counts.
    #[must_use]
    pub fn file_subtree_timestamp_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_timestamp_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree timestamp count.
    #[must_use]
    pub fn mode_file_subtree_timestamp_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (tc, c) in self.file_subtree_timestamp_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((tc, c));
            }
        }
        best.map(|(tc, _)| tc)
    }

    /// Maximum per-file summed headline subtree level count.
    #[must_use]
    pub fn max_file_subtree_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_level_count)
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline subtree level count.
    #[must_use]
    pub fn min_file_subtree_level_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_level_count)
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline subtree level counts.
    #[must_use]
    pub fn total_file_subtree_level_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_level_count)
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline subtree level count.
    #[must_use]
    pub fn mean_file_subtree_level_count(&self) -> usize {
        self.total_file_subtree_level_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline subtree level count.
    #[must_use]
    pub fn median_file_subtree_level_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(closure_org::Headline::subtree_level_count)
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

    /// Histogram of per-file summed headline subtree level counts.
    #[must_use]
    pub fn file_subtree_level_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(closure_org::Headline::subtree_level_count)
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline subtree level count.
    #[must_use]
    pub fn mode_file_subtree_level_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (lc, c) in self.file_subtree_level_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((lc, c));
            }
        }
        best.map(|(lc, _)| lc)
    }

    /// Percentage of headlines with a non-empty body (`0..=100`).
    #[must_use]
    pub fn with_body_pct(&self) -> usize {
        (self.with_body_count() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Count of headlines with an empty body across the vault.
    #[must_use]
    pub fn count_no_body(&self) -> usize {
        self.headline_count() - self.with_body_count()
    }

    /// Percentage of headlines with an empty body (`0..=100`).
    #[must_use]
    pub fn no_body_pct(&self) -> usize {
        (self.count_no_body() * 100)
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Maximum per-file summed headline child count (`None` when no files).
    #[must_use]
    pub fn max_file_child_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(|h| h.children().len())
                    .sum()
            })
            .max()
    }

    /// Minimum per-file summed headline child count.
    #[must_use]
    pub fn min_file_child_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(|h| h.children().len())
                    .sum()
            })
            .min()
    }

    /// Sum of per-file summed headline child counts.
    #[must_use]
    pub fn total_file_child_count(&self) -> usize {
        self.documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(|h| h.children().len())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Integer mean per-file summed headline child count.
    #[must_use]
    pub fn mean_file_child_count(&self) -> usize {
        self.total_file_child_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file summed headline child count.
    #[must_use]
    pub fn median_file_child_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.org()
                    .iter_headlines()
                    .into_iter()
                    .map(|h| h.children().len())
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

    /// Histogram of per-file summed headline child counts.
    #[must_use]
    pub fn file_child_count_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            let c: usize = d
                .org()
                .iter_headlines()
                .into_iter()
                .map(|h| h.children().len())
                .sum();
            *m.entry(c).or_insert(0) += 1;
        }
        m
    }

    /// Most common per-file summed headline child count.
    #[must_use]
    pub fn mode_file_child_count(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.file_child_count_counts() {
            if best.is_none_or(|(_, bestc)| c > bestc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Sorted distinct link-target strings across the vault.
    #[must_use]
    pub fn distinct_link_targets(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                for t in h.link_targets() {
                    s.insert(t.clone());
                }
            }
        }
        s.into_iter().collect()
    }

    /// Count of distinct link-target strings across the vault.
    #[must_use]
    pub fn distinct_link_target_count(&self) -> usize {
        self.distinct_link_targets().len()
    }

    /// All link-target strings across the vault (with duplicates).
    #[must_use]
    pub fn all_link_targets(&self) -> Vec<String> {
        let mut out = Vec::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                for t in h.link_targets() {
                    out.push(t.clone());
                }
            }
        }
        out
    }

    /// Histogram of link-target frequency across the vault.
    #[must_use]
    pub fn link_target_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for d in self.documents.values() {
            for h in d.all_headlines() {
                for t in h.link_targets() {
                    *m.entry(t.clone()).or_insert(0) += 1;
                }
            }
        }
        m
    }

    /// Most frequently appearing link-target across the vault (lowest name wins ties).
    #[must_use]
    pub fn most_common_link_target(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (t, c) in self.link_target_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((t, c));
            }
        }
        best.map(|(t, _)| t)
    }

    /// Least frequently appearing link-target (lowest name wins ties).
    #[must_use]
    pub fn least_common_link_target(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (t, c) in self.link_target_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c < *bc) {
                best = Some((t, c));
            }
        }
        best.map(|(t, _)| t)
    }

    /// Count of headlines carrying at least one link for a single file
    /// by path. Returns `None` if the file isn't loaded.
    #[must_use]
    pub fn with_link_count_of(&self, path: &Path) -> Option<usize> {
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .filter(|h| !h.link_targets().is_empty())
                .count()
        })
    }

    /// Maximum per-file link-carrying headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.link_targets().is_empty())
                    .count()
            })
            .max()
    }

    /// Minimum per-file link-carrying headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_link_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.link_targets().is_empty())
                    .count()
            })
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
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.link_targets().is_empty())
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
        self.total_subtree_level_count().checked_div(n).unwrap_or(0)
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
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .filter(|h| !h.body_text().is_empty())
                .count()
        })
    }

    /// Maximum per-file non-empty-body headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_body_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.body_text().is_empty())
                    .count()
            })
            .max()
    }

    /// Minimum per-file non-empty-body headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_body_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.body_text().is_empty())
                    .count()
            })
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
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.body_text().is_empty())
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
        let n = self.iter().flat_map(|(_, d)| d.all_headlines()).count();
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
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .map(|h| h.body_text().lines().count())
                .sum()
        })
    }

    /// Maximum per-file body line count (`None` when no files).
    #[must_use]
    pub fn max_file_body_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().lines().count())
                    .sum()
            })
            .max()
    }

    /// Minimum per-file body line count (`None` when no files).
    #[must_use]
    pub fn min_file_body_line_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().lines().count())
                    .sum()
            })
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
            .map(|d| {
                d.all_headlines()
                    .map(|h| h.body_text().lines().count())
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
        self.documents.get(path).map(|d| {
            d.all_headlines()
                .filter(|h| !h.properties().is_empty())
                .count()
        })
    }

    /// Maximum per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn max_file_with_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.properties().is_empty())
                    .count()
            })
            .max()
    }

    /// Minimum per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn min_file_with_property_count(&self) -> Option<usize> {
        self.documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.properties().is_empty())
                    .count()
            })
            .min()
    }

    /// Integer mean per-file property-carrying headline count (`0` when no files).
    #[must_use]
    pub fn mean_file_with_property_count(&self) -> usize {
        self.with_property_count()
            .checked_div(self.len())
            .unwrap_or(0)
    }

    /// Median per-file property-carrying headline count (`None` when no files).
    #[must_use]
    pub fn median_file_with_property_count(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .documents
            .values()
            .map(|d| {
                d.all_headlines()
                    .filter(|h| !h.properties().is_empty())
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
        let mut counts: Vec<usize> = self
            .iter()
            .map(|(_, d)| d.all_headlines().count())
            .collect();
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

    /// Replace a loaded file's whole source — what a full-window editor
    /// over the file itself saves.
    ///
    /// Every other mutation is a kernel command against one block,
    /// which is what keeps ids stable. This one hands the user the
    /// text, so the promises are the ones the text can keep: the bytes
    /// written are exactly the bytes given (I1 — the parse is a check,
    /// not a rewrite), and the id index is rebuilt from the result, so
    /// an id typed into the buffer is real the moment it is saved (I2).
    ///
    /// The document's in-memory command history is replaced along with
    /// it: a buffer edit is not a kernel command and cannot be undone
    /// as one. The editor's own undo stack covers the session.
    ///
    /// # Errors
    ///
    /// [`VaultError::Io`] if `path` is not a file this vault has
    /// loaded — the editor addresses a file it is showing, and writing
    /// an unknown path would scatter org files outside the vault — or
    /// if the write fails; [`VaultError::Parse`] if `source` is not org
    /// this parser can round-trip.
    pub fn set_source(&mut self, path: &Path, source: &str) -> Result<(), VaultError> {
        if !self.documents.contains_key(path) {
            return Err(VaultError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            )));
        }
        let doc = Document::load_str(source).map_err(|_| VaultError::Parse {
            path: path.to_path_buf(),
        })?;
        fs::write(path, doc.source())?;
        self.documents.insert(path.to_path_buf(), doc);
        self.reindex_file(path);
        Ok(())
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
        self.revision = self.revision.wrapping_add(1);
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
        self.revision = self.revision.wrapping_add(1);
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
        self.revision = self.revision.wrapping_add(1);
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
            // `notes.org_archive` is org's own archive spelling and an
            // org file like any other: leaving it out would make every
            // `id:` link into an archived note dead the moment the
            // window reopened (Q3-V2).
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "org" || e == "org_archive")
            && let Err(e) = f(&path)
        {
            return Err(io::Error::other(e.to_string()));
        }
    }
    Ok(())
}

/// Decode `+` and `%XX` form encoding (the web form / journal wire).
fn form_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if let Some(h) = s.get(i + 1..i + 3)
                    && let Ok(v) = u8::from_str_radix(h, 16)
                {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
