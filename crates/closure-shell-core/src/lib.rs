//! Shell-agnostic launcher state core for closure's GUI shells.
//!
//! Pure, GPU-free state: the browse/filter list, detail/preview pane,
//! command palette, capture/rename/add edit surfaces, and input-mode
//! awareness. gpui, egui, and any future GUI consume this ONE core so
//! behaviour is identical and fully unit-testable without a window
//! (the vision's decoupled engine/shell). Kernel-agnostic (I7,
//! consumes Vault + closure-query); mutations route through the
//! [`Shell`] / vault commands (I8).

#![forbid(unsafe_code)]

use std::path::PathBuf;

use closure_config::InputMode;
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

    /// Replace a headline's body text through the kernel command (I8).
    /// This is the GUI's org-edit-special commit path.
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_body(
        &mut self,
        id: &closure_core::BlockId,
        body: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_body(id, body)
    }

    /// Set or overwrite a `:KEY: value` property through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_property(
        &mut self,
        id: &closure_core::BlockId,
        key: &str,
        value: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_property(id, key, value)
    }

    /// Set or clear the TODO keyword through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_todo(
        &mut self,
        id: &closure_core::BlockId,
        keyword: Option<&str>,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_todo(id, keyword)
    }

    /// Set or clear the priority cookie through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_priority(
        &mut self,
        id: &closure_core::BlockId,
        priority: Option<char>,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_priority(id, priority)
    }

    /// Replace the tag list through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_tags(
        &mut self,
        id: &closure_core::BlockId,
        tags: &[String],
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_tags(id, tags)
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
pub enum Mode {
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
    /// Editing the selected headline's body in a multiline buffer
    /// (org-edit-special). Commit via [`App::commit_edit_body`].
    EditBody,
    /// Editing a `(key, value)` property on the selected headline.
    /// Commit via [`App::commit_property`].
    PropertyEdit,
    /// Editing the selected headline's tag list (space-separated).
    /// Commit via [`App::commit_tags`].
    TagsEdit,
}

/// Commands offered by the slash palette as `(display, canonical)`:
/// the launcher's which-key surface. The key hint shown beside each is
/// derived from the active mode's keymap (the single source of truth,
/// I4) via the canonical command name — never hardcoded here.
const PALETTE_COMMANDS: &[(&str, &str)] = &[
    ("next-file", "next-file"),
    ("prev-file", "prev-file"),
    ("capture", "capture-start"),
    ("add-sibling", "add-sibling"),
    ("rename", "rename"),
    ("delete", "delete"),
    ("open", "open-file"),
    ("cycle-mode", "cycle-mode"),
    ("quit", "quit"),
];

