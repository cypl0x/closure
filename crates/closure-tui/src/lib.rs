//! ratatui + crossterm shell for the closure kernel.
//!
//! Read-only vault browser: file list + headline tree, full-source
//! file view (=RET=, =j=/=k= scroll), fuzzy find-file (=/=) and
//! vault-wide headline search (=s=), which-key popup on pending chord
//! prefixes. All state transitions live in the terminal-free [`App`].

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use closure_core::Document;
use closure_input::{ChordTrie, TrieStep};
use closure_store::Vault;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use thiserror::Error;

/// Shell errors.
#[derive(Debug, Error)]
pub enum TuiError {
    /// IO failure setting up the terminal.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// A vault write (capture/rename/add/delete) failed.
    #[error("vault: {0}")]
    Vault(String),
}

/// Default browse-mode bindings. Multi-stroke chords exercise the
/// which-key popup (spec invariant I4: bindings drive the popup, no
/// hand-maintained table).
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("q", "quit"),
    ("ESC", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
];

/// Emacs-style bindings: Ctrl/Meta chords, `C-x C-c` quits.
const EMACS_BINDINGS: &[(&str, &str)] = &[
    ("C-n", "next-file"),
    ("C-p", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("M-<", "first-file"),
    ("M->", "last-file"),
    ("C-x C-c", "quit"),
    ("C-s", "search-start"),
    ("C-c s", "search-headline-start"),
    ("RET", "open-file"),
    ("C-c b", "backlinks"),
    ("C-c c", "capture-start"),
    ("C-c l", "headline-list"),
    ("C-x u", "undo"),
    ("C-x r", "redo"),
];

/// Vim-style bindings: modal navigation keys.
const VIM_BINDINGS: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("Z Z", "quit"),
    ("q", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
];

/// Helix-style bindings: vim-like with `U` redo and `g e` end.
const HELIX_BINDINGS: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("g e", "last-file"),
    ("q", "quit"),
    ("ESC", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("U", "redo"),
];

/// Notion-style bindings: arrows + slash command, minimal chords.
const NOTION_BINDINGS: &[(&str, &str)] = &[
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("ESC", "quit"),
    ("q", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
];

/// The `(chord, command)` table for an input mode. Every mode binds
/// the same command set (I4); only the chords differ.
#[must_use]
pub const fn mode_bindings(
    mode: closure_config::InputMode,
) -> &'static [(&'static str, &'static str)] {
    match mode {
        closure_config::InputMode::Emacs => EMACS_BINDINGS,
        closure_config::InputMode::Vim => VIM_BINDINGS,
        closure_config::InputMode::Doom => DEFAULT_BINDINGS,
        closure_config::InputMode::Helix => HELIX_BINDINGS,
        closure_config::InputMode::Notion => NOTION_BINDINGS,
    }
}

/// One headline as the shell sees it: where it lives, its stable
/// block id, and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlineRecord {
    /// File containing the headline.
    pub path: PathBuf,
    /// Stable block id (`:ID:`, I2).
    pub id: String,
    /// Headline title.
    pub title: String,
}

/// Which input surface the shell is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Navigating the file list via chord bindings.
    Browse,
    /// Typing a fuzzy file query; strokes edit the query instead of
    /// firing chords.
    Search,
    /// Typing a fuzzy query over headline titles across the vault.
    SearchHeadlines,
    /// Reading the selected file's full org source; =j=/=k= scroll.
    FileView,
    /// Listing the notes that link into the selected file.
    Backlinks,
    /// Typing the title of a new capture entry in a minibuffer.
    Capture,
    /// Navigating the selected file's headlines with a cursor.
    Headlines,
    /// Editing a headline's title in a minibuffer.
    Rename,
    /// Typing the title of a new sibling headline.
    AddHeadline,
    /// Awaiting =y= to confirm deleting the cursor subtree.
    ConfirmDelete,
}

