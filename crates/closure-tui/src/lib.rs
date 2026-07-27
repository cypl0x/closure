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
const DEFAULT_BINDINGS: &[(&str, &str)] =
    closure_input::mode_keymap(closure_config::InputMode::Doom);

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadlineRecord {
    /// File containing the headline.
    pub path: PathBuf,
    /// Stable block id (`:ID:`, I2).
    pub id: String,
    /// Headline title.
    pub title: String,
    /// Headline body text (for inline editing).
    pub body: String,
    /// TODO keyword, when the headline carries one.
    pub todo: Option<String>,
    /// `[#A]`-style priority letter.
    pub priority: Option<char>,
    /// Headline tags, without the surrounding colons.
    pub tags: Vec<String>,
    /// Whether `:VISIBILITY: folded` is set (the fold state).
    pub folded: bool,
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
    /// The link graph: hubs, orphans and dead links in one pane.
    Graph,
    /// The recorded command journal.
    Journal,
    /// Scheduled jobs declared in the vault.
    Cron,
    /// Typing the selected headline's tags, space-separated.
    EditTags,
    /// Observed network flows with their allow/block verdict.
    Sniffer,
    /// Sync peers and their connection state.
    Sync,
    /// The LLM transcript, with a composer for the next question.
    Llm,
    /// Merge conflicts, each offering ours/theirs.
    Conflicts,
    /// The undo history of the selected file as a jumpable list.
    UndoHistory,
    /// The vim-style `:` command line. Understands the small set of
    /// commands muscle memory reaches for (`:w`, `:q`, `:wq`, `:x`)
    /// and falls through to any command name, so it is a superset of
    /// the palette rather than a replacement for it.
    Ex,
}

/// One unresolved merge conflict as the shell shows it: which block and
/// field diverged, and the two candidate values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictRow {
    /// Block id the conflict is on.
    pub block: String,
    /// Conflicting field — `title` or `body`.
    pub field: String,
    /// The local value.
    pub ours: String,
    /// The incoming value.
    pub theirs: String,
}

/// The link graph as three lists: hubs `(id, title, inbound)`,
/// orphans `(id, title)`, and dead link targets.
pub type LinkGraph = (
    Vec<(String, String, usize)>,
    Vec<(String, String)>,
    Vec<String>,
);