/// Pure, GPU-free state core for the gpui shell.
///
/// All keyboard behaviour lives here so it is unit-testable without a
/// window (mirrors the TUI `App`). The gpui `Render` adapter (behind
/// the `gpui` feature) only translates key events into [`Self::on_key`]
/// and reads the accessors. Mutations route through [`Shell`], i.e.
/// kernel commands (I8); search reuses `closure_query` (I7).
#[derive(Debug)]
pub struct App {
    query: String,
    selected: usize,
    mode: Mode,
    capture_buf: String,
    rename_target: Option<String>,
    add_target: Option<String>,
    /// Block id whose body the `EditBody` surface is editing.
    edit_target: Option<String>,
    /// Multiline body buffer for the `EditBody` surface.
    body_buf: String,
    /// Block id whose property the `PropertyEdit` surface is editing.
    prop_target: Option<String>,
    /// Property key + value buffers for the `PropertyEdit` surface.
    prop_key: String,
    prop_value: String,
    /// Block id + space-separated buffer for the `TagsEdit` surface.
    tags_target: Option<String>,
    tags_buf: String,
    input_mode: closure_config::InputMode,
    palette_cursor: usize,
    status: String,
    quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Fresh app in Browse mode with an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            mode: Mode::Browse,
            capture_buf: String::new(),
            rename_target: None,
            add_target: None,
            edit_target: None,
            body_buf: String::new(),
            prop_target: None,
            prop_key: String::new(),
            prop_value: String::new(),
            tags_target: None,
            tags_buf: String::new(),
            input_mode: closure_config::InputMode::Notion,
            palette_cursor: 0,
            status: "browse — type to filter".to_owned(),
            quit: false,
        }
    }

    /// Palette rows `(command, key-hint)` matching the live palette
    /// filter (held in the capture buffer while in [`Mode::Palette`]),
    /// best fuzzy match first.
    #[must_use]
    pub fn palette_results(&self) -> Vec<(String, String)> {
        let q = &self.capture_buf;
        let mut scored: Vec<(u32, (String, String))> = PALETTE_COMMANDS
            .iter()
            .filter_map(|(name, canonical)| {
                let sc = if q.is_empty() {
                    Some(0)
                } else {
                    closure_query::fuzzy_score(q, name)
                };
                // Key hint from the active mode's keymap, not hardcoded.
                let key = self.chord_for(canonical).unwrap_or("—");
                sc.map(|s| (s, ((*name).to_owned(), key.to_owned())))
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

    /// Begin a capture (Notion "＋" affordance / `C-c`): switch to the
    /// capture surface with an empty title buffer.
    pub fn begin_capture(&mut self) {
        self.mode = Mode::Capture;
        self.capture_buf.clear();
        self.set_status("capture: type a title");
    }

    /// Begin adding a sibling after the selected row (Notion "＋" / `C-a`).
    /// No-op when there is no selection. Mouse + keyboard share this.
    pub fn begin_add_sibling(&mut self, shell: &Shell) {
        if let Some(row) = self.rows(shell).get(self.selected) {
            self.add_target = Some(row.id.clone());
            self.capture_buf.clear();
            self.mode = Mode::AddSibling;
            self.set_status("add sibling: type a title");
        }
    }

    /// Begin renaming the selected row (double-click / `C-r`), prefilling
    /// the buffer with its current title. No-op without a selection.
    pub fn begin_rename(&mut self, shell: &Shell) {
        if let Some(row) = self.rows(shell).get(self.selected) {
            self.rename_target = Some(row.id.clone());
            self.capture_buf.clear();
            self.capture_buf.push_str(&row.title);
            self.mode = Mode::Rename;
            self.set_status("rename: edit the title");
        }
    }

    /// Begin editing the selected headline's body (org-edit-special),
    /// prefilling the buffer with the current body. No-op without a
    /// selection. Commit with [`Self::commit_edit_body`] or cancel with
    /// Esc.
    pub fn begin_edit_body(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let body = self
            .detail(shell)
            .map(|d| d.body)
            .unwrap_or_default();
        self.edit_target = Some(row.id);
        self.body_buf = body;
        self.mode = Mode::EditBody;
        self.set_status("edit body — save to commit, Esc to cancel");
    }

    /// The body editor buffer (read).
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        &self.body_buf
    }

    /// Mutable body buffer, for the egui multiline `TextEdit` to bind to
    /// (the widget mutates the buffer in place; commit reads it back).
    pub const fn body_buffer_mut(&mut self) -> &mut String {
        &mut self.body_buf
    }

    /// Commit the body editor buffer to the target headline through the
    /// kernel command (I8), then return to Browse. No-op if not editing.
    pub fn commit_edit_body(&mut self, shell: &mut Shell) {
        if let Some(id) = self.edit_target.take() {
            let bid = closure_core::BlockId::from_existing(&id);
            // Org bodies are newline-terminated; without a trailing
            // newline a following sibling headline would be absorbed.
            let mut body = self.body_buf.clone();
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            match shell.set_body(&bid, &body) {
                Ok(()) => self.set_status("body saved"),
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
        self.body_buf.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel body editing without writing.
    fn cancel_edit_body(&mut self) {
        self.edit_target = None;
        self.body_buf.clear();
        self.mode = Mode::Browse;
        self.set_status("edit cancelled");
    }

    /// Begin adding a new property to the selected headline (empty
    /// key+value form). No-op without a selection.
    pub fn begin_add_property(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        self.prop_target = Some(row.id);
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::PropertyEdit;
        self.set_status("property — key + value, save to commit");
    }

    /// Begin editing an existing property `key` on the selected
    /// headline, prefilling its current value. No-op without a
    /// selection.
    pub fn begin_edit_property(&mut self, shell: &Shell, key: &str) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let value = self
            .detail(shell)
            .and_then(|d| {
                d.properties
                    .into_iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v)
            })
            .unwrap_or_default();
        self.prop_target = Some(row.id);
        self.prop_key.clear();
        self.prop_key.push_str(key);
        self.prop_value = value;
        self.mode = Mode::PropertyEdit;
        self.set_status("property — edit value, save to commit");
    }

    /// Property key buffer (read) + its mutable form for the egui field.
    #[must_use]
    pub fn prop_key(&self) -> &str {
        &self.prop_key
    }
    /// Mutable property-key buffer for the egui text field.
    pub const fn prop_key_mut(&mut self) -> &mut String {
        &mut self.prop_key
    }
    /// Property value buffer (read).
    #[must_use]
    pub fn prop_value(&self) -> &str {
        &self.prop_value
    }
    /// Mutable property-value buffer for the egui text field.
    pub const fn prop_value_mut(&mut self) -> &mut String {
        &mut self.prop_value
    }

    /// Commit the property (key,value) to the target headline through
    /// the kernel command (I8), then return to Browse. No-op if not
    /// editing or the key is empty.
    pub fn commit_property(&mut self, shell: &mut Shell) {
        if let Some(id) = self.prop_target.take()
            && !self.prop_key.trim().is_empty()
        {
            let bid = closure_core::BlockId::from_existing(&id);
            match shell.set_property(&bid, self.prop_key.trim(), &self.prop_value) {
                Ok(()) => self.set_status("property saved"),
                Err(e) => self.status = format!("property save failed: {e}"),
            }
        }
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel property editing without writing.
    fn cancel_property(&mut self) {
        self.prop_target = None;
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::Browse;
        self.set_status("property edit cancelled");
    }

    /// Cycle the selected headline's TODO keyword None -> TODO -> DONE
    /// -> None through the kernel command (I8). No-op without a
    /// selection.
    pub fn cycle_todo(&mut self, shell: &mut Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let next = match self.detail(shell).and_then(|d| d.todo) {
            None => Some("TODO"),
            Some(k) if k == "TODO" => Some("DONE"),
            Some(_) => None,
        };
        let bid = closure_core::BlockId::from_existing(&row.id);
        match shell.set_todo(&bid, next) {
            Ok(()) => self.set_status(next.map_or("todo cleared", |k| {
                if k == "TODO" { "todo: TODO" } else { "todo: DONE" }
            })),
            Err(e) => self.status = format!("todo failed: {e}"),
        }
    }

    /// Set (or clear) the selected headline's priority through the
    /// kernel command (I8). No-op without a selection.
    pub fn set_priority_cmd(&mut self, shell: &mut Shell, priority: Option<char>) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let bid = closure_core::BlockId::from_existing(&row.id);
        match shell.set_priority(&bid, priority) {
            Ok(()) => self.set_status("priority updated"),
            Err(e) => self.status = format!("priority failed: {e}"),
        }
    }

    /// Begin editing the selected headline's tags (space-separated
    /// buffer prefilled with the current tags). No-op without a
    /// selection.
    pub fn begin_edit_tags(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let tags = self.detail(shell).map(|d| d.tags).unwrap_or_default();
        self.tags_target = Some(row.id);
        self.tags_buf = tags.join(" ");
        self.mode = Mode::TagsEdit;
        self.set_status("tags — space-separated, save to commit");
    }

    /// Tags buffer (read) + its mutable form for the egui text field.
    #[must_use]
    pub fn tags_buffer(&self) -> &str {
        &self.tags_buf
    }
    /// Mutable tags buffer for the egui text field.
    pub const fn tags_buffer_mut(&mut self) -> &mut String {
        &mut self.tags_buf
    }

    /// Commit the tags buffer (split on whitespace) to the target
    /// headline through the kernel command (I8), then return to Browse.
    pub fn commit_tags(&mut self, shell: &mut Shell) {
        if let Some(id) = self.tags_target.take() {
            let tags: Vec<String> = self
                .tags_buf
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect();
            let bid = closure_core::BlockId::from_existing(&id);
            match shell.set_tags(&bid, &tags) {
                Ok(()) => self.set_status("tags saved"),
                Err(e) => self.status = format!("tags save failed: {e}"),
            }
        }
        self.tags_buf.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel tag editing without writing.
    fn cancel_tags(&mut self) {
        self.tags_target = None;
        self.tags_buf.clear();
        self.mode = Mode::Browse;
        self.set_status("tags edit cancelled");
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
    pub const fn mode(&self) -> Mode {
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
            // Browse hints come from the keymap source of truth (I4), so
            // every shown chord is the real binding for the active mode,
            // never a hardcoded string (vision: every UI element shows
            // its keybinding).
            Mode::Browse => {
                return format!("[{:?}] type: filter   {}", self.input_mode, self.command_hints());
            }
            Mode::Capture => "capture title — Enter: save   Esc: cancel",
            Mode::Rename => "rename — Enter: save   Esc: cancel",
            Mode::AddSibling => "add sibling — Enter: save   Esc: cancel",
            Mode::Palette => "command palette — type to filter   Enter: run   Esc: cancel",
            Mode::EditBody => "edit body — C-Enter: save   Enter: newline   Esc: cancel",
            Mode::PropertyEdit => "property — fill key + value   Save: commit   Esc: cancel",
            Mode::TagsEdit => "tags — space-separated   Save: commit   Esc: cancel",
        };
        format!("[{:?}] {body}", self.input_mode)
    }

    /// Which-key line for the active mode, built from
    /// [`closure_input::mode_keymap`] — the single keymap source of
    /// truth (I4). Shared shape with [`ModalApp::key_hints`].
    fn command_hints(&self) -> String {
        closure_input::mode_keymap(self.input_mode)
            .iter()
            .map(|(chord, cmd)| format!("{chord}:{cmd}"))
            .collect::<Vec<_>>()
            .join("  ")
    }

    /// The chord bound to `command` in the active mode, for labelling
    /// actionable widgets (the egui "＋" buttons) with their real key.
    #[must_use]
    pub fn chord_for(&self, command: &str) -> Option<&'static str> {
        closure_input::chord_for_command(self.input_mode, command)
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
            Mode::Capture => self.on_capture_key(shell, key, text),
            Mode::Rename => self.on_rename_key(shell, key, text),
            Mode::AddSibling => self.on_add_key(shell, key, text),
            Mode::Palette => self.on_palette_key(shell, key, text),
            Mode::EditBody => self.on_editbody_key(shell, key, ctrl, text),
            Mode::PropertyEdit => self.on_property_key(key),
            Mode::TagsEdit => self.on_tags_key(key),
            Mode::Browse => self.on_browse_key(shell, key, ctrl, text),
        }
    }

    /// Tags editor keys for keyboard fallback: Esc cancels. The text
    /// field owns typing; commit is the Save affordance / `commit_tags`.
    fn on_tags_key(&mut self, key: &str) {
        if key == "escape" {
            self.cancel_tags();
        }
    }

    /// Property editor keys for keyboard fallback: Esc cancels. Typing
    /// the key/value uses the egui text fields (`prop_key_mut` /
    /// `prop_value_mut`); commit is the Save affordance / `commit_property`.
    fn on_property_key(&mut self, key: &str) {
        if key == "escape" {
            self.cancel_property();
        }
    }

    /// Body editor keys: Esc cancels, `C-<enter>` commits, plain Enter
    /// inserts a newline, Backspace deletes, printable chars append.
    /// (egui also binds a multiline `TextEdit` to `body_buffer_mut` + a
    /// Save button; this path makes it keyboard-drivable + testable.)
    fn on_editbody_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        match key {
            "escape" => self.cancel_edit_body(),
            "enter" if ctrl => self.commit_edit_body(shell),
            "enter" => self.body_buf.push('\n'),
            "backspace" => {
                self.body_buf.pop();
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.body_buf.push(c);
                }
            }
        }
    }

    fn on_palette_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
            "capture" => self.begin_capture(),
            "add-sibling" => self.begin_add_sibling(shell),
            "rename" => self.begin_rename(shell),
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
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
                self.mode = Mode::Browse;
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
            "c" if ctrl => self.begin_capture(),
            // Notion-style slash command: `/` on an empty filter opens
            // the command palette; mid-query it's a literal filter char.
            "/" if !ctrl && self.query.is_empty() => {
                self.mode = Mode::Palette;
                self.capture_buf.clear();
                self.palette_cursor = 0;
                self.set_status("command palette — type to filter, Enter to run");
            }
            "t" if ctrl => self.cycle_mode(),
            "a" if ctrl => self.begin_add_sibling(shell),
            "r" if ctrl => self.begin_rename(shell),
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

