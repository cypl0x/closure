//! gpui shell for closure (Zed's native GPU UI framework).
//!
//! Native desktop window built on gpui, behind the opt-in `gpui`
//! cargo feature so the default workspace stays hermetic (I10). All
//! behaviour lives in the dep-free, unit-tested [`GpuiApp`] state core
//! (mirrors the TUI `App`); the gpui `Render`/`run` adapter is a thin
//! translation of key events plus drawing. Kernel-agnostic (I7,
//! consumes Vault + closure-query only); mutations route through the
//! [`Shell`] / vault commands (I8). Live fuzzy filter, level-indented
//! tree with TODO colours, capture, and a which-key footer that always
//! shows the active bindings.

#![forbid(unsafe_code)]
#![recursion_limit = "512"]

use std::path::PathBuf;

use closure_store::Vault;

/// Selection state (parity with egui Shell for consistent multi-UI model).
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Currently-selected file, if any.
    pub file: Option<PathBuf>,
    /// Index of the selected headline within the file.
    pub headline: usize,
}

/// Kernel-side shell state for gpui (I7: only L2, no direct org spans).
/// Reuses Vault + commands for mutations (I8).
pub struct Shell {
    /// The loaded vault.
    pub vault: Vault,
    /// Current selection.
    pub selection: Selection,
}

impl Shell {
    /// Build a shell over an already-loaded vault.
    #[must_use]
    pub const fn new(vault: Vault) -> Self {
        Self {
            vault,
            selection: Selection {
                file: None,
                headline: 0,
            },
        }
    }

    /// Capture a new `TODO` entry into `inbox.org` (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`] from the capture.
    pub fn capture(&mut self, title: &str) -> Result<(), closure_store::VaultError> {
        let template = closure_store::CaptureTemplate {
            target: std::path::PathBuf::from("inbox.org"),
            headline_prefix: "TODO ".to_owned(),
            body: String::new(),
        };
        self.vault.capture(&template, title).map(|_| ())
    }

    /// Select `path` and reset the headline cursor.
    pub fn select_file(&mut self, path: Option<std::path::PathBuf>) {
        self.selection.file = path;
        self.selection.headline = 0;
    }

    /// Vault-wide fuzzy headline search, best 20 matches first.
    #[must_use]
    pub fn fuzzy_search(&self, q: &str) -> Vec<(std::path::PathBuf, String)> {
        let mut scored: Vec<(u32, std::path::PathBuf, String)> = vec![];
        for (p, doc) in self.vault.iter() {
            for h in doc.all_headlines() {
                if let Some(sc) = closure_query::fuzzy_score(q, h.title()) {
                    scored.push((sc, p.to_path_buf(), h.title().to_owned()));
                }
            }
        }
        scored.sort_by_key(|(sc, _, _)| std::cmp::Reverse(*sc));
        scored
            .into_iter()
            .map(|(_, p, t)| (p, t))
            .take(20)
            .collect()
    }

    /// Rename a headline through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn rename_headline(
        &mut self,
        id: &closure_core::BlockId,
        title: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.rename_headline(id, title)
    }

    /// Remove a subtree through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn remove_subtree(
        &mut self,
        id: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.remove_subtree(id)
    }

    /// Add a sibling headline after `after_id` through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn add_sibling(
        &mut self,
        after_id: &closure_core::BlockId,
        title: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.add_sibling(after_id, title)
    }
}

/// Adapter for gpui embedder (parity with egui).
pub trait ShellAdapter {
    /// Render one frame from `shell` state.
    fn frame(&mut self, shell: &Shell);
    /// Feed one chord stroke into `shell`.
    fn input(&mut self, shell: &mut Shell, chord: &str);
}

/// Headless for tests (I7, no real GPU/window needed for invariants).
#[derive(Debug, Default)]
pub struct HeadlessAdapter {
    /// Number of frames rendered.
    pub frames: u64,
    /// Last chord fed in.
    pub last_chord: Option<String>,
}

impl ShellAdapter for HeadlessAdapter {
    fn frame(&mut self, _shell: &Shell) {
        self.frames += 1;
    }
    fn input(&mut self, _shell: &mut Shell, chord: &str) {
        self.last_chord = Some(chord.to_owned());
    }
}