/// Elm-style application state for the terminal shell. Strokes go in
/// via [`Self::handle_stroke`]; rendering reads the accessors. No
/// terminal I/O lives here, which keeps every transition testable.
pub struct App {
    paths: Vec<PathBuf>,
    selected: Option<usize>,
    bindings: Vec<(String, String)>,
    trie: ChordTrie,
    pending: Vec<String>,
    popup: Option<Vec<String>>,
    quit: bool,
    mode: AppMode,
    query: String,
    result_cursor: usize,
    headlines: Vec<HeadlineRecord>,
    sources: Vec<(PathBuf, String)>,
    scroll: usize,
    backlinks: Vec<(PathBuf, PathBuf, String)>,
    capture_request: Option<String>,
    rename_target: Option<String>,
    rename_request: Option<(String, String)>,
    add_target: Option<String>,
    add_request: Option<(String, String)>,
    delete_target: Option<String>,
    delete_request: Option<String>,
    undo_request: bool,
    redo_request: bool,
}

impl App {
    /// Build an app over `paths` with the default browse bindings.
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self::with_bindings(paths, DEFAULT_BINDINGS)
    }

    /// Build an app over `paths` with the binding table of `mode`.
    #[must_use]
    pub fn with_mode(paths: Vec<PathBuf>, mode: closure_config::InputMode) -> Self {
        Self::with_bindings(paths, mode_bindings(mode))
    }

    /// Build an app over `paths` with caller-supplied
    /// `(chord, command)` bindings, replacing the defaults entirely.
    #[must_use]
    pub fn with_bindings(paths: Vec<PathBuf>, bindings: &[(&str, &str)]) -> Self {
        let selected = if paths.is_empty() { None } else { Some(0) };
        Self {
            paths,
            selected,
            bindings: bindings
                .iter()
                .map(|(c, n)| ((*c).to_owned(), (*n).to_owned()))
                .collect(),
            trie: ChordTrie::build(bindings),
            pending: Vec::new(),
            popup: None,
            quit: false,
            mode: AppMode::Browse,
            query: String::new(),
            result_cursor: 0,
            headlines: Vec::new(),
            sources: Vec::new(),
            scroll: 0,
            backlinks: Vec::new(),
            capture_request: None,
            rename_target: None,
            rename_request: None,
            add_target: None,
            add_request: None,
            delete_target: None,
            delete_request: None,
            undo_request: false,
            redo_request: false,
        }
    }

    /// Consume the pending undo request, true at most once per `u`.
    pub const fn take_undo_request(&mut self) -> bool {
        let r = self.undo_request;
        self.undo_request = false;
        r
    }

    /// Consume the pending redo request, true at most once per `C-r`.
    pub const fn take_redo_request(&mut self) -> bool {
        let r = self.redo_request;
        self.redo_request = false;
        r
    }

    /// Consume the `(after-id, title)` add-sibling confirmed by the
    /// user, if any. The shell performs the vault write.
    pub const fn take_add_request(&mut self) -> Option<(String, String)> {
        self.add_request.take()
    }

    /// Consume the block id whose subtree deletion the user
    /// confirmed, if any. The shell performs the vault write.
    pub const fn take_delete_request(&mut self) -> Option<String> {
        self.delete_request.take()
    }

    /// Consume the `(block id, new title)` rename confirmed by the
    /// user, if any. The shell performs the vault write.
    pub const fn take_rename_request(&mut self) -> Option<(String, String)> {
        self.rename_request.take()
    }

    /// Consume the capture title confirmed by the user, if any. The
    /// shell performs the actual vault write and clears the request.
    pub const fn take_capture_request(&mut self) -> Option<String> {
        self.capture_request.take()
    }

    /// Replace the browsable paths, keeping the selection on the same
    /// file when it still exists and falling back to the first path.
    pub fn set_paths(&mut self, paths: Vec<PathBuf>) {
        let keep = self.selected_path().map(Path::to_path_buf);
        self.paths = paths;
        self.selected = keep
            .and_then(|k| self.paths.iter().position(|p| *p == k))
            .or(if self.paths.is_empty() { None } else { Some(0) });
    }

    /// Provide the vault's backlink records as
    /// `(target file, linking file, linking headline title)` rows.
    pub fn set_backlinks(&mut self, backlinks: Vec<(PathBuf, PathBuf, String)>) {
        self.backlinks = backlinks;
    }

    /// `(linking file, linking headline title)` rows pointing at the
    /// selected file, in insertion order.
    #[must_use]
    pub fn backlink_results(&self) -> Vec<(&Path, &str)> {
        let Some(sel) = self.selected_path() else {
            return Vec::new();
        };
        self.backlinks
            .iter()
            .filter(|(target, _, _)| target.as_path() == sel)
            .map(|(_, src, title)| (src.as_path(), title.as_str()))
            .collect()
    }

    /// Provide the `(file, org source)` records shown by the file
    /// view. Typically harvested from the vault once at startup.
    pub fn set_sources(&mut self, sources: Vec<(PathBuf, String)>) {
        self.sources = sources;
    }

    /// Source of the file open in the view, `None` outside
    /// [`AppMode::FileView`].
    #[must_use]
    pub fn view_source(&self) -> Option<&str> {
        if self.mode != AppMode::FileView {
            return None;
        }
        self.selected_path().and_then(|sel| {
            self.sources
                .iter()
                .find(|(p, _)| p.as_path() == sel)
                .map(|(_, src)| src.as_str())
        })
    }

    /// Scroll offset (top visible line) of the file view.
    #[must_use]
    pub const fn scroll(&self) -> usize {
        self.scroll
    }

    /// Provide the headline records searched by headline mode and
    /// listed by the per-file headline list. Typically harvested from
    /// the vault once at startup.
    pub fn set_headlines(&mut self, headlines: Vec<HeadlineRecord>) {
        self.headlines = headlines;
    }

    /// Headlines matching the live query, best fuzzy score first.
    #[must_use]
    pub fn headline_results(&self) -> Vec<(&Path, &str)> {
        let mut scored: Vec<(usize, u32)> = self
            .headlines
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                closure_query::fuzzy_score(&self.query, &r.title).map(|sc| (i, sc))
            })
            .collect();
        scored.sort_by_key(|&(_, sc)| std::cmp::Reverse(sc));
        scored
            .iter()
            .map(|&(i, _)| {
                let r = &self.headlines[i];
                (r.path.as_path(), r.title.as_str())
            })
            .collect()
    }

    /// `(title, id)` rows of the selected file's headlines, in record
    /// order.
    #[must_use]
    pub fn file_headlines(&self) -> Vec<(&str, &str)> {
        let Some(sel) = self.selected_path() else {
            return Vec::new();
        };
        self.headlines
            .iter()
            .filter(|r| r.path.as_path() == sel)
            .map(|r| (r.title.as_str(), r.id.as_str()))
            .collect()
    }

    /// Index of the highlighted row in [`Self::results`].
    #[must_use]
    pub const fn result_cursor(&self) -> usize {
        self.result_cursor
    }

    /// Current input surface.
    #[must_use]
    pub const fn mode(&self) -> AppMode {
        self.mode
    }

    /// The live fuzzy query (empty outside search mode).
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Paths matching the live query, best fuzzy score first. With an
    /// empty query every path is returned in display order.
    #[must_use]
    pub fn results(&self) -> Vec<&Path> {
        let names: Vec<String> = self.paths.iter().map(|p| p.display().to_string()).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        closure_query::fuzzy_filter(&self.query, &name_refs)
            .iter()
            .filter_map(|(name, _)| {
                self.paths
                    .iter()
                    .find(|p| p.display().to_string() == *name)
                    .map(PathBuf::as_path)
            })
            .collect()
    }

    /// The browsable file paths, in display order.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Index of the selected file, if any.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Path of the selected file, if any.
    #[must_use]
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected
            .and_then(|i| self.paths.get(i))
            .map(PathBuf::as_path)
    }

    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// The strokes of the in-progress chord, space-joined; empty when
    /// no chord is pending.
    #[must_use]
    pub fn pending_chord(&self) -> String {
        self.pending.join(" ")
    }

    /// Which-key popup lines (`chord → command`) while a chord prefix
    /// is pending, `None` otherwise.
    #[must_use]
    pub fn popup_lines(&self) -> Option<&[String]> {
        self.popup.as_deref()
    }

    /// Feed one key stroke into the active surface: query editing in
    /// search mode, the chord trie otherwise.
    pub fn handle_stroke(&mut self, stroke: &str) {
        if matches!(self.mode, AppMode::Search | AppMode::SearchHeadlines) {
            self.handle_search_stroke(stroke);
            return;
        }
        if self.mode == AppMode::FileView {
            self.handle_view_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Backlinks {
            self.handle_backlinks_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Capture {
            self.handle_capture_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Headlines {
            self.handle_headlines_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Rename {
            self.handle_rename_stroke(stroke);
            return;
        }
        if self.mode == AppMode::AddHeadline {
            self.handle_add_stroke(stroke);
            return;
        }
        if self.mode == AppMode::ConfirmDelete {
            if stroke == "y" {
                self.delete_request = self.delete_target.take();
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            } else {
                self.delete_target = None;
                self.mode = AppMode::Headlines;
            }
            return;
        }
        match self.trie.step(stroke) {
            TrieStep::Resolved(cmd) => {
                self.pending.clear();
                self.popup = None;
                self.apply_command(&cmd);
            }
            TrieStep::Pending(_) => {
                self.pending.push(stroke.to_owned());
                let prefix = self.pending_chord();
                let mut lines: Vec<String> = self
                    .bindings
                    .iter()
                    .filter(|(chord, _)| chord.starts_with(&prefix) && chord.as_str() != prefix)
                    .map(|(chord, cmd)| {
                        let rest = chord[prefix.len()..].trim_start();
                        format!("{rest} → {cmd}")
                    })
                    .collect();
                lines.sort();
                self.popup = Some(lines);
            }
            TrieStep::Unbound => {
                self.pending.clear();
                self.popup = None;
            }
        }
    }

    fn handle_headlines_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.file_headlines().len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "r" => {
                let target = self
                    .file_headlines()
                    .get(self.result_cursor)
                    .map(|(title, id)| ((*title).to_owned(), (*id).to_owned()));
                if let Some((title, id)) = target {
                    self.rename_target = Some(id);
                    self.query = title;
                    self.mode = AppMode::Rename;
                }
            }
            "a" => {
                let target = self
                    .file_headlines()
                    .get(self.result_cursor)
                    .map(|(_, id)| (*id).to_owned());
                if let Some(id) = target {
                    self.add_target = Some(id);
                    self.query.clear();
                    self.mode = AppMode::AddHeadline;
                }
            }
            "d" => {
                let target = self
                    .file_headlines()
                    .get(self.result_cursor)
                    .map(|(_, id)| (*id).to_owned());
                if let Some(id) = target {
                    self.delete_target = Some(id);
                    self.mode = AppMode::ConfirmDelete;
                }
            }
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => {}
        }
    }

    fn handle_add_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.add_target = None;
            }
            "RET" => {
                if let Some(id) = self.add_target.take()
                    && !self.query.is_empty()
                {
                    self.add_request = Some((id, std::mem::take(&mut self.query)));
                }
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "DEL" => {
                self.query.pop();
            }
            "SPC" => self.query.push(' '),
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                }
            }
        }
    }

    fn handle_rename_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.rename_target = None;
            }
            "RET" => {
                if let Some(id) = self.rename_target.take()
                    && !self.query.is_empty()
                {
                    self.rename_request = Some((id, std::mem::take(&mut self.query)));
                }
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "DEL" => {
                self.query.pop();
            }
            "SPC" => self.query.push(' '),
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                }
            }
        }
    }

    fn handle_capture_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "RET" => {
                if !self.query.is_empty() {
                    self.capture_request = Some(std::mem::take(&mut self.query));
                }
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "DEL" => {
                self.query.pop();
            }
            "SPC" => self.query.push(' '),
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                }
            }
        }
    }

    fn handle_backlinks_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.backlink_results().len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "RET" => {
                let idx = self
                    .backlink_results()
                    .get(self.result_cursor)
                    .map(|(src, _)| src.to_path_buf())
                    .and_then(|pb| self.paths.iter().position(|p| *p == pb));
                if idx.is_some() {
                    self.selected = idx;
                }
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => {}
        }
    }

    fn handle_view_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self
                    .view_source()
                    .map_or(0, |src| src.lines().count().saturating_sub(1));
                self.scroll = (self.scroll + 1).min(last);
            }
            "k" | "<up>" => self.scroll = self.scroll.saturating_sub(1),
            "ESC" | "q" | "h" | "DEL" => self.mode = AppMode::Browse,
            _ => {}
        }
    }

    fn handle_search_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "RET" => {
                let pick = if self.mode == AppMode::SearchHeadlines {
                    self.headline_results()
                        .get(self.result_cursor)
                        .map(|(p, _)| p.to_path_buf())
                } else {
                    self.results()
                        .get(self.result_cursor)
                        .copied()
                        .map(Path::to_path_buf)
                };
                let idx = pick.and_then(|pb| self.paths.iter().position(|p| *p == pb));
                if idx.is_some() {
                    self.selected = idx;
                }
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "<down>" => {
                let len = if self.mode == AppMode::SearchHeadlines {
                    self.headline_results().len()
                } else {
                    self.results().len()
                };
                self.result_cursor = (self.result_cursor + 1).min(len.saturating_sub(1));
            }
            "<up>" => {
                self.result_cursor = self.result_cursor.saturating_sub(1);
            }
            "DEL" => {
                self.query.pop();
                self.result_cursor = 0;
            }
            "SPC" => {
                self.query.push(' ');
                self.result_cursor = 0;
            }
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                    self.result_cursor = 0;
                }
            }
        }
    }

    fn apply_command(&mut self, cmd: &str) {
        let last = self.paths.len().checked_sub(1);
        match cmd {
            "next-file" => {
                if let (Some(i), Some(last)) = (self.selected, last) {
                    self.selected = Some((i + 1).min(last));
                }
            }
            "prev-file" => {
                if let Some(i) = self.selected {
                    self.selected = Some(i.saturating_sub(1));
                }
            }
            "first-file" if self.selected.is_some() => {
                self.selected = Some(0);
            }
            "last-file" => {
                self.selected = last;
            }
            "quit" => self.quit = true,
            "search-start" => {
                self.mode = AppMode::Search;
                self.query.clear();
            }
            "search-headline-start" => {
                self.mode = AppMode::SearchHeadlines;
                self.query.clear();
            }
            "backlinks" => {
                self.mode = AppMode::Backlinks;
                self.result_cursor = 0;
            }
            "capture-start" => {
                self.mode = AppMode::Capture;
                self.query.clear();
            }
            "headline-list" => {
                self.mode = AppMode::Headlines;
                self.result_cursor = 0;
            }
            "undo" => self.undo_request = true,
            "redo" => self.redo_request = true,
            "open-file" => {
                let has_source = self
                    .selected_path()
                    .is_some_and(|sel| self.sources.iter().any(|(p, _)| p.as_path() == sel));
                if has_source {
                    self.mode = AppMode::FileView;
                    self.scroll = 0;
                }
            }
            _ => {}
        }
    }
}

