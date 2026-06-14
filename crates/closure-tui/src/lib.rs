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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use thiserror::Error;

use closure_tree_sitter::Highlighter;

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

/// Doom/default browse bindings, from the shared canonical keymap
/// ([`closure_input::mode_keymap`]) so the TUI and every other shell
/// stay identical per mode (I4).
const DEFAULT_BINDINGS: &[(&str, &str)] = closure_input::mode_keymap(closure_config::InputMode::Doom);

/// The `(chord, command)` table for an input mode — delegates to the
/// canonical [`closure_input::mode_keymap`], the single source of
/// truth shared by all shells.
#[must_use]
pub const fn mode_bindings(
    mode: closure_config::InputMode,
) -> &'static [(&'static str, &'static str)] {
    closure_input::mode_keymap(mode)
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
    /// Headline body text (for inline editing).
    pub body: String,
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
    /// Fuzzy-picking a command by name; rows show the chord (I4).
    Palette,
    /// Browsing database view rows with a cursor.
    DbView,
    /// Typing `KEY=VALUE` to set a property on the cursor row.
    EditCell,
    /// Picking a code block of the selected file to evaluate.
    Blocks,
    /// Editing a headline's body in a multi-line buffer.
    EditBody,
    /// Browsing the SCHEDULED/DEADLINE agenda with a cursor.
    Agenda,
    /// Typing a fuzzy query over body lines across the vault.
    BodySearch,
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
    /// Last error surfaced from re-validating config.org on save
    /// (validate-on-save). Contains the rich message with line info.
    last_config_error: Option<String>,
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
    input_mode: closure_config::InputMode,
    view_rows: Vec<(String, Vec<String>)>,
    cell_target: Option<String>,
    property_request: Option<(String, String, String)>,
    blocks: Vec<(PathBuf, String)>,
    eval_request: Option<(PathBuf, usize)>,
    buffer: String,
    body_target: Option<String>,
    body_request: Option<(String, String)>,
    struct_request: Option<(String, String)>,
    move_request: Option<(String, String)>,
    cut_request: Option<String>,
    paste_request: Option<String>,
    agenda: Vec<(PathBuf, String)>,
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
        let mut app = Self::with_bindings(paths, mode_bindings(mode));
        app.input_mode = mode;
        app
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
            last_config_error: None,
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
            input_mode: closure_config::InputMode::Doom,
            view_rows: Vec::new(),
            cell_target: None,
            property_request: None,
            blocks: Vec::new(),
            eval_request: None,
            buffer: String::new(),
            body_target: None,
            body_request: None,
            struct_request: None,
            move_request: None,
            cut_request: None,
            paste_request: None,
            agenda: Vec::new(),
        }
    }

    /// Provide the agenda rows as `(file, "date KIND title")`, sorted.
    pub fn set_agenda(&mut self, agenda: Vec<(PathBuf, String)>) {
        self.agenda = agenda;
    }

    /// The agenda row labels, in order.
    #[must_use]
    pub fn agenda_results(&self) -> Vec<&str> {
        self.agenda
            .iter()
            .map(|(_, label)| label.as_str())
            .collect()
    }

    /// Body-search hits matching the live query: `(file, "file:line:
    /// text")`, best fuzzy score first. Empty query yields nothing.
    #[must_use]
    pub fn body_results(&self) -> Vec<(&Path, String)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(&Path, String, u32)> = Vec::new();
        for (path, src) in &self.sources {
            for (i, line) in src.lines().enumerate() {
                if let Some(score) = closure_query::fuzzy_score(&self.query, line) {
                    scored.push((
                        path.as_path(),
                        format!("{}:{}:{}", path.display(), i + 1, line),
                        score,
                    ));
                }
            }
        }
        scored.sort_by_key(|&(_, _, s)| std::cmp::Reverse(s));
        scored.into_iter().map(|(p, t, _)| (p, t)).collect()
    }

    /// Consume the block id whose subtree the user cut. The shell
    /// pushes it onto the kill ring and removes it.
    pub const fn take_cut_request(&mut self) -> Option<String> {
        self.cut_request.take()
    }

    /// Consume the block id after which the user pasted the kill-ring
    /// top. The shell performs the splice.
    pub const fn take_paste_request(&mut self) -> Option<String> {
        self.paste_request.take()
    }

    /// Consume the `(id, after-id)` subtree move confirmed by the
    /// user. The shell performs the vault write.
    pub const fn take_move_request(&mut self) -> Option<(String, String)> {
        self.move_request.take()
    }

    /// Consume the `(op, id)` structure operation confirmed by the
    /// user (`op` is `promote`/`demote`). The shell performs the
    /// vault write.
    pub const fn take_struct_request(&mut self) -> Option<(String, String)> {
        self.struct_request.take()
    }

    /// The multi-line edit buffer (body editor).
    #[must_use]
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Consume the `(id, body)` body edit confirmed by the user, if
    /// any. The shell performs the vault write.
    pub const fn take_body_request(&mut self) -> Option<(String, String)> {
        self.body_request.take()
    }

    /// Provide the `(file, label)` code-block records listed by the
    /// block picker, in per-file source order.
    pub fn set_blocks(&mut self, blocks: Vec<(PathBuf, String)>) {
        self.blocks = blocks;
    }

    /// Labels of the selected file's code blocks, in source order.
    #[must_use]
    pub fn block_results(&self) -> Vec<&str> {
        let Some(sel) = self.selected_path() else {
            return Vec::new();
        };
        self.blocks
            .iter()
            .filter(|(p, _)| p.as_path() == sel)
            .map(|(_, label)| label.as_str())
            .collect()
    }

    /// Consume the `(file, block index)` evaluation confirmed by the
    /// user, if any. The shell performs the vault write.
    pub const fn take_eval_request(&mut self) -> Option<(PathBuf, usize)> {
        self.eval_request.take()
    }

    /// Provide the database view rows as `(block id, cells)` pairs.
    pub fn set_view_rows(&mut self, rows: Vec<(String, Vec<String>)>) {
        self.view_rows = rows;
    }

    /// The database view rows.
    #[must_use]
    pub fn view_rows(&self) -> &[(String, Vec<String>)] {
        &self.view_rows
    }

    /// Consume the `(id, key, value)` property edit confirmed by the
    /// user, if any. The shell performs the vault write.
    pub const fn take_property_request(&mut self) -> Option<(String, String, String)> {
        self.property_request.take()
    }

    /// The active keybinding dialect.
    #[must_use]
    pub const fn input_mode(&self) -> closure_config::InputMode {
        self.input_mode
    }

    /// `(command, chord)` rows matching the live query, best fuzzy
    /// score first (alphabetical on ties); one row per command with
    /// its first bound chord in the active table.
    #[must_use]
    pub fn palette_results(&self) -> Vec<(String, String)> {
        let mut commands: std::collections::BTreeMap<&str, &str> =
            std::collections::BTreeMap::new();
        for (chord, cmd) in &self.bindings {
            commands.entry(cmd.as_str()).or_insert(chord.as_str());
        }
        let mut scored: Vec<(&str, &str, u32)> = commands
            .iter()
            .filter_map(|(cmd, chord)| {
                closure_query::fuzzy_score(&self.query, cmd).map(|sc| (*cmd, *chord, sc))
            })
            .collect();
        scored.sort_by_key(|&(_, _, sc)| std::cmp::Reverse(sc));
        scored
            .into_iter()
            .map(|(cmd, chord, _)| (cmd.to_owned(), chord.to_owned()))
            .collect()
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

    /// Set the last config validation error (from validate-on-save using
    /// the vault watcher + `revalidate_config`). The error string includes
    /// the rich CUE-style line/col context.
    pub fn set_config_error(&mut self, err: Option<String>) {
        self.last_config_error = err;
    }

    /// Current config validation error to surface in the TUI (status line).
    #[must_use]
    pub fn config_error(&self) -> Option<&str> {
        self.last_config_error.as_deref()
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
        if self.mode == AppMode::Palette {
            self.handle_palette_stroke(stroke);
            return;
        }
        if self.mode == AppMode::DbView {
            self.handle_dbview_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Blocks {
            self.handle_blocks_stroke(stroke);
            return;
        }
        if self.mode == AppMode::EditBody {
            self.handle_editbody_stroke(stroke);
            return;
        }
        if self.mode == AppMode::Agenda {
            self.handle_agenda_stroke(stroke);
            return;
        }
        if self.mode == AppMode::BodySearch {
            self.handle_bodysearch_stroke(stroke);
            return;
        }
        if self.mode == AppMode::EditCell {
            self.handle_editcell_stroke(stroke);
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
            "i" => {
                let target = self
                    .file_headlines()
                    .get(self.result_cursor)
                    .map(|(_, id)| (*id).to_owned());
                if let Some(id) = target {
                    let body = self
                        .headlines
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| r.body.clone())
                        .unwrap_or_default();
                    self.body_target = Some(id);
                    self.buffer = body;
                    self.mode = AppMode::EditBody;
                }
            }
            "<" | ">" => {
                let op = if stroke == "<" { "promote" } else { "demote" };
                if let Some((_, id)) = self.file_headlines().get(self.result_cursor) {
                    self.struct_request = Some((op.to_owned(), (*id).to_owned()));
                }
            }
            "x" => {
                if let Some((_, id)) = self.file_headlines().get(self.result_cursor) {
                    self.cut_request = Some((*id).to_owned());
                }
            }
            "p" => {
                if let Some((_, id)) = self.file_headlines().get(self.result_cursor) {
                    self.paste_request = Some((*id).to_owned());
                }
            }
            "J" => {
                let hs = self.file_headlines();
                if let (Some((_, cur)), Some((_, next))) =
                    (hs.get(self.result_cursor), hs.get(self.result_cursor + 1))
                {
                    self.move_request = Some(((*cur).to_owned(), (*next).to_owned()));
                }
            }
            "K" => {
                let hs = self.file_headlines();
                if self.result_cursor > 0
                    && let (Some((_, prev)), Some((_, cur))) =
                        (hs.get(self.result_cursor - 1), hs.get(self.result_cursor))
                {
                    self.move_request = Some(((*prev).to_owned(), (*cur).to_owned()));
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

    fn handle_bodysearch_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "RET" => {
                let pick = self
                    .body_results()
                    .get(self.result_cursor)
                    .map(|(p, _)| p.to_path_buf());
                if let Some(path) = pick
                    && let Some(i) = self.paths.iter().position(|p| *p == path)
                {
                    self.selected = Some(i);
                }
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "<down>" => {
                let last = self.body_results().len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
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

    fn handle_agenda_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.agenda.len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "RET" => {
                let target = self.agenda.get(self.result_cursor).map(|(p, _)| p.clone());
                if let Some(path) = target
                    && let Some(i) = self.paths.iter().position(|p| *p == path)
                {
                    self.selected = Some(i);
                    self.mode = AppMode::Browse;
                    self.result_cursor = 0;
                }
            }
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => {}
        }
    }

    fn handle_editbody_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.buffer.clear();
                self.body_target = None;
            }
            "C-s" => {
                if let Some(id) = self.body_target.take() {
                    self.body_request = Some((id, std::mem::take(&mut self.buffer)));
                }
                self.mode = AppMode::Browse;
                self.buffer.clear();
            }
            "RET" => self.buffer.push('\n'),
            "SPC" => self.buffer.push(' '),
            "DEL" => {
                self.buffer.pop();
            }
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.buffer.push(c);
                }
            }
        }
    }

    fn handle_blocks_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.block_results().len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "RET" => {
                let target = self
                    .selected_path()
                    .filter(|_| !self.block_results().is_empty())
                    .map(Path::to_path_buf);
                if let Some(path) = target {
                    self.eval_request = Some((path, self.result_cursor));
                    self.mode = AppMode::Browse;
                    self.result_cursor = 0;
                }
            }
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => {}
        }
    }

    fn handle_dbview_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.view_rows.len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "RET" => {
                let target = self
                    .view_rows
                    .get(self.result_cursor)
                    .map(|(id, _)| id.clone());
                if let Some(id) = target {
                    self.cell_target = Some(id);
                    self.query.clear();
                    self.mode = AppMode::EditCell;
                }
            }
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => {}
        }
    }

    fn handle_editcell_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.cell_target = None;
            }
            "RET" => {
                let parsed = self.cell_target.take().zip(
                    self.query
                        .split_once('=')
                        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())),
                );
                if let Some((id, (key, value))) = parsed
                    && !key.is_empty()
                {
                    self.property_request = Some((id, key, value));
                }
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
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

    fn handle_palette_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "RET" => {
                let pick = self
                    .palette_results()
                    .get(self.result_cursor)
                    .map(|(cmd, _)| cmd.clone());
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
                if let Some(cmd) = pick {
                    self.apply_command(&cmd);
                }
            }
            "<down>" => {
                let last = self.palette_results().len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
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
            "palette" => {
                self.mode = AppMode::Palette;
                self.query.clear();
                self.result_cursor = 0;
            }
            "db-view" => {
                self.mode = AppMode::DbView;
                self.result_cursor = 0;
            }
            "block-list" => {
                self.mode = AppMode::Blocks;
                self.result_cursor = 0;
            }
            "agenda" => {
                self.mode = AppMode::Agenda;
                self.result_cursor = 0;
            }
            "body-search" => {
                self.mode = AppMode::BodySearch;
                self.query.clear();
                self.result_cursor = 0;
            }
            "cycle-mode" => {
                use closure_config::InputMode as M;
                let next = match self.input_mode {
                    M::Notion => M::Emacs,
                    M::Emacs => M::Vim,
                    M::Vim => M::Doom,
                    M::Doom => M::Helix,
                    M::Helix => M::Notion,
                };
                let table = mode_bindings(next);
                self.input_mode = next;
                self.bindings = table
                    .iter()
                    .map(|(c, n)| ((*c).to_owned(), (*n).to_owned()))
                    .collect();
                self.trie = ChordTrie::build(table);
                self.pending.clear();
                self.popup = None;
            }
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

/// Current unix time in seconds (0 if the clock predates the epoch).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
                body: h.body_text().to_owned(),
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
    let spec = closure_query::ViewSpec::parse(":from all :columns title,todo,priority")
        .unwrap_or_else(|_| closure_query::ViewSpec {
            from: closure_query::Source::All,
            columns: Vec::new(),
            sort: None,
            filter: None,
        });
    let rows: Vec<(String, Vec<String>)> = spec
        .rows(vault)
        .iter()
        .map(|m| {
            let cells: Vec<String> = spec.columns.iter().map(|c| c.extract(m.headline)).collect();
            (m.headline.id().to_string(), cells)
        })
        .collect();
    app.set_view_rows(rows);
    let mut blocks: Vec<(PathBuf, String)> = Vec::new();
    for (path, doc) in vault.iter() {
        for (i, n) in doc.org().code_blocks().iter().enumerate() {
            if let Some(cb) = n.as_code_block() {
                let name = doc
                    .org()
                    .code_block_name(i)
                    .map_or_else(String::new, |n| format!(" ({n})"));
                let first = cb.content.lines().next().unwrap_or("");
                blocks.push((
                    path.to_path_buf(),
                    format!("{}{name}: {first}", cb.language.unwrap_or("shell")),
                ));
            }
        }
    }
    app.set_blocks(blocks);
    let agenda: Vec<(PathBuf, String)> = vault
        .agenda()
        .into_iter()
        .map(|e| {
            let kind = match e.kind {
                closure_store::AgendaKind::Scheduled => "SCHEDULED",
                closure_store::AgendaKind::Deadline => "DEADLINE",
            };
            (e.path, format!("{}  {kind:9}  {}", e.date, e.title))
        })
        .collect();
    app.set_agenda(agenda);
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    vault: &mut Vault,
) -> Result<(), TuiError> {
    let cfg = closure_config::Config::from_path(&vault.root().join("config.org")).ok();
    let mode = cfg
        .as_ref()
        .map_or(closure_config::InputMode::Doom, |c| c.input_mode);
    let journal =
        closure_record::Journal::new(vault.root(), cfg.is_some_and(|c| c.record_commands));
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
            apply_requests(&mut app, vault, &journal)?;
            if app.should_quit() {
                return Ok(());
            }
        }

        // validate-on-save: periodically (or on every loop tick) re-validate
        // config.org using the vault's watcher-friendly method. Surface the
        // rich error (with line/col) so the user sees CUE-style problems
        // immediately after saving a bad config.
        if let Err(e) = vault.revalidate_config() {
            app.set_config_error(Some(e.to_string()));
        } else {
            app.set_config_error(None);
        }
    }
}