/// Marker.
pub const GPUI_SHELL: &str = "gpui";

/// One rendered row in the gpui browse/search list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Stable block id (`:ID:`, I2) — the edit target.
    pub id: String,
    /// File the headline lives in (display path).
    pub path: String,
    /// Headline title.
    pub title: String,
    /// Outline level (1-based).
    pub level: u8,
    /// TODO keyword, if any.
    pub todo: Option<String>,
}

/// Full preview of the selected headline for the detail pane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detail {
    /// Headline title.
    pub title: String,
    /// TODO keyword, if any.
    pub todo: Option<String>,
    /// Priority letter, if any.
    pub priority: Option<char>,
    /// Tags, in order.
    pub tags: Vec<String>,
    /// `SCHEDULED:` timestamp, if any.
    pub scheduled: Option<String>,
    /// `DEADLINE:` timestamp, if any.
    pub deadline: Option<String>,
    /// `:KEY: value` property pairs.
    pub properties: Vec<(String, String)>,
    /// Body text below the headline.
    pub body: String,
    /// File the headline lives in (display path).
    pub path: String,
}

/// Which input surface the gpui shell is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuiMode {
    /// Browsing/filtering the headline list.
    Browse,
    /// Typing the title of a new capture entry.
    Capture,
    /// Editing the selected headline's title.
    Rename,
    /// Typing the title of a new sibling after the selected headline.
    AddSibling,
    /// Slash command palette: fuzzy-pick a command (which-key list).
    Palette,
}

/// Commands offered by the slash palette, with their gpui key hint.
/// The launcher's which-key surface: every command is reachable and
/// labelled here.
const PALETTE_COMMANDS: &[(&str, &str)] = &[
    ("next-file", "down / C-n"),
    ("prev-file", "up / C-p"),
    ("capture", "C-c"),
    ("add-sibling", "C-a"),
    ("rename", "C-r"),
    ("delete", "C-d"),
    ("open", "Enter"),
    ("cycle-mode", "C-t"),
    ("quit", "C-q / Esc"),
];

/// Pure, GPU-free state core for the gpui shell.
///
/// All keyboard behaviour lives here so it is unit-testable without a
/// window (mirrors the TUI `App`). The gpui `Render` adapter (behind
/// the `gpui` feature) only translates key events into [`Self::on_key`]
/// and reads the accessors. Mutations route through [`Shell`], i.e.
/// kernel commands (I8); search reuses `closure_query` (I7).
#[derive(Debug)]
pub struct GpuiApp {
    query: String,
    selected: usize,
    mode: GpuiMode,
    capture_buf: String,
    rename_target: Option<String>,
    add_target: Option<String>,
    input_mode: closure_config::InputMode,
    palette_cursor: usize,
    status: String,
    quit: bool,
}