/// Input surface for the modal command-surface experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalSurface {
    /// Keys are commands resolved against the active mode's keymap.
    Browse,
    /// A search overlay: typing filters, Enter picks, Esc cancels.
    Search,
    /// Typing the title of a new capture entry.
    Capture,
    /// Editing the selected headline's body (org-edit-special).
    EditBody,
}

/// Modal command-surface launcher (the "modal GUI" experiment).
///
/// Unlike [`App`] (a Notion-style type-to-filter launcher), `ModalApp`
/// treats Browse as a command surface: every key resolves against
/// [`closure_input::mode_keymap`] for the active [`InputMode`], so the
/// five editing modes (vim `j`/`k`, `g g`; emacs `C-x C-c`; …) drive a
/// GUI exactly as in the TUI. Typing happens only in the Search/Capture
/// overlays. Pure + headless-testable; mutations via [`Shell`] (I8).
#[derive(Debug)]
pub struct ModalApp {
    mode: InputMode,
    surface: ModalSurface,
    selected: usize,
    query: String,
    capture_buf: String,
    body_buf: String,
    edit_target: Option<String>,
    pending: Vec<String>,
    status: String,
    quit: bool,
}

impl ModalApp {
    /// New modal app in the given editing mode, Browse surface.
    #[must_use]
    pub const fn new(mode: InputMode) -> Self {
        Self {
            mode,
            surface: ModalSurface::Browse,
            selected: 0,
            query: String::new(),
            capture_buf: String::new(),
            body_buf: String::new(),
            edit_target: None,
            pending: Vec::new(),
            status: String::new(),
            quit: false,
        }
    }