/// Drain every pending vault-write request the last stroke produced,
/// executing it through the kernel-backed vault methods and re-syncing
/// the app on success. Soft errors (missing block, empty history) are
/// no-ops; hard errors propagate.
fn apply_requests(
    app: &mut App,
    vault: &mut Vault,
    journal: &closure_record::Journal,
) -> Result<(), TuiError> {
    let vault_err = |e: closure_store::VaultError| TuiError::Vault(e.to_string());
    if let Some(title) = app.take_capture_request() {
        let template = closure_store::CaptureTemplate {
            target: PathBuf::from(CAPTURE_TARGET),
            headline_prefix: CAPTURE_PREFIX.to_owned(),
            body: String::new(),
        };
        vault.capture(&template, &title).map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((id, title)) = app.take_rename_request() {
        vault
            .rename_headline(&closure_core::BlockId::from_existing(&id), &title)
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((after, title)) = app.take_add_request() {
        vault
            .add_sibling(&closure_core::BlockId::from_existing(&after), &title)
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some(id) = app.take_delete_request() {
        vault
            .remove_subtree(&closure_core::BlockId::from_existing(&id))
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((id, body)) = app.take_body_request() {
        vault
            .set_body(&closure_core::BlockId::from_existing(&id), &body)
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((id, key, value)) = app.take_property_request() {
        vault
            .set_property(&closure_core::BlockId::from_existing(&id), &key, &value)
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    apply_structure_requests(app, vault, journal)
}

/// Drain the structural / history requests (promote, demote, move,
/// cut, paste, eval, undo, redo). These tolerate soft errors —
/// a missing block or empty history is a no-op, not a crash.
fn apply_structure_requests(
    app: &mut App,
    vault: &mut Vault,
    journal: &closure_record::Journal,
) -> Result<(), TuiError> {
    let soft = |e: &closure_store::VaultError| {
        matches!(
            e,
            closure_store::VaultError::Command(_) | closure_store::VaultError::Undo(_)
        )
    };
    // Run a vault op that may soft-fail, re-syncing on success.
    let run = |r: Result<(), closure_store::VaultError>,
               app: &mut App,
               vault: &mut Vault|
     -> Result<(), TuiError> {
        match r {
            Ok(()) => {
                sync_app(app, vault);
                Ok(())
            }
            Err(e) if soft(&e) => Ok(()),
            Err(e) => Err(TuiError::Vault(e.to_string())),
        }
    };
    let bid = closure_core::BlockId::from_existing;
    if let Some((op, id)) = app.take_struct_request() {
        let r = if op == "promote" {
            vault.promote(&bid(&id))
        } else {
            vault.demote(&bid(&id))
        };
        run(r, app, vault)?;
    }
    if let Some((id, after)) = app.take_move_request() {
        let r = vault.move_after(&bid(&id), &bid(&after));
        run(r, app, vault)?;
    }
    if let Some(id) = app.take_cut_request()
        && let Some(path) = app.selected_path().map(Path::to_path_buf)
    {
        let r = vault.cut(&path, &bid(&id));
        if r.is_ok() {
            let killed = vault.ring_top().unwrap_or_default().to_owned();
            journal.record(now_secs(), "kill", &killed).ok();
        }
        run(r, app, vault)?;
    }
    if let Some(id) = app.take_paste_request()
        && let Some(path) = app.selected_path().map(Path::to_path_buf)
    {
        let yanked = vault.ring_top().unwrap_or_default().to_owned();
        let r = vault.paste(&path, &bid(&id));
        if r.is_ok() {
            journal.record(now_secs(), "yank", &yanked).ok();
        }
        run(r, app, vault)?;
    }
    if let Some((path, index)) = app.take_eval_request() {
        let r = vault.eval_block(&path, index).map(|_| ());
        run(r, app, vault)?;
    }
    if app.take_undo_request()
        && let Some(path) = app.selected_path().map(Path::to_path_buf)
    {
        let r = vault.undo_in(&path);
        run(r, app, vault)?;
    }
    if app.take_redo_request()
        && let Some(path) = app.selected_path().map(Path::to_path_buf)
    {
        let r = vault.redo_in(&path);
        run(r, app, vault)?;
    }
    Ok(())
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
        let src = app.view_source().unwrap_or_default();
        let title = app
            .selected_path()
            .map_or_else(String::new, |p| p.display().to_string());
        let offset = u16::try_from(app.scroll()).unwrap_or(u16::MAX);
        // Use the tree-sitter powered helper so code blocks inside the
        // displayed source get real highlight spans (KeywordHighlighter
        // by default, pluggable via the trait).
        let highlighted = highlight_org_source(src);
        let view = Paragraph::new(highlighted)
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

    if let Some((title, rows)) = overlay_content(app) {
        draw_overlay_list(f, area, title, rows, app.result_cursor());
    }

    if let Some(lines) = app.popup_lines() {
        draw_whichkey(f, area, app, lines);
    }

    // validate-on-save status: show last config error (rich message with
    // line info from the CUE-style loader) at the bottom when present.
    if let Some(err) = app.config_error() {
        let status_area = ratatui::layout::Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: area.width,
            height: 1,
        };
        let status = Paragraph::new(format!("CONFIG ERROR: {err}"))
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
        f.render_widget(status, status_area);
    }
}

/// Render helper: turn an org source into ratatui `Line`s with styled spans
/// for code block contents (using the pluggable tree-sitter highlighter).
///
/// Delivers "TUI file view renders code blocks with TS-derived highlight spans".
/// Light fence scan (no full org parse in shell required).
#[must_use]
#[allow(clippy::too_long_first_doc_paragraph)]
pub fn highlight_org_source(src: &str) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if trimmed.to_ascii_uppercase().starts_with("#+BEGIN_SRC") {
            // fence line - plain
            out.push(Line::from(Span::raw(line.to_owned())));
            i += 1;

            // parse lang from the begin line (after BEGIN_SRC)
            let lang = trimmed
                .split_whitespace()
                .nth(1)
                .unwrap_or("plain")
                .to_ascii_lowercase();
            let highlighter = closure_tree_sitter::KeywordHighlighter::for_language(&lang);

            // accumulate + highlight block content until #+END_SRC
            let mut block_content = String::new();

            while i < lines.len() {
                let cl = lines[i];
                let ct = cl.trim_start().to_ascii_uppercase();
                if ct.starts_with("#+END_SRC") {
                    break;
                }
                if !block_content.is_empty() {
                    block_content.push('\n');
                }
                block_content.push_str(cl);
                i += 1;
            }

            // highlight the whole content (guarantees the contract)
            let hl = highlighter.highlight(&block_content);

            // For output, split the highlighted content back to original lines
            // and build styled Spans. (Simple per-char map for the ranges.)
            let content_lines: Vec<&str> = block_content.lines().collect();
            let mut hl_cursor = 0usize;
            for &cl in &content_lines {
                let mut line_spans: Vec<Span<'static>> = Vec::new();
                let mut pos = 0usize;
                while pos < cl.len() {
                    // find if current pos is covered by a highlight range (relative to block_content)
                    let abs = hl_cursor + pos;
                    if let Some(h) = hl.iter().find(|h| abs >= h.start && abs < h.end) {
                        let end_in_line = (h.end - hl_cursor).min(cl.len());
                        let piece = &cl[pos..end_in_line];
                        let style = style_for(h.kind);
                        line_spans.push(Span::styled(piece.to_owned(), style));
                        pos = end_in_line;
                    } else {
                        // plain char
                        let end = pos + 1;
                        line_spans.push(Span::raw(cl[pos..end].to_owned()));
                        pos = end;
                    }
                }
                out.push(Line::from(line_spans));
                hl_cursor += cl.len() + 1; // + newline
            }

            // the end fence (if we stopped on it)
            if i < lines.len() {
                out.push(Line::from(Span::raw(lines[i].to_owned())));
                i += 1;
            }
            continue;
        }

        // normal line
        out.push(Line::from(Span::raw(line.to_owned())));
        i += 1;
    }

    if out.is_empty() && !src.is_empty() {
        out.push(Line::from(Span::raw(src.to_owned())));
    }
    out
}

fn style_for(kind: closure_tree_sitter::HighlightKind) -> Style {
    match kind {
        closure_tree_sitter::HighlightKind::Keyword => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        closure_tree_sitter::HighlightKind::Literal => Style::default().fg(Color::Green),
        closure_tree_sitter::HighlightKind::Comment => Style::default().fg(Color::DarkGray),
        closure_tree_sitter::HighlightKind::Identifier => Style::default().fg(Color::Cyan),
        _ => Style::default(),
    }
}

/// Title + rows of the bottom overlay for the active surface, `None`
/// when no overlay is shown.
fn overlay_content(app: &App) -> Option<(String, Vec<String>)> {
    match app.mode() {
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
        AppMode::DbView => Some((
            "database: title | todo | priority".to_owned(),
            app.view_rows()
                .iter()
                .map(|(_, cells)| cells.join("  |  "))
                .collect(),
        )),
        AppMode::EditCell => Some((
            format!("set property KEY=VALUE: {}", app.query()),
            Vec::new(),
        )),
        AppMode::Palette => Some((
            format!("command: {}", app.query()),
            app.palette_results()
                .iter()
                .map(|(cmd, chord)| format!("{cmd:24} {chord}"))
                .collect(),
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
        AppMode::EditBody => Some((
            "edit body — C-s save, ESC cancel".to_owned(),
            app.buffer().lines().map(str::to_owned).collect(),
        )),
        AppMode::Agenda => Some((
            "agenda (SCHEDULED / DEADLINE)".to_owned(),
            app.agenda_results()
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        )),
        AppMode::BodySearch => Some((
            format!("body search: {}", app.query()),
            app.body_results().into_iter().map(|(_, t)| t).collect(),
        )),
        AppMode::Blocks => Some((
            app.selected_path().map_or_else(
                || "code blocks".to_owned(),
                |p| format!("code blocks: {}", p.display()),
            ),
            app.block_results()
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        )),
        AppMode::Browse | AppMode::FileView => None,
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