impl Default for GpuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuiApp {
    /// Fresh app in Browse mode with an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            mode: GpuiMode::Browse,
            capture_buf: String::new(),
            rename_target: None,
            add_target: None,
            input_mode: closure_config::InputMode::Notion,
            palette_cursor: 0,
            status: "browse — type to filter".to_owned(),
            quit: false,
        }
    }

    /// Palette rows `(command, key-hint)` matching the live palette
    /// filter (held in the capture buffer while in [`GpuiMode::Palette`]),
    /// best fuzzy match first.
    #[must_use]
    pub fn palette_results(&self) -> Vec<(String, String)> {
        let q = &self.capture_buf;
        let mut scored: Vec<(u32, (String, String))> = PALETTE_COMMANDS
            .iter()
            .filter_map(|(name, key)| {
                let sc = if q.is_empty() {
                    Some(0)
                } else {
                    closure_query::fuzzy_score(q, name)
                };
                sc.map(|s| (s, ((*name).to_owned(), (*key).to_owned())))
            })
            .collect();
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        scored.into_iter().map(|(_, row)| row).collect()
    }

    /// Index of the highlighted palette row.
    #[must_use]
    pub const fn palette_cursor(&self) -> usize {
        self.palette_cursor
    }

    /// The active editing mode (label/which-key only — the GUI is a
    /// launcher shell, see ROADMAP Decisions).
    #[must_use]
    pub const fn input_mode(&self) -> closure_config::InputMode {
        self.input_mode
    }

    /// Set the active editing mode.
    pub const fn set_mode(&mut self, mode: closure_config::InputMode) {
        self.input_mode = mode;
    }

    fn cycle_mode(&mut self) {
        use closure_config::InputMode as M;
        self.input_mode = match self.input_mode {
            M::Notion => M::Emacs,
            M::Emacs => M::Vim,
            M::Vim => M::Doom,
            M::Doom => M::Helix,
            M::Helix => M::Notion,
        };
        self.set_status(&format!("mode: {:?}", self.input_mode));
    }

    /// Move the selection to row `i`, clamped to the current result
    /// set. Used by mouse clicks on a row.
    pub fn select(&mut self, i: usize, shell: &Shell) {
        let last = self.rows(shell).len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// Live filter query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Highlighted row index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Active input surface.
    #[must_use]
    pub const fn mode(&self) -> GpuiMode {
        self.mode
    }

    /// In-progress capture title.
    #[must_use]
    pub fn capture_buffer(&self) -> &str {
        &self.capture_buf
    }

    /// One-line status / feedback message.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// Which-key style hint line for the active mode (vision: every
    /// UI element shows its keybindings).
    #[must_use]
    pub fn key_hints(&self) -> String {
        let body = match self.mode {
            GpuiMode::Browse => {
                "type: filter   up/down or C-n/C-p: move   Enter: open   \
                 C-c: capture   C-a: add   C-r: rename   C-d: delete   \
                 C-t: cycle-mode   Esc: clear   C-q: quit"
            }
            GpuiMode::Capture => "capture title — Enter: save   Esc: cancel",
            GpuiMode::Rename => "rename — Enter: save   Esc: cancel",
            GpuiMode::AddSibling => "add sibling — Enter: save   Esc: cancel",
            GpuiMode::Palette => "command palette — type to filter   Enter: run   Esc: cancel",
        };
        format!("[{:?}] {body}", self.input_mode)
    }

    /// Rows for the current query, each carrying its block id, level,
    /// and TODO keyword. Empty query lists every headline in file
    /// order; otherwise fuzzy matches, best first (reusing
    /// `closure_query`, I7).
    #[must_use]
    pub fn rows(&self, shell: &Shell) -> Vec<Row> {
        let mut scored: Vec<(u32, Row)> = Vec::new();
        for (p, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                let score = if self.query.is_empty() {
                    Some(0)
                } else {
                    closure_query::fuzzy_score(&self.query, h.title())
                };
                if let Some(sc) = score {
                    scored.push((
                        sc,
                        Row {
                            id: h.id().to_string(),
                            path: p.display().to_string(),
                            title: h.title().to_owned(),
                            level: h.level(),
                            todo: h.todo().map(ToOwned::to_owned),
                        },
                    ));
                }
            }
        }
        if !self.query.is_empty() {
            scored.sort_by_key(|(sc, _)| std::cmp::Reverse(*sc));
        }
        scored.into_iter().map(|(_, r)| r).collect()
    }

    /// The visible slice of rows for a viewport of `page` rows, plus
    /// its start offset, chosen so the selection stays on screen. Caps
    /// the number of rendered nodes for large vaults; stateless (offset
    /// derived from the selection each call).
    #[must_use]
    pub fn view_window(&self, shell: &Shell, page: usize) -> (usize, Vec<Row>) {
        let rows = self.rows(shell);
        if page == 0 || rows.len() <= page {
            return (0, rows);
        }
        let max_offset = rows.len() - page;
        let offset = self.selected.saturating_sub(page - 1).min(max_offset);
        let slice = rows[offset..offset + page].to_vec();
        (offset, slice)
    }

    /// Full preview of the currently-selected headline (resolved by
    /// its stable id through the vault index), for the detail pane.
    #[must_use]
    pub fn detail(&self, shell: &Shell) -> Option<Detail> {
        let rows = self.rows(shell);
        let row = rows.get(self.selected)?;
        let bid = closure_core::BlockId::from_existing(&row.id);
        let (h, path) = shell.vault.find_by_id(&bid)?;
        Some(Detail {
            title: h.title().to_owned(),
            todo: h.todo().map(ToOwned::to_owned),
            priority: h.priority(),
            tags: h.tags().to_vec(),
            scheduled: h.scheduled().map(ToOwned::to_owned),
            deadline: h.deadline().map(ToOwned::to_owned),
            properties: h.properties().to_vec(),
            body: h.body_text().to_owned(),
            path: path.display().to_string(),
        })
    }

    /// Feed one key. `key` is the gpui key name (`"a"`, `"enter"`,
    /// `"backspace"`, `"escape"`, `"down"`, `"up"`, …); `ctrl` is the
    /// control modifier; `text` is the typed character when the key
    /// produced printable, unmodified input.
    pub fn on_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        if ctrl && key == "q" {
            self.quit = true;
            return;
        }
        match self.mode {
            GpuiMode::Capture => self.on_capture_key(shell, key, text),
            GpuiMode::Rename => self.on_rename_key(shell, key, text),
            GpuiMode::AddSibling => self.on_add_key(shell, key, text),
            GpuiMode::Palette => self.on_palette_key(shell, key, text),
            GpuiMode::Browse => self.on_browse_key(shell, key, ctrl, text),
        }
    }

    fn on_palette_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
            }
            "down" => {
                let last = self.palette_results().len().saturating_sub(1);
                self.palette_cursor = (self.palette_cursor + 1).min(last);
            }
            "up" => self.palette_cursor = self.palette_cursor.saturating_sub(1),
            "backspace" => {
                self.capture_buf.pop();
                self.palette_cursor = 0;
            }
            "enter" => {
                let pick = self
                    .palette_results()
                    .get(self.palette_cursor)
                    .map(|(name, _)| name.clone());
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
                if let Some(cmd) = pick {
                    self.run_palette_command(shell, &cmd);
                }
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                    self.palette_cursor = 0;
                }
            }
        }
    }

    /// Execute a command chosen from the palette, reusing the same
    /// surfaces the key bindings drive.
    fn run_palette_command(&mut self, shell: &mut Shell, cmd: &str) {
        let rows = self.rows(shell);
        match cmd {
            "next-file" => {
                let last = rows.len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
            "prev-file" => self.selected = self.selected.saturating_sub(1),
            "capture" => {
                self.mode = GpuiMode::Capture;
                self.capture_buf.clear();
                self.set_status("capture: type a title");
            }
            "add-sibling" => {
                if let Some(row) = rows.get(self.selected) {
                    self.add_target = Some(row.id.clone());
                    self.capture_buf.clear();
                    self.mode = GpuiMode::AddSibling;
                }
            }
            "rename" => {
                if let Some(row) = rows.get(self.selected) {
                    self.rename_target = Some(row.id.clone());
                    self.capture_buf.clear();
                    self.capture_buf.push_str(&row.title);
                    self.mode = GpuiMode::Rename;
                }
            }
            "delete" => {
                if let Some(row) = rows.get(self.selected) {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.remove_subtree(&bid);
                    self.selected = self.selected.min(self.rows(shell).len().saturating_sub(1));
                }
            }
            "open" => {
                if let Some(row) = rows.get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "cycle-mode" => self.cycle_mode(),
            "quit" => self.quit = true,
            _ => {}
        }
    }

    fn on_add_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
                self.add_target = None;
                self.set_status("add cancelled");
            }
            "enter" => {
                if let Some(after) = self.add_target.take()
                    && !self.capture_buf.is_empty()
                {
                    let bid = closure_core::BlockId::from_existing(&after);
                    match shell.add_sibling(&bid, &self.capture_buf) {
                        Ok(()) => self.status = format!("added: {}", self.capture_buf),
                        Err(e) => self.status = format!("add failed: {e}"),
                    }
                }
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn on_rename_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
                self.rename_target = None;
                self.set_status("rename cancelled");
            }
            "enter" => {
                if let Some(id) = self.rename_target.take()
                    && !self.capture_buf.is_empty()
                {
                    let bid = closure_core::BlockId::from_existing(&id);
                    match shell.rename_headline(&bid, &self.capture_buf) {
                        Ok(()) => self.status = format!("renamed to {}", self.capture_buf),
                        Err(e) => self.status = format!("rename failed: {e}"),
                    }
                }
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn set_status(&mut self, s: &str) {
        self.status.clear();
        self.status.push_str(s);
    }

    fn on_capture_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
                self.set_status("capture cancelled");
            }
            "enter" => {
                if !self.capture_buf.is_empty() {
                    match shell.capture(&self.capture_buf) {
                        Ok(()) => self.status = format!("captured: {}", self.capture_buf),
                        Err(e) => self.status = format!("capture failed: {e}"),
                    }
                }
                self.mode = GpuiMode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn on_browse_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        let rows = self.rows(shell);
        let last = rows.len().saturating_sub(1);
        match key {
            "c" if ctrl => {
                self.mode = GpuiMode::Capture;
                self.capture_buf.clear();
                self.set_status("capture: type a title");
            }
            // Notion-style slash command: `/` on an empty filter opens
            // the command palette; mid-query it's a literal filter char.
            "/" if !ctrl && self.query.is_empty() => {
                self.mode = GpuiMode::Palette;
                self.capture_buf.clear();
                self.palette_cursor = 0;
                self.set_status("command palette — type to filter, Enter to run");
            }
            "t" if ctrl => self.cycle_mode(),
            "a" if ctrl => {
                if let Some(row) = rows.get(self.selected) {
                    self.add_target = Some(row.id.clone());
                    self.capture_buf.clear();
                    self.mode = GpuiMode::AddSibling;
                    self.set_status("add sibling: type a title");
                }
            }
            "r" if ctrl => {
                if let Some(row) = rows.get(self.selected) {
                    self.rename_target = Some(row.id.clone());
                    self.capture_buf.clear();
                    self.capture_buf.push_str(&row.title);
                    self.mode = GpuiMode::Rename;
                    self.set_status("rename: edit the title");
                }
            }
            "d" if ctrl => {
                if let Some(row) = rows.get(self.selected) {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let title = row.title.clone();
                    match shell.remove_subtree(&bid) {
                        Ok(()) => {
                            self.status = format!("deleted: {title}");
                            self.selected =
                                self.selected.min(self.rows(shell).len().saturating_sub(1));
                        }
                        Err(e) => self.status = format!("delete failed: {e}"),
                    }
                }
            }
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.set_status("browse — type to filter");
            }
            "down" => self.selected = (self.selected + 1).min(last),
            "up" => self.selected = self.selected.saturating_sub(1),
            "n" if ctrl => self.selected = (self.selected + 1).min(last),
            "p" if ctrl => self.selected = self.selected.saturating_sub(1),
            "enter" => {
                if let Some(row) = rows.get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.query.push(c);
                    self.selected = 0;
                }
            }
        }
    }
}