/// Translate a terminal key event into a chord stroke in Emacs/doom
/// notation (`j`, `G`, `SPC`, `C-c`, `M-x`, `RET`, …). Returns `None`
/// for keys the shell does not map.
#[must_use]
pub fn stroke_of(ev: &crossterm::event::KeyEvent) -> Option<String> {
    use crossterm::event::KeyModifiers;
    let base = match ev.code {
        KeyCode::Char(' ') => "SPC".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "ESC".to_owned(),
        KeyCode::Enter => "RET".to_owned(),
        KeyCode::Tab => "TAB".to_owned(),
        KeyCode::Backspace => "DEL".to_owned(),
        KeyCode::Up => "<up>".to_owned(),
        KeyCode::Down => "<down>".to_owned(),
        _ => return None,
    };
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(format!("C-{base}"));
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        return Some(format!("M-{base}"));
    }
    Some(base)
}

/// Render the headline tree of `doc` as indented text lines:
/// `indent * TODO [#P] title :tags:    [id]`.
#[must_use]
pub fn headline_lines(doc: &Document) -> String {
    let mut s = String::new();
    for h in doc.all_headlines() {
        let indent = "  ".repeat(usize::from(h.level()).saturating_sub(1));
        let mut prefix = String::new();
        if let Some(t) = h.todo() {
            prefix.push_str(t);
            prefix.push(' ');
        }
        if let Some(p) = h.priority() {
            let _ = write!(prefix, "[#{p}] ");
        }
        let tags = if h.tags().is_empty() {
            String::new()
        } else {
            format!(" :{}:", h.tags().join(":"))
        };
        let _ = writeln!(
            s,
            "{indent}* {prefix}{title}{tags}    [{id}]",
            title = h.title(),
            id = h.id()
        );
    }
    s
}