    /// Active editing mode.
    #[must_use]
    pub const fn input_mode(&self) -> InputMode {
        self.mode
    }
    /// Active surface.
    #[must_use]
    pub const fn surface(&self) -> ModalSurface {
        self.surface
    }
    /// Highlighted row index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }
    /// Search filter (only meaningful on the Search surface).
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }
    /// In-progress capture title.
    #[must_use]
    pub fn capture_buffer(&self) -> &str {
        &self.capture_buf
    }
    /// One-line status.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// The active mode's full chord→command listing (which-key).
    #[must_use]
    pub fn key_hints(&self) -> String {
        closure_input::mode_keymap(self.mode)
            .iter()
            .map(|(c, cmd)| format!("{c}:{cmd}"))
            .collect::<Vec<_>>()
            .join("  ")
    }

    /// Rows: all headlines on Browse, fuzzy-filtered while searching.
    #[must_use]
    pub fn rows(&self, shell: &Shell) -> Vec<Row> {
        let filter = if self.surface == ModalSurface::Search {
            self.query.as_str()
        } else {
            ""
        };
        let mut scored: Vec<(u32, Row)> = Vec::new();
        for (p, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                let score = if filter.is_empty() {
                    Some(0)
                } else {
                    closure_query::fuzzy_score(filter, h.title())
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
        if !filter.is_empty() {
            scored.sort_by_key(|(sc, _)| std::cmp::Reverse(*sc));
        }
        scored.into_iter().map(|(_, r)| r).collect()
    }

    /// Move the selection to row `i`, clamped to the current result
    /// set. Used by mouse clicks on a row (draw parity with [`App`]).
    pub fn select(&mut self, i: usize, shell: &Shell) {
        let last = self.rows(shell).len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// The visible slice of rows for a viewport of `page` rows, plus its
    /// start offset, chosen so the selection stays on screen. Stateless
    /// (offset derived from the selection each call); mirrors
    /// [`App::view_window`].
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

    /// Full preview of the currently-selected headline (resolved by its
    /// stable id through the vault index), for the detail pane. Mirrors
    /// [`App::detail`].
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

    /// Feed one key. `key` is the gpui/egui-style name; `ctrl`/`alt`
    /// are modifiers; `text` is the printable char when any.
    pub fn on_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match self.surface {
            ModalSurface::Search => self.on_search_key(shell, key, text),
            ModalSurface::Capture => self.on_capture_key(shell, key, text),
            ModalSurface::EditBody => self.on_editbody_key(shell, key, ctrl, text),
            ModalSurface::Browse => self.on_browse_key(shell, key, ctrl, alt, text),
        }
    }

    /// Body editor keys (org-edit-special): Esc cancels, `C-<enter>`
    /// commits through the Vault (I8), Enter inserts a newline,
    /// Backspace deletes, printable chars append. Mirrors
    /// [`App::on_editbody_key`].
    fn on_editbody_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        match key {
            "escape" => {
                self.edit_target = None;
                self.body_buf.clear();
                self.surface = ModalSurface::Browse;
            }
            "enter" if ctrl => self.commit_edit_body(shell),
            "enter" => self.body_buf.push('\n'),
            "backspace" => {
                self.body_buf.pop();
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.body_buf.push(c);
                }
            }
        }
    }

    /// The body editor buffer (read).
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        &self.body_buf
    }

    /// Mutable body buffer for the egui multiline `TextEdit`.
    pub const fn body_buffer_mut(&mut self) -> &mut String {
        &mut self.body_buf
    }

    /// Commit the body buffer to the target headline through the kernel
    /// command (I8), then return to Browse. No-op if not editing.
    pub fn commit_edit_body(&mut self, shell: &mut Shell) {
        if let Some(id) = self.edit_target.take() {
            let bid = closure_core::BlockId::from_existing(&id);
            let mut body = self.body_buf.clone();
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            match shell.set_body(&bid, &body) {
                Ok(()) => "body saved".clone_into(&mut self.status),
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
        self.body_buf.clear();
        self.surface = ModalSurface::Browse;
    }

    fn on_search_key(&mut self, shell: &Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.surface = ModalSurface::Browse;
            }
            "enter" => {
                self.query.clear();
                self.surface = ModalSurface::Browse;
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            "down" => {
                let last = self.rows(shell).len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
            "up" => self.selected = self.selected.saturating_sub(1),
            _ => {
                if let Some(c) = text {
                    self.query.push(c);
                    self.selected = 0;
                }
            }
        }
    }

    fn on_capture_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.surface = ModalSurface::Browse;
                self.capture_buf.clear();
            }
            "enter" => {
                if !self.capture_buf.is_empty() {
                    match shell.capture(&self.capture_buf) {
                        Ok(()) => self.status = format!("captured: {}", self.capture_buf),
                        Err(e) => self.status = format!("capture failed: {e}"),
                    }
                }
                self.surface = ModalSurface::Browse;
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

    fn on_browse_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        let stroke = modal_stroke(key, ctrl, alt, text);
        let Some(stroke) = stroke else {
            self.pending.clear();
            return;
        };
        self.pending.push(stroke);
        let chord = self.pending.join(" ");
        let km = closure_input::mode_keymap(self.mode);
        if let Some((_, cmd)) = km.iter().find(|(c, _)| *c == chord) {
            self.pending.clear();
            let cmd = *cmd;
            self.run_command(shell, cmd);
        } else if km.iter().any(|(c, _)| c.starts_with(&format!("{chord} "))) {
            // Valid prefix — keep the pending strokes.
        } else {
            self.pending.clear();
        }
    }

    fn run_command(&mut self, shell: &Shell, cmd: &str) {
        let last = self.rows(shell).len().saturating_sub(1);
        match cmd {
            "next-file" => self.selected = (self.selected + 1).min(last),
            "prev-file" => self.selected = self.selected.saturating_sub(1),
            "first-file" => self.selected = 0,
            "last-file" => self.selected = last,
            "quit" => self.quit = true,
            "capture-start" => {
                self.surface = ModalSurface::Capture;
                self.capture_buf.clear();
            }
            "search-start" | "search-headline-start" => {
                self.surface = ModalSurface::Search;
                self.query.clear();
                self.selected = 0;
            }
            "open-file" => {
                if let Some(row) = self.rows(shell).get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "edit-body" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    self.edit_target = Some(row.id);
                    self.body_buf = self.detail(shell).map(|d| d.body).unwrap_or_default();
                    self.surface = ModalSurface::EditBody;
                    "edit body — C-Enter save, Esc cancel".clone_into(&mut self.status);
                }
            }
            "cycle-mode" => {
                self.mode = match self.mode {
                    InputMode::Notion => InputMode::Emacs,
                    InputMode::Emacs => InputMode::Vim,
                    InputMode::Vim => InputMode::Doom,
                    InputMode::Doom => InputMode::Helix,
                    InputMode::Helix => InputMode::Notion,
                };
            }
            other => self.status = format!("{other}: not available in the modal GUI experiment"),
        }
    }
}

/// Translate a GUI key event into a keymap chord stroke (`C-n`, `M-<`,
/// `<down>`, `RET`, bare `g`/`G`). Returns `None` for keys with no
/// stroke representation.
fn modal_stroke(key: &str, ctrl: bool, alt: bool, text: Option<char>) -> Option<String> {
    let base = match key {
        "enter" => "RET".to_owned(),
        "escape" => "ESC".to_owned(),
        "backspace" => "DEL".to_owned(),
        "tab" => "TAB".to_owned(),
        "space" => "SPC".to_owned(),
        "down" => "<down>".to_owned(),
        "up" => "<up>".to_owned(),
        _ => {
            if let Some(c) = text {
                c.to_string()
            } else if ctrl || alt {
                key.to_ascii_lowercase()
            } else {
                return None;
            }
        }
    };
    if ctrl {
        Some(format!("C-{base}"))
    } else if alt {
        Some(format!("M-{base}"))
    } else {
        Some(base)
    }
}