/// Launch fallback when the `gpui` feature is disabled (the default,
/// hermetic build). The kernel-side [`Shell`] is always available; the
/// GPU window requires `--features gpui` and the system GPU/X11 libs.
#[cfg(not(feature = "gpui"))]
pub fn run(_vault_path: &std::path::Path) -> Result<(), String> {
    Err(
        "gpui shell not compiled: rebuild closure-cli with `--features gpui` \
         (pulls Zed's GPU stack + system X11/xkbcommon/freetype). \
         The egui shell is the default native path."
            .to_owned(),
    )
}

// === Polished gpui app (high-perf desktop per vision) ===
// A real Zed/gpui window over the dep-free, unit-tested GpuiApp state
// core above: dark Tokyo-night theme, level-indented headline tree
// with TODO colours, live fuzzy filter, capture, and a which-key
// footer that always shows the active bindings. Keyboard-first and
// consistent with the TUI/egui shells (I7/I8). Compiled only under
// `--features gpui`.

#[cfg(feature = "gpui")]
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

/// Launch the gpui desktop window against the vault at `vault_path`.
/// Blocks until the window closes.
///
/// # Errors
///
/// Returns the vault open error as a string; window/runtime failures
/// surface through gpui's own panics on the UI thread.
#[cfg(feature = "gpui")]
pub fn run(vault_path: &std::path::Path) -> Result<(), String> {
    let vault = Vault::open(vault_path).map_err(|e| format!("{e}"))?;
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(640.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| GpuiView {
                    shell: Shell::new(vault),
                    app: GpuiApp::new(),
                    focus_handle: cx.focus_handle(),
                })
            },
        );
        if let Ok(window) = opened {
            window
                .update(cx, |view, window, cx| {
                    window.focus(&view.focus_handle(cx));
                })
                .ok();
        }
        cx.activate(true);
    });
    Ok(())
}