/// Default capture target inside the vault for TUI captures.
const CAPTURE_TARGET: &str = "inbox.org";
/// Default headline prefix for TUI captures.
const CAPTURE_PREFIX: &str = "TODO ";

/// Run the TUI against an already-loaded vault. Returns when the user
/// quits via `q` or `Esc`. Captures (`c`) write back through
/// [`Vault::capture`], hence the mutable borrow.
pub fn run(vault: &mut Vault) -> Result<(), TuiError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, vault);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

/// Refresh every vault-derived record in the app: paths, headline
/// titles, file sources, and the backlink rows.
fn sync_app(app: &mut App, vault: &Vault) {
    app.set_paths(vault.paths());
    let mut headlines: Vec<HeadlineRecord> = Vec::new();
    for (path, doc) in vault.iter() {
        for h in doc.all_headlines() {
            headlines.push(HeadlineRecord {
                path: path.to_path_buf(),
                id: h.id().as_str().to_owned(),
                title: h.title().to_owned(),
            });
        }
    }
    app.set_headlines(headlines);
    let sources: Vec<(PathBuf, String)> = vault
        .iter()
        .map(|(path, doc)| (path.to_path_buf(), doc.source()))
        .collect();
    app.set_sources(sources);
    let mut backlinks: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for (path, doc) in vault.iter() {
        for h in doc.all_headlines() {
            for (src_path, src_id) in vault.backlinks_of(&h.id().to_string()) {
                let title = vault
                    .find_by_id(src_id)
                    .map_or_else(String::new, |(sh, _)| sh.title().to_owned());
                backlinks.push((path.to_path_buf(), src_path.clone(), title));
            }
        }
    }
    app.set_backlinks(backlinks);
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    vault: &mut Vault,
) -> Result<(), TuiError> {
    let mode = closure_config::Config::from_path(&vault.root().join("config.org"))
        .map_or(closure_config::InputMode::Doom, |c| c.input_mode);
    let mut app = App::with_mode(Vec::new(), mode);
    sync_app(&mut app, vault);

    loop {
        terminal.draw(|f| draw(f, &app, vault))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(stroke) = stroke_of(&key)
        {
            app.handle_stroke(&stroke);
            if let Some(title) = app.take_capture_request() {
                let template = closure_store::CaptureTemplate {
                    target: PathBuf::from(CAPTURE_TARGET),
                    headline_prefix: CAPTURE_PREFIX.to_owned(),
                    body: String::new(),
                };
                vault
                    .capture(&template, &title)
                    .map_err(|e| TuiError::Vault(e.to_string()))?;
                sync_app(&mut app, vault);
            }
            if let Some((id, title)) = app.take_rename_request() {
                vault
                    .rename_headline(&closure_core::BlockId::from_existing(&id), &title)
                    .map_err(|e| TuiError::Vault(e.to_string()))?;
                sync_app(&mut app, vault);
            }
            if let Some((after, title)) = app.take_add_request() {
                vault
                    .add_sibling(&closure_core::BlockId::from_existing(&after), &title)
                    .map_err(|e| TuiError::Vault(e.to_string()))?;
                sync_app(&mut app, vault);
            }
            if let Some(id) = app.take_delete_request() {
                vault
                    .remove_subtree(&closure_core::BlockId::from_existing(&id))
                    .map_err(|e| TuiError::Vault(e.to_string()))?;
                sync_app(&mut app, vault);
            }
            if app.take_undo_request()
                && let Some(path) = app.selected_path().map(Path::to_path_buf)
            {
                match vault.undo_in(&path) {
                    Ok(()) => sync_app(&mut app, vault),
                    Err(closure_store::VaultError::Undo(_)) => {}
                    Err(e) => return Err(TuiError::Vault(e.to_string())),
                }
            }
            if app.take_redo_request()
                && let Some(path) = app.selected_path().map(Path::to_path_buf)
            {
                match vault.redo_in(&path) {
                    Ok(()) => sync_app(&mut app, vault),
                    Err(closure_store::VaultError::Undo(_)) => {}
                    Err(e) => return Err(TuiError::Vault(e.to_string())),
                }
            }
            if app.should_quit() {
                return Ok(());
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame<'_>, app: &App, vault: &Vault) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let file_items: Vec<ListItem<'_>> = app
        .paths()
        .iter()
        .map(|p| ListItem::new(p.display().to_string()))
        .collect();
    let mut file_state = ListState::default();
    file_state.select(app.selected_index());
    let files = List::new(file_items)
        .block(Block::default().title("files").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(files, chunks[0], &mut file_state);

    if app.mode() == AppMode::FileView {
        let src = app.view_source().unwrap_or_default().to_owned();
        let title = app
            .selected_path()
            .map_or_else(String::new, |p| p.display().to_string());
        let offset = u16::try_from(app.scroll()).unwrap_or(u16::MAX);
        let view = Paragraph::new(src)
            .scroll((offset, 0))
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(view, chunks[1]);
    } else {
        let body_text = app
            .selected_path()
            .and_then(|p| vault.document(p))
            .map_or_else(String::new, headline_lines);
        let body = Paragraph::new(body_text)
            .block(Block::default().title("headlines").borders(Borders::ALL));
        f.render_widget(body, chunks[1]);
    }

    let overlay = match app.mode() {
        AppMode::Search => Some((
            format!("find file: {}", app.query()),
            app.results()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<String>>(),
        )),
        AppMode::SearchHeadlines => Some((
            format!("find headline: {}", app.query()),
            app.headline_results()
                .iter()
                .map(|(p, t)| format!("{t}    ({})", p.display()))
                .collect(),
        )),
        AppMode::Backlinks => Some((
            app.selected_path().map_or_else(
                || "backlinks".to_owned(),
                |p| format!("backlinks: {}", p.display()),
            ),
            app.backlink_results()
                .iter()
                .map(|(p, t)| format!("{t}    ({})", p.display()))
                .collect(),
        )),
        AppMode::Capture => Some((format!("capture: {}", app.query()), Vec::new())),
        AppMode::Rename => Some((format!("rename: {}", app.query()), Vec::new())),
        AppMode::AddHeadline => Some((format!("add headline: {}", app.query()), Vec::new())),
        AppMode::ConfirmDelete => Some((
            "delete subtree? y = confirm, other = cancel".to_owned(),
            Vec::new(),
        )),
        AppMode::Headlines => Some((
            app.selected_path().map_or_else(
                || "headlines".to_owned(),
                |p| format!("headlines: {}", p.display()),
            ),
            app.file_headlines()
                .iter()
                .map(|(t, id)| format!("{t}    [{id}]"))
                .collect(),
        )),
        AppMode::Browse | AppMode::FileView => None,
    };
    if let Some((title, rows)) = overlay {
        draw_overlay_list(f, area, title, rows, app.result_cursor());
    }

    if let Some(lines) = app.popup_lines() {
        draw_whichkey(f, area, app, lines);
    }
}

/// Bottom-half overlay list with a cursor row, used by the fuzzy
/// finders and the backlinks pane.
fn draw_overlay_list(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    title: String,
    rows: Vec<String>,
    cursor: usize,
) {
    let height = area.height / 2;
    let overlay_area = ratatui::layout::Rect {
        x: area.x,
        y: area.height.saturating_sub(height),
        width: area.width,
        height,
    };
    let items: Vec<ListItem<'_>> = rows.into_iter().map(ListItem::new).collect();
    let mut state = ListState::default();
    state.select(Some(cursor));
    let pane = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(ratatui::widgets::Clear, overlay_area);
    f.render_stateful_widget(pane, overlay_area, &mut state);
}

fn draw_whichkey(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    app: &App,
    lines: &[String],
) {
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let popup_area = ratatui::layout::Rect {
        x: area.x,
        y: area.height.saturating_sub(height.saturating_add(2)),
        width: area.width,
        height: height.saturating_add(2).min(area.height),
    };
    let title = format!("which-key: {}", app.pending_chord());
    let popup =
        Paragraph::new(lines.join("\n")).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(popup, popup_area);
}