/// Elm-style application state for the terminal shell. Strokes go in
/// via [`Self::handle_stroke`]; rendering reads the accessors. No
/// terminal I/O lives here, which keeps every transition testable.
// The flags are independent one-shot signals to the driver (quit, undo,
// redo) plus two display toggles — grouping them into sub-structs would
// only add a level of indirection to a flat state bag.
#[allow(clippy::struct_excessive_bools)]
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
    /// Last message from the `:` command line, for the status line.
    status: String,
    /// Link-graph rows, pushed in by the driver like everything else:
    /// hubs `(id, title, inbound)`, orphans `(id, title)`, dead links.
    graph: LinkGraph,
    /// Recorded journal entries.
    journal: Vec<String>,
    /// Scheduled jobs, `(spec, command)`.
    cron: Vec<(String, String)>,
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
    /// The shared modal editor behind `EditBody` — the same one the
    /// gpui shell drives, so both shells speak one vim grammar (I4).
    body: closure_shell_core::BodyEditor,
    body_target: Option<String>,
    body_request: Option<(String, String)>,
    struct_request: Option<(String, String)>,
    move_request: Option<(String, String)>,
    cut_request: Option<String>,
    paste_request: Option<String>,
    agenda: Vec<(PathBuf, String)>,
    /// Which headline of the selected file the editing chords act on.
    /// The Headlines list moves it; Browse-level chords read it, so
    /// `t`/`p`/`z` hit the headline the user last looked at.
    head_cursor: usize,
    todo_request: Option<(String, Option<String>)>,
    priority_request: Option<(String, Option<char>)>,
    tags_request: Option<(String, Vec<String>)>,
    /// Undo-history rows for the selected file as `(label, is_current)`.
    history: Vec<(String, bool)>,
    history_request: Option<usize>,
    /// Headline the open minibuffer (tags) is editing.
    field_target: Option<String>,
    /// Observed flows as `(candidate, verdict)`, pushed by the driver.
    sniffer: Vec<(String, String)>,
    /// `(candidate, allow)` rule the user toggled.
    flow_request: Option<(String, bool)>,
    /// Sync peers as `(address, state)`.
    peers: Vec<(String, String)>,
    /// The LLM transcript as `(role, text)`.
    chat: Vec<(String, String)>,
    /// Question the user composed for the model.
    ask_request: Option<String>,
    /// Whether LLM answers render as org rather than raw text.
    llm_render: bool,
    /// Whether the LLM pane is composing rather than reading.
    llm_composing: bool,
    /// The live dabbrev cycle: where the prefix starts, the candidates,
    /// and which one is currently in the buffer.
    completion: Option<(usize, Vec<String>, usize)>,
    conflicts: Vec<ConflictRow>,
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
            status: String::new(),
            graph: (Vec::new(), Vec::new(), Vec::new()),
            journal: Vec::new(),
            cron: Vec::new(),
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
            body: closure_shell_core::BodyEditor::new(),
            body_target: None,
            body_request: None,
            struct_request: None,
            move_request: None,
            cut_request: None,
            paste_request: None,
            agenda: Vec::new(),
            head_cursor: 0,
            todo_request: None,
            priority_request: None,
            tags_request: None,
            history: Vec::new(),
            history_request: None,
            field_target: None,
            sniffer: Vec::new(),
            flow_request: None,
            peers: Vec::new(),
            chat: Vec::new(),
            ask_request: None,
            llm_render: true,
            llm_composing: false,
            completion: None,
            conflicts: Vec::new(),
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
        self.body.text()
    }

    /// Consume the `(id, keyword)` TODO change the user asked for.
    pub const fn take_todo_request(&mut self) -> Option<(String, Option<String>)> {
        self.todo_request.take()
    }

    /// Consume the `(id, priority)` change the user asked for.
    pub const fn take_priority_request(&mut self) -> Option<(String, Option<char>)> {
        self.priority_request.take()
    }

    /// Consume the `(id, tags)` change the user asked for.
    pub const fn take_tags_request(&mut self) -> Option<(String, Vec<String>)> {
        self.tags_request.take()
    }

    /// Consume the undo-history index the user jumped to.
    pub const fn take_history_request(&mut self) -> Option<usize> {
        self.history_request.take()
    }

    /// Provide the observed flows as `(candidate, verdict)`.
    pub fn set_sniffer(&mut self, flows: Vec<(String, String)>) {
        self.sniffer = flows;
    }

    /// The sniffer rows, in order.
    #[must_use]
    pub fn sniffer_rows(&self) -> Vec<String> {
        self.sniffer
            .iter()
            .map(|(candidate, verdict)| format!("{verdict:<6} {candidate}"))
            .collect()
    }

    /// Consume the `(candidate, allow)` flow rule the user toggled.
    pub const fn take_flow_request(&mut self) -> Option<(String, bool)> {
        self.flow_request.take()
    }

    /// Provide the sync peers as `(address, state)`.
    pub fn set_peers(&mut self, peers: Vec<(String, String)>) {
        self.peers = peers;
    }

    /// The peer rows, in order.
    #[must_use]
    pub fn peer_rows(&self) -> Vec<String> {
        self.peers
            .iter()
            .map(|(addr, state)| format!("{addr}  [{state}]"))
            .collect()
    }

    /// Provide the LLM transcript as `(role, text)`.
    pub fn set_chat(&mut self, chat: Vec<(String, String)>) {
        self.chat = chat;
    }

    /// The transcript rows, in order.
    #[must_use]
    pub fn chat_rows(&self) -> Vec<String> {
        self.chat
            .iter()
            .map(|(role, text)| format!("{role}: {text}"))
            .collect()
    }

    /// Consume the question the user composed for the model.
    pub const fn take_ask_request(&mut self) -> Option<String> {
        self.ask_request.take()
    }

    /// Whether LLM answers render as org rather than raw text.
    #[must_use]
    pub const fn llm_render(&self) -> bool {
        self.llm_render
    }

    /// The rows of the open subsystem pane, or its empty state.
    ///
    /// An empty list is never shown bare: the terminal binary does not
    /// run the sniffer, the sync transport or a model client yet, and a
    /// blank box reads as a broken pane rather than an unfed one.
    #[must_use]
    pub fn pane_rows(&self) -> Vec<String> {
        let (rows, empty) = match self.mode {
            AppMode::Sniffer => (
                self.sniffer_rows(),
                "no flows — the terminal shell does not run the sniffer yet \
                 (closure sniff --live, or the gpui shell)",
            ),
            AppMode::Sync => (
                self.peer_rows(),
                "no peer — the terminal shell does not dial the sync transport yet \
                 (the gpui shell pairs)",
            ),
            AppMode::Llm => (
                self.chat_rows(),
                "nothing asked — the terminal shell has no model client yet \
                 (config.org endpoint, or the gpui shell)",
            ),
            AppMode::Conflicts => (
                self.conflict_rows(),
                "no conflict — merges arrive over sync, which this shell does not run yet",
            ),
            // Graph, journal and cron are not here: each already labels
            // its own empty state, and more usefully — the journal names
            // the config knob that turns recording on.
            _ => return Vec::new(),
        };
        if rows.is_empty() {
            vec![empty.to_owned()]
        } else {
            rows
        }
    }

    /// Provide the unresolved merge conflicts.
    pub fn set_conflicts(&mut self, conflicts: Vec<ConflictRow>) {
        self.conflicts = conflicts;
    }

    /// The conflict rows, in order — both sides on one line, because a
    /// terminal has no room for the GUI's three-pane diff.
    #[must_use]
    pub fn conflict_rows(&self) -> Vec<String> {
        self.conflicts
            .iter()
            .map(|c| {
                format!(
                    "{} {}: ours={:?} theirs={:?}",
                    c.block, c.field, c.ours, c.theirs
                )
            })
            .collect()
    }

    /// Provide the selected file's undo history as `(label, is_current)`
    /// rows, oldest first — pushed in by the driver like every pane.
    pub fn set_history(&mut self, history: Vec<(String, bool)>) {
        self.history = history;
    }

    /// The undo-history rows, in order.
    #[must_use]
    pub fn history_rows(&self) -> Vec<String> {
        self.history
            .iter()
            .map(|(label, current)| {
                let mark = if *current { "*" } else { " " };
                format!("{mark} {label}")
            })
            .collect()
    }

    /// The headline the editing chords act on: the one under the
    /// headline cursor in the selected file, if any.
    fn current_headline(&self) -> Option<&HeadlineRecord> {
        let path = self.selected_path()?;
        let mut of_file = self.headlines.iter().filter(|r| r.path.as_path() == path);
        let nth = self.head_cursor.min(
            self.headlines
                .iter()
                .filter(|r| r.path.as_path() == path)
                .count()
                .saturating_sub(1),
        );
        of_file.nth(nth)
    }

    /// The block id of [`Self::current_headline`], or a status line
    /// saying why there is none.
    fn current_headline_id(&mut self) -> Option<String> {
        let id = self.current_headline().map(|r| r.id.clone());
        if id.is_none() {
            "no headline here — open a file with headlines first".clone_into(&mut self.status);
        }
        id
    }

    /// The body editor's vim mode, for the mode indicator.
    #[must_use]
    pub const fn body_mode(&self) -> closure_shell_core::EditorMode {
        self.body.mode()
    }

    /// The body editor's cursor as `(line, column)`, both 0-based —
    /// where the terminal parks its caret.
    #[must_use]
    pub fn body_cursor(&self) -> (usize, usize) {
        self.body.cursor_line_col()
    }

    /// Park the body cursor at a byte offset (clamped to a char
    /// boundary) — the mouse/goto entry point.
    pub fn set_body_cursor(&mut self, byte: usize) {
        self.body.set_cursor_byte(byte);
    }

    /// The body editor's buffer.
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        self.body.text()
    }

    /// The body editor's open `/` search line, if one is.
    #[must_use]
    pub fn body_search_prompt(&self) -> Option<String> {
        self.body.search_prompt()
    }

    /// The body editor's status line: the vim mode, the chord in
    /// progress, and the keys that leave the surface.
    #[must_use]
    pub fn body_status(&self) -> String {
        use closure_shell_core::EditorMode;
        let mode = match self.body.mode() {
            EditorMode::Insert => "INSERT",
            EditorMode::Normal => "NORMAL",
            EditorMode::Visual => "VISUAL",
            EditorMode::VisualLine => "VISUAL LINE",
        };
        let pending = self.body.pending_chord();
        let chord = if pending.is_empty() {
            String::new()
        } else {
            format!("  {pending}")
        };
        // A live dabbrev cycle replaces the hint: which candidate of how
        // many is the only thing worth the width while cycling.
        let tail = match &self.completion {
            Some((_, items, ix)) if *ix != usize::MAX => {
                format!("completion {}/{}  C-n/C-p cycle", ix + 1, items.len())
            }
            _ => closure_shell_core::editor_hint(self.body.mode()).to_owned(),
        };
        let (line, col) = self.body.cursor_line_col();
        format!(
            "-- {mode} --{chord}   {}:{}   {tail}   C-s save",
            line + 1,
            col + 1,
        )
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

    /// Last message from the `:` command line (status line).
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Push the link graph in, the way paths and backlinks arrive.
    pub fn set_graph(
        &mut self,
        hubs: Vec<(String, String, usize)>,
        orphans: Vec<(String, String)>,
        dead: Vec<String>,
    ) {
        self.graph = (hubs, orphans, dead);
    }

    /// Push in the recorded journal.
    pub fn set_journal(&mut self, entries: Vec<String>) {
        self.journal = entries;
    }

    /// Push in the scheduled jobs.
    pub fn set_cron(&mut self, jobs: Vec<(String, String)>) {
        self.cron = jobs;
    }

    /// The graph pane's lines: three labelled sections, because three
    /// lists in one pane are unreadable without saying which is which.
    #[must_use]
    pub fn graph_rows(&self) -> Vec<String> {
        let (hubs, orphans, dead) = &self.graph;
        let mut out = vec!["-- hubs (most linked to) --".to_owned()];
        if hubs.is_empty() {
            out.push("  (nothing links anywhere yet)".to_owned());
        }
        out.extend(hubs.iter().map(|(_, title, n)| format!("{n:>4}  {title}")));
        out.push("-- orphans (nothing links here) --".to_owned());
        if orphans.is_empty() {
            out.push("  (none)".to_owned());
        }
        out.extend(orphans.iter().map(|(_, title)| format!("      {title}")));
        out.push("-- dead links --".to_owned());
        if dead.is_empty() {
            out.push("  (none)".to_owned());
        }
        out.extend(dead.iter().map(|d| format!("      {d}")));
        out
    }

    /// The journal pane's lines, or a line saying it is empty — a
    /// blank pane is indistinguishable from a broken one.
    #[must_use]
    pub fn journal_rows(&self) -> Vec<String> {
        if self.journal.is_empty() {
            return vec!["no recorded commands (set record_commands = true)".to_owned()];
        }
        self.journal.clone()
    }

    /// The scheduled-jobs pane's lines.
    #[must_use]
    pub fn cron_rows(&self) -> Vec<String> {
        if self.cron.is_empty() {
            return vec!["no scheduled jobs in this vault".to_owned()];
        }
        self.cron
            .iter()
            .map(|(spec, command)| format!("{spec:16} {command}"))
            .collect()
    }

    /// Run a command by name, for tests that assert a shared keymap
    /// entry is actually implemented here too.
    pub fn apply_command_for_test(&mut self, command: &str) {
        self.apply_command(command);
    }

    /// Current config validation error to surface in the TUI (status line).
    #[must_use]
    pub fn config_error(&self) -> Option<&str> {
        self.last_config_error.as_deref()
    }

    /// Feed one key stroke into the active surface: query editing in
    /// search mode, the chord trie otherwise.
    pub fn handle_stroke(&mut self, stroke: &str) {
        // A surface that owns the keyboard handles the stroke and the
        // chord trie never sees it; Browse is the only mode that falls
        // through to the bindings.
        match self.mode {
            AppMode::Search | AppMode::SearchHeadlines => {
                return self.handle_search_stroke(stroke);
            }
            AppMode::FileView => return self.handle_view_stroke(stroke),
            AppMode::Backlinks => return self.handle_backlinks_stroke(stroke),
            AppMode::Capture => return self.handle_capture_stroke(stroke),
            AppMode::Headlines => return self.handle_headlines_stroke(stroke),
            AppMode::Rename => return self.handle_rename_stroke(stroke),
            AppMode::AddHeadline => return self.handle_add_stroke(stroke),
            AppMode::Palette => return self.handle_palette_stroke(stroke),
            AppMode::Ex => return self.handle_ex_stroke(stroke),
            AppMode::Graph | AppMode::Journal | AppMode::Cron => {
                return self.handle_pane_stroke(stroke);
            }
            AppMode::DbView => return self.handle_dbview_stroke(stroke),
            AppMode::Blocks => return self.handle_blocks_stroke(stroke),
            AppMode::EditBody => return self.handle_editbody_stroke(stroke),
            AppMode::Agenda => return self.handle_agenda_stroke(stroke),
            AppMode::BodySearch => return self.handle_bodysearch_stroke(stroke),
            AppMode::EditCell => return self.handle_editcell_stroke(stroke),
            AppMode::EditTags => return self.handle_edittags_stroke(stroke),
            AppMode::UndoHistory => return self.handle_history_stroke(stroke),
            AppMode::ConfirmDelete => return self.handle_confirm_stroke(stroke),
            // The subsystem panes own their navigation keys but let
            // everything else reach the trie, so the chords that act on
            // the cursor row (`g b`, `g o`, `g t`) still fire.
            AppMode::Sniffer => {
                if self.handle_list_pane_stroke(stroke, self.sniffer.len()) {
                    return;
                }
            }
            AppMode::Sync => {
                if self.handle_list_pane_stroke(stroke, self.peers.len()) {
                    return;
                }
            }
            AppMode::Conflicts => {
                if self.handle_list_pane_stroke(stroke, self.conflicts.len()) {
                    return;
                }
            }
            AppMode::Llm => {
                if self.handle_llm_stroke(stroke) {
                    return;
                }
            }
            AppMode::Browse => {}
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
                self.head_cursor = self.result_cursor;
            }
            "k" | "<up>" => {
                self.result_cursor = self.result_cursor.saturating_sub(1);
                self.head_cursor = self.result_cursor;
            }
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
                    self.body.load(body);
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

    /// Feed one stroke to the shared modal editor.
    ///
    /// Terminal strokes arrive in the emacs notation the chord trie uses
    /// (`ESC`, `RET`, `<down>`, `C-s`); [`BodyEditor`] speaks the shell
    /// vocabulary (`escape`, `down`). Translating here is what lets one
    /// editor serve both shells.
    ///
    /// [`BodyEditor`]: closure_shell_core::BodyEditor
    fn handle_editbody_stroke(&mut self, stroke: &str) {
        use closure_shell_core::EditorMode;
        // Saving and cancelling are the surface's own keys in every mode.
        if stroke == "C-s" {
            if let Some(id) = self.body_target.take() {
                self.body_request = Some((id, self.body.text().to_owned()));
            }
            self.mode = AppMode::Browse;
            self.body.clear();
            return;
        }
        if self.body.mode() == EditorMode::Insert {
            return self.editbody_insert_stroke(stroke);
        }
        // Esc on a quiet Normal surface cancels the edit; mid-chord (or
        // in Visual) it belongs to the editor — the same rule the gpui
        // shell applies.
        if stroke == "ESC"
            && self.body.mode() == EditorMode::Normal
            && self.body.pending_stroke().is_none()
            && self.body.pending_count() == 0
        {
            self.mode = AppMode::Browse;
            self.body.clear();
            self.body_target = None;
            return;
        }
        if stroke == "C-r" {
            self.body.redo_local();
            return;
        }
        if let Some(key) = Self::modal_key_of(stroke) {
            self.body.modal_key(&key);
        }
    }

    /// `C-n`/`C-p`: cycle dabbrev candidates for the word being typed.
    ///
    /// The candidates come from the document sources the driver already
    /// pushes in, so the terminal app needs no vault of its own.
    fn cycle_completion(&mut self, forward: bool) {
        let session = self.completion.take().or_else(|| {
            let prefix = self.body.word_prefix().to_owned();
            let items = closure_shell_core::body_completions_from(
                &prefix,
                self.sources.iter().map(|(_, s)| s.as_str()),
            );
            (!items.is_empty()).then(|| (self.body.word_start(), items, usize::MAX))
        });
        let Some((start, items, ix)) = session else {
            return;
        };
        // usize::MAX marks a fresh cycle, so the first C-n takes item 0
        // and the first C-p takes the last.
        let next = if ix == usize::MAX {
            if forward { 0 } else { items.len() - 1 }
        } else if forward {
            (ix + 1) % items.len()
        } else {
            (ix + items.len() - 1) % items.len()
        };
        self.body.replace_to_cursor(start, &items[next]);
        self.completion = Some((start, items, next));
    }

    /// Insert-mode strokes: readline chords, then plain text.
    fn editbody_insert_stroke(&mut self, stroke: &str) {
        // Any stroke other than cycling ends the completion cycle.
        if !matches!(stroke, "C-n" | "C-p") {
            self.completion = None;
        }
        match stroke {
            "C-n" => return self.cycle_completion(true),
            "C-p" => return self.cycle_completion(false),
            _ => {}
        }
        match stroke {
            "ESC" => self.body.to_normal(),
            "RET" => self.body.insert_char('\n'),
            "SPC" => self.body.insert_char(' '),
            "TAB" => self.body.tempo_expand_or_indent(),
            "DEL" => self.body.backspace(),
            "<up>" => self.body.up(),
            "<down>" => self.body.down(),
            "<pageup>" => self.body.page(false, 20),
            "<pagedown>" => self.body.page(true, 20),
            // The readline set every "normal input field" answers to,
            // sharing its arms with the named keys.
            "<left>" | "C-b" => self.body.left(),
            "<right>" | "C-f" => self.body.right(),
            "<home>" | "C-a" => self.body.line_home(),
            "<end>" | "C-e" => self.body.line_end_motion(),
            "<delete>" | "C-d" => self.body.delete_at(),
            "C-k" => self.body.kill_rest_of_line(),
            "C-u" => self.body.kill_to_line_start(),
            "C-w" => self.body.delete_word_back(),
            "C-y" => self.body.yank_insert(),
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.body.insert_char(c);
                }
            }
        }
    }

    /// Translate a terminal stroke into the modal-editor key name, or
    /// `None` when the editor has no use for it.
    fn modal_key_of(stroke: &str) -> Option<String> {
        Some(match stroke {
            "ESC" => "escape".to_owned(),
            "<up>" => "up".to_owned(),
            "<down>" => "down".to_owned(),
            "<left>" => "left".to_owned(),
            "<right>" => "right".to_owned(),
            "<home>" => "home".to_owned(),
            "<end>" => "end".to_owned(),
            "<delete>" => "delete".to_owned(),
            "<pageup>" => "pageup".to_owned(),
            "<pagedown>" => "pagedown".to_owned(),
            "RET" => "enter".to_owned(),
            "SPC" => " ".to_owned(),
            "DEL" => "backspace".to_owned(),
            s if s.chars().count() == 1 => s.to_owned(),
            // `C-d`, `C-f`, `C-a` … are chords of the editor's own
            // grammar (scroll, increment). Dropped here, they were dead
            // in the terminal while working in the GUI.
            s if s.len() == 3 && s.starts_with("C-") => s.to_owned(),
            _ => return None,
        })
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

    /// Open one of the read-only panes.
    fn open_pane(&mut self, cmd: &str) {
        self.mode = match cmd {
            "graph" => AppMode::Graph,
            "journal" => AppMode::Journal,
            _ => AppMode::Cron,
        };
        self.result_cursor = 0;
    }

    /// `y` confirms the pending delete; anything else backs out to the
    /// list it was started from.
    fn handle_confirm_stroke(&mut self, stroke: &str) {
        if stroke == "y" {
            self.delete_request = self.delete_target.take();
            self.mode = AppMode::Browse;
            self.result_cursor = 0;
        } else {
            self.delete_target = None;
            self.mode = AppMode::Headlines;
        }
    }

    /// Strokes for the read-only panes: a cursor and a way out.
    fn handle_pane_stroke(&mut self, stroke: &str) {
        let len = match self.mode {
            AppMode::Graph => self.graph_rows().len(),
            AppMode::Journal => self.journal_rows().len(),
            _ => self.cron_rows().len(),
        };
        match stroke {
            "ESC" | "q" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            "j" | "<down>" => {
                self.result_cursor = (self.result_cursor + 1).min(len.saturating_sub(1));
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            _ => {}
        }
    }

    /// Keys for the `:` line. Typing edits it, `RET` runs it, `ESC`
    /// abandons it, and deleting past the start closes it.
    fn handle_ex_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "DEL" if self.query.pop().is_none() => self.mode = AppMode::Browse,
            "DEL" => {}
            "RET" => {
                let line = std::mem::take(&mut self.query);
                self.mode = AppMode::Browse;
                self.run_ex(line.trim());
            }
            "SPC" => self.query.push(' '),
            s if s.chars().count() == 1 => {
                self.query.push_str(s);
            }
            _ => {}
        }
    }

    /// Execute an ex command: the vim set first, then any command name.
    fn run_ex(&mut self, line: &str) {
        match line {
            "" => {}
            "q" | "q!" | "quit" => self.quit = true,
            "w" | "write" | "wq" | "x" | "wq!" | "x!" => {
                // Every edit is written through the kernel as it
                // happens (I8), so there is genuinely nothing to flush
                // — and reporting a write that never happened would be
                // a lie.
                "the vault is written on every edit — nothing to save".clone_into(&mut self.status);
                if line.starts_with("wq") || line.starts_with('x') {
                    self.quit = true;
                }
            }
            other => {
                let known = self.bindings.iter().any(|(_, cmd)| cmd == other);
                if known {
                    self.apply_command(other);
                } else {
                    self.status = format!("not an editor command: {other}");
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

    /// `M-k`/`M-j`: reorder the cursor headline among its siblings.
    ///
    /// The vault only knows `move_after`, so moving up is "put the
    /// previous sibling below us" — the same rule the GUI applies.
    fn move_subtree(&mut self, up: bool) {
        let ids: Vec<String> = self
            .file_headlines()
            .iter()
            .map(|(_, id)| (*id).to_owned())
            .collect();
        let here = self.head_cursor.min(ids.len().saturating_sub(1));
        let other = if up {
            here.checked_sub(1)
        } else {
            Some(here + 1).filter(|n| *n < ids.len())
        };
        let (Some(other), true) = (other, !ids.is_empty()) else {
            return;
        };
        // Whoever ends up second in the pair is the one that moves.
        self.move_request = Some(if up {
            (ids[other].clone(), ids[here].clone())
        } else {
            (ids[here].clone(), ids[other].clone())
        });
    }

    fn handle_edittags_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.field_target = None;
            }
            "RET" => {
                if let Some(id) = self.field_target.take() {
                    let tags: Vec<String> =
                        self.query.split_whitespace().map(str::to_owned).collect();
                    self.tags_request = Some((id, tags));
                }
                self.mode = AppMode::Browse;
                self.query.clear();
            }
            "SPC" => self.query.push(' '),
            "DEL" => {
                self.query.pop();
            }
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                }
            }
        }
    }

    /// The sniffer, sync and conflict panes are cursor lists over rows
    /// the driver pushed; only the row source differs.
    ///
    /// Returns `false` for strokes the pane does not own, so they fall
    /// through to the chord trie — that is what keeps `g b`, `g o` and
    /// `g t` alive while a pane holds the screen.
    fn handle_list_pane_stroke(&mut self, stroke: &str, len: usize) -> bool {
        match stroke {
            "j" | "<down>" => {
                self.result_cursor = (self.result_cursor + 1).min(len.saturating_sub(1));
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "ESC" | "q" | "h" | "DEL" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            _ => return false,
        }
        true
    }

    /// The LLM pane reads the transcript until `i` starts composing;
    /// then it is a minibuffer that sends on RET.
    fn handle_llm_stroke(&mut self, stroke: &str) -> bool {
        if !self.llm_composing {
            if stroke == "i" {
                self.llm_composing = true;
                self.query.clear();
                return true;
            }
            return self.handle_list_pane_stroke(stroke, self.chat.len());
        }
        match stroke {
            "ESC" => {
                self.llm_composing = false;
                self.query.clear();
            }
            "RET" => {
                if !self.query.is_empty() {
                    self.ask_request = Some(std::mem::take(&mut self.query));
                }
                self.llm_composing = false;
            }
            "SPC" => self.query.push(' '),
            "DEL" => {
                self.query.pop();
            }
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                }
            }
        }
        true
    }

    /// `g o` / `g t`: take one side of the cursor conflict. The write
    /// rides the rename/body channels the shell already drains, so
    /// conflict resolution is an ordinary edit (I8).
    fn resolve_conflict(&mut self, ours: bool) {
        let Some(c) = self.conflicts.get(self.result_cursor).cloned() else {
            "no conflict here — g m lists them".clone_into(&mut self.status);
            return;
        };
        let value = if ours { c.ours } else { c.theirs };
        if c.field == "body" {
            self.body_request = Some((c.block, value));
        } else {
            self.rename_request = Some((c.block, value));
        }
        self.conflicts.remove(self.result_cursor);
        self.result_cursor = self
            .result_cursor
            .min(self.conflicts.len().saturating_sub(1));
    }

    fn handle_history_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self.history.len().saturating_sub(1);
                self.result_cursor = (self.result_cursor + 1).min(last);
            }
            "k" | "<up>" => self.result_cursor = self.result_cursor.saturating_sub(1),
            "RET" => {
                if !self.history.is_empty() {
                    self.history_request = Some(self.result_cursor);
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
            "ex-command" => {
                self.mode = AppMode::Ex;
                self.query.clear();
                self.status.clear();
            }
            "graph" | "journal" | "cron" => self.open_pane(cmd),
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
            other => self.apply_headline_command(other),
        }
    }

    /// The commands that act on the cursor headline or open a subsystem
    /// pane. Split from [`Self::apply_command`] only for length; the
    /// two together are the shell's whole command vocabulary.
    fn apply_headline_command(&mut self, cmd: &str) {
        match cmd {
            // --- Headline edits. Each resolves the cursor headline and
            // parks a request; the driver performs the vault write.
            "toggle-todo" => {
                let next = match self.current_headline().and_then(|r| r.todo.as_deref()) {
                    None => Some("TODO".to_owned()),
                    Some("TODO") => Some("DONE".to_owned()),
                    Some(_) => None,
                };
                if let Some(id) = self.current_headline_id() {
                    self.todo_request = Some((id, next));
                }
            }
            "cycle-priority" => {
                let next = match self.current_headline().and_then(|r| r.priority) {
                    None => Some('A'),
                    Some('A') => Some('B'),
                    Some('B') => Some('C'),
                    Some(_) => None,
                };
                if let Some(id) = self.current_headline_id() {
                    self.priority_request = Some((id, next));
                }
            }
            "edit-tags" => {
                let tags = self
                    .current_headline()
                    .map(|r| r.tags.join(" "))
                    .unwrap_or_default();
                if let Some(id) = self.current_headline_id() {
                    self.field_target = Some(id);
                    self.query = tags;
                    self.mode = AppMode::EditTags;
                }
            }
            "toggle-fold" => {
                // Folding is `:VISIBILITY:` in the file, so it rides the
                // property channel the GUI uses (I1: it round-trips).
                let next = if self.current_headline().is_some_and(|r| r.folded) {
                    "all"
                } else {
                    "folded"
                };
                if let Some(id) = self.current_headline_id() {
                    self.property_request = Some((id, "VISIBILITY".to_owned(), next.to_owned()));
                }
            }
            "promote" | "demote" => {
                if let Some(id) = self.current_headline_id() {
                    self.struct_request = Some((cmd.to_owned(), id));
                }
            }
            "move-subtree-up" | "move-subtree-down" => self.move_subtree(cmd == "move-subtree-up"),
            "add-heading" => {
                if let Some(id) = self.current_headline_id() {
                    self.add_request = Some((id, "untitled".to_owned()));
                }
            }
            "add-sibling" => {
                if let Some(id) = self.current_headline_id() {
                    self.add_target = Some(id);
                    self.query.clear();
                    self.mode = AppMode::AddHeadline;
                }
            }
            "rename" => {
                let title = self
                    .current_headline()
                    .map(|r| r.title.clone())
                    .unwrap_or_default();
                if let Some(id) = self.current_headline_id() {
                    self.rename_target = Some(id);
                    self.query = title;
                    self.mode = AppMode::Rename;
                }
            }
            "delete" => {
                if let Some(id) = self.current_headline_id() {
                    self.delete_target = Some(id);
                    self.mode = AppMode::ConfirmDelete;
                }
            }
            // `edit-special` opens the same editor: in a terminal the
            // body *is* the special buffer, so pointing them at one
            // surface is honest rather than a second half-editor.
            "edit-body" | "edit-special" => {
                let body = self
                    .current_headline()
                    .map(|r| r.body.clone())
                    .unwrap_or_default();
                if let Some(id) = self.current_headline_id() {
                    self.body_target = Some(id);
                    self.body.load(body);
                    self.mode = AppMode::EditBody;
                }
            }
            "edit-property" => {
                if let Some(id) = self.current_headline_id() {
                    self.cell_target = Some(id);
                    self.query.clear();
                    self.mode = AppMode::EditCell;
                }
            }
            other => self.apply_pane_command(other),
        }
    }

    /// The commands that open a subsystem pane or act on the row one
    /// holds. Split from [`Self::apply_headline_command`] for length.
    fn apply_pane_command(&mut self, cmd: &str) {
        match cmd {
            "eval-block" => {
                let target = self
                    .selected_path()
                    .filter(|_| !self.block_results().is_empty())
                    .map(Path::to_path_buf);
                match target {
                    Some(path) => self.eval_request = Some((path, self.result_cursor)),
                    None => {
                        "eval-block: this file has no source blocks".clone_into(&mut self.status);
                    }
                }
            }
            "undo-history" => {
                self.mode = AppMode::UndoHistory;
                self.result_cursor = self
                    .history
                    .iter()
                    .position(|(_, current)| *current)
                    .unwrap_or(0);
            }
            // --- Subsystem panes. The driver owns the sockets and the
            // model client; the pane only shows rows and parks requests.
            "sniffer" => {
                self.mode = AppMode::Sniffer;
                self.result_cursor = 0;
            }
            "sync" => {
                self.mode = AppMode::Sync;
                self.result_cursor = 0;
            }
            "llm" => {
                self.mode = AppMode::Llm;
                self.result_cursor = 0;
                self.llm_composing = false;
            }
            "conflicts" => {
                self.mode = AppMode::Conflicts;
                self.result_cursor = 0;
            }
            // The rail's home button, as a command: the GUIs paint it,
            // and here it is `g h` — the way back to the file list from
            // whichever pane you are in.
            "browse" => {
                self.mode = AppMode::Browse;
                self.result_cursor = 0;
            }
            // The two shapes of the shell, as far as a terminal has
            // them: the list, or the file whole. The GUI's editor view
            // is editable; this one is the reader the terminal already
            // had, which is the same *choice* — file or list — without
            // claiming an editor it does not have here yet.
            "toggle-view" => {
                if self.mode == AppMode::FileView {
                    self.mode = AppMode::Browse;
                    self.scroll = 0;
                } else if self
                    .selected_path()
                    .is_some_and(|sel| self.sources.iter().any(|(p, _)| p.as_path() == sel))
                {
                    self.mode = AppMode::FileView;
                    self.scroll = 0;
                } else {
                    "no file to open".clone_into(&mut self.status);
                }
            }
            "block-flow" | "allow-flow" => match self.sniffer.get(self.result_cursor) {
                Some((candidate, _)) => {
                    self.flow_request = Some((candidate.clone(), cmd == "allow-flow"));
                }
                None => "no flow here — g n lists the observed flows".clone_into(&mut self.status),
            },
            "resolve-ours" => self.resolve_conflict(true),
            "resolve-theirs" => self.resolve_conflict(false),
            "toggle-llm-render" => self.llm_render = !self.llm_render,
            // A chord the keymap advertises but this shell cannot serve
            // must say so — silence reads as a broken keyboard (I4).
            other => self.status = format!("{other}: not available in the terminal shell"),
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
        KeyCode::Left => "<left>".to_owned(),
        KeyCode::Right => "<right>".to_owned(),
        KeyCode::Home => "<home>".to_owned(),
        KeyCode::End => "<end>".to_owned(),
        KeyCode::Delete => "<delete>".to_owned(),
        KeyCode::PageUp => "<pageup>".to_owned(),
        KeyCode::PageDown => "<pagedown>".to_owned(),
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

/// Render a shared [`closure_shell_core::Node`] view tree to text lines
/// (V1b).
///
/// The TUI is one embedder of the declarative tree the engine emits —
/// the same `Node` the web shell renders. Hermetic (no terminal): the
/// ratatui draw path paints these lines. Actionable nodes show their
/// chord in `[..]` (the "keybinding everywhere" rule).
#[must_use]
pub fn render_view(node: &closure_shell_core::Node) -> Vec<String> {
    let mut out = Vec::new();
    push_view_node(node, 0, &mut out);
    out
}

/// Render a [`closure_shell_core::Node`] to a single deterministic text
/// snapshot (V10a): [`render_view`] lines joined by `\n`. The headless
/// render harness — golden-testable with no terminal.
#[must_use]
pub fn render_snapshot(node: &closure_shell_core::Node) -> String {
    render_view(node).join("\n")
}

fn push_view_node(node: &closure_shell_core::Node, depth: usize, out: &mut Vec<String>) {
    use closure_shell_core::Node;
    let pad = "  ".repeat(depth);
    match node {
        Node::Pane { title, children } => {
            out.push(format!("{pad}# {title}"));
            for c in children {
                push_view_node(c, depth + 1, out);
            }
        }
        Node::Rows { rows, selected } => {
            for (i, r) in rows.iter().enumerate() {
                let mark = if i == *selected { '>' } else { ' ' };
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(String::new, |t| format!("{t} "));
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(String::new, |g| format!("{g} "));
                let badges = if r.badges.is_empty() {
                    String::new()
                } else {
                    format!("  :{}:", r.badges.join(":"))
                };
                out.push(format!("{pad}{mark} {icon}{todo}{}{badges}", r.title));
            }
        }
        Node::Detail { fields } => {
            for f in fields {
                let kbd = f
                    .action
                    .as_ref()
                    .map_or_else(String::new, |a| format!("  [{}]", a.chord()));
                out.push(format!("{pad}{}: {}{kbd}", f.label, f.value));
            }
        }
        Node::Input { label, buffer } => out.push(format!("{pad}{label}> {buffer}")),
        Node::Palette { items, cursor } => {
            for (i, it) in items.iter().enumerate() {
                let mark = if i == *cursor { '>' } else { ' ' };
                out.push(format!("{pad}{mark} [{}] {}", it.action.chord(), it.label));
            }
        }
        Node::Hints { line } => out.push(format!("{pad}{line}")),
        Node::Widget { name, content } => {
            out.push(format!("{pad}«{name}»"));
            for l in content.lines() {
                out.push(format!("{pad}  {l}"));
            }
        }
        Node::Text(t) => out.push(format!("{pad}{t}")),
        Node::Split { direction, panes } => {
            out.push(format!("{pad}== split:{} ==", direction.as_str()));
            for p in panes {
                push_view_node(p, depth + 1, out);
            }
        }
        Node::Modal { title, body } => {
            out.push(format!("{pad}▌ modal: {title}"));
            push_view_node(body, depth + 1, out);
        }
        Node::Toast { level, text } => {
            out.push(format!("{pad}⚑ [{}] {text}", level.as_str()));
        }
    }
}

/// Map a typed theme role to a ratatui colour (G2).
///
/// The same declarative tokens the web shell renders as CSS variables,
/// here as terminal [`Color::Rgb`]. Hermetic — no terminal needed to
/// resolve a colour.
#[must_use]
pub fn theme_color(
    theme: &closure_shell_core::Theme,
    role: closure_shell_core::ColorRole,
) -> Color {
    let (r, g, b) = theme.color(role).rgb();
    Color::Rgb(r, g, b)
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

/// Derive the read-only panes the GUI grew from the same vault. A
/// feature that exists only in the window is a feature you cannot use
/// over ssh, which is most of what a terminal shell is for.
fn sync_panes(app: &mut App, vault: &Vault) {
    // The undo history belongs to the selected file, like undo itself.
    let history = app
        .selected_path()
        .and_then(|p| vault.document(p))
        .map(closure_core::Document::history_view)
        .map(|rows| {
            rows.into_iter()
                .map(|r| (r.label.clone(), r.is_current))
                .collect()
        })
        .unwrap_or_default();
    app.set_history(history);
    let mut counts: std::collections::HashMap<closure_core::BlockId, usize> =
        std::collections::HashMap::new();
    for targets in vault.link_graph().values() {
        for t in targets {
            *counts.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let mut hubs: Vec<(String, String, usize)> = counts
        .iter()
        .map(|(id, n)| {
            let title = vault
                .find_by_id(id)
                .map_or_else(|| "?".to_owned(), |(h, _)| h.title().to_owned());
            (id.to_string(), title, *n)
        })
        .collect();
    // Count descending, then id, so the list does not shuffle.
    hubs.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let orphans: Vec<(String, String)> = vault
        .iter()
        .flat_map(|(_, doc)| doc.all_headlines())
        .filter(|h| !counts.contains_key(h.id()))
        .map(|h| (h.id().to_string(), h.title().to_owned()))
        .collect();
    let mut dead: Vec<String> = Vec::new();
    for (path, doc) in vault.iter() {
        for h in doc.all_headlines() {
            for raw in h.link_targets() {
                if let Some(stripped) = raw.strip_prefix("id:")
                    && !vault.has_id(&closure_core::BlockId::from_existing(stripped))
                {
                    dead.push(format!("{raw}  ← {} in {}", h.title(), path.display()));
                }
            }
        }
    }
    app.set_graph(hubs, orphans, dead);
    app.set_journal(
        closure_record::Journal::new(vault.root(), true)
            .entries()
            .unwrap_or_default(),
    );
    app.set_cron(
        vault
            .iter()
            .filter_map(|(_, doc)| closure_cron::parse_jobs(&doc.source()).ok())
            .flatten()
            .map(|job| (format!("{:?}", job.spec), job.command))
            .collect(),
    );
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
                todo: h.todo().map(str::to_owned),
                priority: h.priority(),
                tags: h.tags().to_vec(),
                folded: h
                    .properties()
                    .iter()
                    .any(|(k, v)| k == "VISIBILITY" && v == "folded"),
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
    sync_panes(app, vault);
    let spec = closure_query::ViewSpec::parse(":from all :columns title,todo,priority")
        .unwrap_or_else(|_| closure_query::ViewSpec {
            from: closure_query::Source::All,
            columns: Vec::new(),
            sort: Vec::new(),
            filter: Vec::new(),
            name: None,
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
    if let Some((id, keyword)) = app.take_todo_request() {
        vault
            .set_todo(
                &closure_core::BlockId::from_existing(&id),
                keyword.as_deref(),
            )
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((id, priority)) = app.take_priority_request() {
        vault
            .set_priority(&closure_core::BlockId::from_existing(&id), priority)
            .map_err(vault_err)?;
        sync_app(app, vault);
    }
    if let Some((id, tags)) = app.take_tags_request() {
        vault
            .set_tags(&closure_core::BlockId::from_existing(&id), &tags)
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
    if let Some(index) = app.take_history_request()
        && let Some(path) = app.selected_path().map(Path::to_path_buf)
    {
        let r = vault.jump_history_in(&path, index);
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
        // In the body editor the highlighted row is the caret's line,
        // not a result cursor.
        let cursor = if app.mode() == AppMode::EditBody {
            app.body_cursor().0
        } else {
            app.result_cursor()
        };
        draw_overlay_list(f, area, title, rows, cursor);
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

/// Choose the highlighter for `lang` (V6b).
///
/// With the `tree-sitter` feature on, a real grammar (`TsHighlighter`) is
/// preferred when one is bundled for `lang`; otherwise (and always in the
/// hermetic default build) the dep-free `KeywordHighlighter` is used.
#[must_use]
pub fn pick_highlighter(lang: &str) -> Box<dyn Highlighter> {
    #[cfg(feature = "tree-sitter")]
    if let Some(ts) = closure_tree_sitter::TsHighlighter::for_language(lang) {
        return Box::new(ts);
    }
    Box::new(closure_tree_sitter::KeywordHighlighter::for_language(lang))
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
            let highlighter = pick_highlighter(&lang);

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
/// The body editor's lines with a caret glyph at the cursor column.
///
/// The overlay is a ratatui `List`, which has no cell-level cursor, so
/// the caret is drawn into the text — the same trick the whichkey pane
/// uses for its prefix.
fn body_editor_lines(app: &App) -> Vec<String> {
    let (cur_line, cur_col) = app.body_cursor();
    let mut out: Vec<String> = Vec::new();
    for (i, line) in app.buffer().split('\n').enumerate() {
        if i == cur_line {
            let mut with_caret: String = line.chars().take(cur_col).collect();
            with_caret.push('▏');
            with_caret.extend(line.chars().skip(cur_col));
            out.push(with_caret);
        } else {
            out.push(line.to_owned());
        }
    }
    out
}

/// The overlay for the single-line minibuffers: a prompt plus the live
/// query, and no rows underneath.
fn minibuffer_overlay(app: &App) -> Option<(String, Vec<String>)> {
    let prompt = match app.mode() {
        AppMode::Capture => "capture",
        AppMode::Rename => "rename",
        AppMode::AddHeadline => "add headline",
        AppMode::EditCell => "set property KEY=VALUE",
        AppMode::EditTags => "tags (space separated)",
        _ => return None,
    };
    Some((format!("{prompt}: {}", app.query()), Vec::new()))
}

/// The overlay for the four subsystem panes, each a driver-fed list
/// whose title carries the chords that act on the cursor row (V1).
fn subsystem_overlay(app: &App) -> Option<(String, Vec<String>)> {
    let title = match app.mode() {
        AppMode::Sniffer => "sniffer — g b block · g w allow · ESC back".to_owned(),
        AppMode::Sync => "sync peers — ESC back".to_owned(),
        AppMode::Llm => format!(
            "llm ({}) — i ask · g r toggle render · ESC back",
            if app.llm_render() { "org" } else { "raw" }
        ),
        AppMode::Conflicts => "conflicts — g o ours · g t theirs · ESC back".to_owned(),
        _ => return None,
    };
    Some((title, app.pane_rows()))
}

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
        AppMode::Capture
        | AppMode::Rename
        | AppMode::AddHeadline
        | AppMode::EditCell
        | AppMode::EditTags => minibuffer_overlay(app),
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
        // The graph pane labels each of its three sections and says
        // "(none)" under an empty one, so it is never blank.
        AppMode::Graph => Some(("link graph".to_owned(), app.graph_rows())),
        AppMode::Journal => Some(("recorded commands".to_owned(), app.journal_rows())),
        AppMode::Cron => Some(("scheduled jobs".to_owned(), app.cron_rows())),
        AppMode::Ex => Some((
            format!(":{}  — :w :q :wq :x, or any command name", app.query()),
            if app.status().is_empty() {
                Vec::new()
            } else {
                vec![app.status().to_owned()]
            },
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
        AppMode::EditBody => Some((app.body_status(), body_editor_lines(app))),
        AppMode::UndoHistory => Some((
            "undo history — j/k move · RET jump · ESC back".to_owned(),
            app.history_rows(),
        )),
        AppMode::Sniffer | AppMode::Sync | AppMode::Llm | AppMode::Conflicts => {
            subsystem_overlay(app)
        }
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