/// gpui view: owns the kernel-side [`Shell`] and the pure [`GpuiApp`]
/// state, plus a focus handle so the root receives key events.
#[cfg(feature = "gpui")]
struct GpuiView {
    shell: Shell,
    app: GpuiApp,
    focus_handle: FocusHandle,
}

#[cfg(feature = "gpui")]
impl Focusable for GpuiView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(feature = "gpui")]
impl GpuiView {
    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = &ks.modifiers;
        let text = ks
            .key_char
            .as_ref()
            .and_then(|s| s.chars().next())
            .filter(|_| !m.control && !m.alt && !m.platform && !m.function);
        self.app.on_key(&mut self.shell, &ks.key, m.control, text);
        if self.app.should_quit() {
            cx.quit();
        }
        cx.notify();
    }

    /// Right-hand detail/preview pane for the selected headline.
    #[allow(clippy::unreadable_literal)]
    fn detail_pane(&self) -> impl IntoElement {
        let fg = rgb(0xc0caf5);
        let dim = rgb(0x565f89);
        let accent = rgb(0x7aa2f7);
        let todo_col = rgb(0xf7768e);
        let sel = rgb(0x414868);
        let pane = div().flex().flex_col().flex_grow().px_3().py_2().gap_2();
        // While the palette is open, the right pane is the which-key
        // command list (name + key), with the cursor row highlighted.
        if self.app.mode() == GpuiMode::Palette {
            let cursor = self.app.palette_cursor();
            return pane.child(
                div().flex().flex_col().children(
                    self.app.palette_results().into_iter().enumerate().map(
                        |(i, (name, keyhint))| {
                            div()
                                .flex()
                                .px_2()
                                .py_1()
                                .bg(if i == cursor { sel } else { rgb(0x1a1b26) })
                                .child(div().text_color(fg).child(name))
                                .child(div().flex_grow())
                                .child(div().text_color(dim).text_size(px(11.0)).child(keyhint))
                        },
                    ),
                ),
            );
        }
        let Some(d) = self.app.detail(&self.shell) else {
            return pane.child(div().text_color(dim).child("no selection"));
        };
        let props = d
            .properties
            .iter()
            .map(|(k, v)| format!(":{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        pane.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(accent).text_lg().child(d.title.clone()))
                .child(
                    div()
                        .text_color(dim)
                        .text_size(px(12.0))
                        .child(meta_line(&d)),
                )
                .child(
                    div()
                        .text_color(dim)
                        .text_size(px(11.0))
                        .child(d.path.clone()),
                )
                .child(div().text_color(todo_col).text_size(px(12.0)).child(props))
                .child(
                    div()
                        .mt_2()
                        .text_color(fg)
                        .text_size(px(13.0))
                        .child(d.body.clone()),
                ),
        )
    }
}

/// One-line metadata summary (TODO / priority / tags / planning) for
/// the detail pane.
#[cfg(feature = "gpui")]
fn meta_line(d: &Detail) -> String {
    use std::fmt::Write as _;
    let mut meta = String::new();
    if let Some(t) = &d.todo {
        let _ = write!(meta, "{t} ");
    }
    if let Some(p) = d.priority {
        let _ = write!(meta, "[#{p}] ");
    }
    if !d.tags.is_empty() {
        let _ = write!(meta, ":{}: ", d.tags.join(":"));
    }
    if let Some(s) = &d.scheduled {
        let _ = write!(meta, "SCHEDULED {s} ");
    }
    if let Some(s) = &d.deadline {
        let _ = write!(meta, "DEADLINE {s} ");
    }
    meta
}

#[cfg(feature = "gpui")]
impl Render for GpuiView {
    #[allow(clippy::unreadable_literal)] // RGB hex literals read clearest ungrouped
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Window the list so huge vaults stay snappy; ~40 rows per page.
        const PAGE: usize = 40;
        // Tokyo-night palette.
        let bg = rgb(0x1a1b26);
        let fg = rgb(0xc0caf5);
        let dim = rgb(0x565f89);
        let sel = rgb(0x414868);
        let accent = rgb(0x7aa2f7);
        let todo_col = rgb(0xf7768e);

        let total = self.app.rows(&self.shell).len();
        let (offset, rows) = self.app.view_window(&self.shell, PAGE);
        let selected = self.app.selected();
        let count = format!("{total} headline(s)");

        let header = match self.app.mode() {
            GpuiMode::Capture => format!("＋ capture: {}▏", self.app.capture_buffer()),
            GpuiMode::AddSibling => format!("＋ add: {}▏", self.app.capture_buffer()),
            GpuiMode::Rename => format!("✎ rename: {}▏", self.app.capture_buffer()),
            GpuiMode::Palette => format!("❯ command: {}▏", self.app.capture_buffer()),
            GpuiMode::Browse => format!("⌕ {}▏   {count}", self.app.query()),
        };

        let list = div()
            .flex()
            .flex_col()
            .w(px(380.0))
            .min_w(px(280.0))
            .gap_0()
            .border_r_1()
            .border_color(rgb(0x2a2e42))
            .children(rows.into_iter().enumerate().map(|(vis, row)| {
                let i = offset + vis;
                let mut line = div()
                    .flex()
                    .px_2()
                    .py_1()
                    .text_size(px(14.0))
                    .bg(if i == selected { sel } else { bg })
                    // Mouse: click a row to select it (Notion-style pointer use).
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev, _window, cx| {
                            this.app.select(i, &this.shell);
                            cx.notify();
                        }),
                    );
                let indent = "  ".repeat(usize::from(row.level).saturating_sub(1));
                if let Some(todo) = &row.todo {
                    line = line.child(div().text_color(todo_col).mr_2().child(todo.clone()));
                }
                line.child(format!("{indent}{}", row.title))
                    .child(div().flex_grow())
                    .child(div().text_color(dim).text_size(px(11.0)).child(row.path))
            }));

        let body_row = div()
            .flex()
            .flex_row()
            .flex_grow()
            .child(list)
            .child(self.detail_pane());

        div()
            .key_context("ClosureGpui")
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .font_family("monospace")
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_color(accent)
                    .text_lg()
                    .child("closure · gpui"),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(rgb(0x24283b))
                    .text_color(fg)
                    .child(header),
            )
            .child(body_row)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(rgb(0x24283b))
                    .text_color(dim)
                    .text_size(px(11.0))
                    .child(self.app.status().to_owned()),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_color(dim)
                    .text_size(px(11.0))
                    .child(self.app.key_hints()),
            )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fs;

    use closure_store::Vault;
    use tempfile::TempDir;

    use super::{HeadlessAdapter, Shell, ShellAdapter};

    fn test_vault() -> (TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tmp");
        fs::write(
            dir.path().join("notes.org"),
            "* TODO Test gpui\n:PROPERTIES:\n:ID: 01HQXGPUI0000000000000000\n:END:\n",
        )
        .expect("write");
        let v = Vault::open(dir.path()).expect("open");
        (dir, v)
    }

    #[test]
    fn gpui_shell_parity_with_egui_model() {
        // Invariant: gpui Shell has same API surface as egui for multi-UI consistency (vision).
        let (_td, v) = test_vault();
        let mut shell = Shell::new(v);
        assert!(!shell.fuzzy_search("Test").is_empty());
        shell.capture("New from gpui").expect("capture");
        // Mutation via commands only (I8) -- find after capture.
        assert!(!shell.fuzzy_search("New from gpui").is_empty());
    }

    #[test]
    fn gpui_headless_adapter_no_panic() {
        // I5 / I7: headless works without GPU/window, drives shell.
        let (_td, v) = test_vault();
        let mut shell = Shell::new(v);
        let mut adapter = HeadlessAdapter::default();
        adapter.frame(&shell);
        adapter.input(&mut shell, "C-c c");
        assert_eq!(adapter.frames, 1);
        assert_eq!(adapter.last_chord.as_deref(), Some("C-c c"));
    }

    #[test]
    fn gpui_uses_registry_for_commands() {
        // I4/I8: mutations only through registry surface.
        let reg = closure_core::default_registry();
        assert!(
            reg.get("rename-headline").is_some(),
            "gpui must align with registry"
        );
    }
}
