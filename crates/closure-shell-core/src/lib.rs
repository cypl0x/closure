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
pub use closure_tree_sitter::HighlightKind;

/// Fold the keyword highlighter's byte ranges into owned segments.
///
/// Produces `(text, kind)` segments a GUI can colour without
/// re-scanning, via the dependency-free
/// [`closure_tree_sitter::KeywordHighlighter`] (hermetic — no
/// tree-sitter C grammar). Empty source yields no spans; otherwise the
/// segments concatenate back to `source` exactly.
#[must_use]
pub fn highlight_spans(source: &str, lang: &str) -> Vec<(String, HighlightKind)> {
    use closure_tree_sitter::{Highlighter as _, KeywordHighlighter};
    if source.is_empty() {
        return Vec::new();
    }
    KeywordHighlighter::for_language(lang)
        .highlight(source)
        .into_iter()
        .filter(|h| h.start < h.end)
        .map(|h| (source[h.start..h.end].to_owned(), h.kind))
        .collect()
}

/// One segment of a headline body: free-form prose or a fenced code
/// block (carrying its normalised language). Lets a GUI render code
/// blocks distinct from prose without re-parsing the fences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySegment {
    /// Prose lines (everything outside a `#+BEGIN_SRC … #+END_SRC`).
    Prose(String),
    /// A code block: `lang` is the normalised fence language (`"plain"`
    /// when none), `text` the block's inner content.
    Code {
        /// Normalised fence language (matches [`highlight_spans`]).
        lang: String,
        /// Inner content of the block (between the fences).
        text: String,
    },
}

/// Normalise a fence language token to the names [`highlight_spans`]
/// understands (`sh`/`bash`/`zsh` → `shell`, `py` → `python`, `rs` →
/// `rust`, empty → `plain`).
fn normalise_lang(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "" => "plain".to_owned(),
        "sh" | "bash" | "zsh" => "shell".to_owned(),
        "py" => "python".to_owned(),
        "rs" => "rust".to_owned(),
        other => other.to_owned(),
    }
}

/// Split a headline body into prose / code-block [`BodySegment`]s.
///
/// Consecutive prose lines coalesce into one [`BodySegment::Prose`];
/// each `#+BEGIN_SRC [lang] … #+END_SRC` becomes a
/// [`BodySegment::Code`] (fence match is case-insensitive). An
/// unterminated block captures to the end. Empty input yields no
/// segments.
#[must_use]
pub fn segment_body(body: &str) -> Vec<BodySegment> {
    let mut out: Vec<BodySegment> = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut lines = body.lines();

    let flush_prose = |prose: &mut Vec<&str>, out: &mut Vec<BodySegment>| {
        if !prose.is_empty() {
            out.push(BodySegment::Prose(prose.join("\n")));
            prose.clear();
        }
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.to_ascii_uppercase().starts_with("#+BEGIN_SRC") {
            flush_prose(&mut prose, &mut out);
            let lang = normalise_lang(trimmed.split_whitespace().nth(1).unwrap_or(""));
            let mut code: Vec<&str> = Vec::new();
            for cl in lines.by_ref() {
                if cl
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("#+END_SRC")
                {
                    break;
                }
                code.push(cl);
            }
            out.push(BodySegment::Code {
                lang,
                text: code.join("\n"),
            });
        } else {
            prose.push(line);
        }
    }
    flush_prose(&mut prose, &mut out);
    out
}

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

/// A keybinding-bearing action attached to an actionable view node.
///
/// Constructing one *requires* a chord, so an actionable node can never
/// lack its keybinding — the vision's "every UI element shows its
/// keybinding" rule made type-level (V1). The only constructor,
/// [`Action::new`], returns `None` when the active mode binds no chord to
/// the command, so a command-without-chord cannot be represented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    command: String,
    chord: String,
}

impl Action {
    /// An action for `command` in `mode`. `None` when no chord is bound
    /// (the source of truth is [`closure_input::chord_for_command`], I4).
    #[must_use]
    pub fn new(mode: closure_config::InputMode, command: impl Into<String>) -> Option<Self> {
        let command = command.into();
        closure_input::chord_for_command(mode, &command).map(|chord| Self {
            command,
            chord: chord.to_owned(),
        })
    }

    /// The canonical command name.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The chord bound to it (non-empty by construction).
    #[must_use]
    pub fn chord(&self) -> &str {
        &self.chord
    }
}

/// A headline row in a [`Node::Rows`] list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// Stable block id (I2).
    pub id: String,
    /// Headline title.
    pub title: String,
    /// Outline level (1-based).
    pub level: u8,
    /// TODO keyword, if any.
    pub todo: Option<String>,
}

/// A labelled field in a [`Node::Detail`] pane; `action` present iff the
/// field is actionable (and then it carries its chord, V1 invariant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    /// Field label (`title`, `todo`, `tags`, a property key, …).
    pub label: String,
    /// Field value, rendered as text.
    pub value: String,
    /// The action triggered by activating the field, if any.
    pub action: Option<Action>,
}

/// A command row in a [`Node::Palette`]; always actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItemView {
    /// Display label.
    pub label: String,
    /// The command + its chord.
    pub action: Action,
}

/// A node of the declarative view tree (V1).
///
/// A pure description of a screen that any embedder renders: the engine
/// emits the tree, the shell draws it (the Flutter engine/embedder
/// split). Deterministic (I6) — `view` is a pure function of state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A labelled region containing child nodes.
    Pane {
        /// Region label.
        title: String,
        /// Child nodes, in render order.
        children: Vec<Self>,
    },
    /// The headline list with the selected index.
    Rows {
        /// Visible rows.
        rows: Vec<RowView>,
        /// Index of the selected row.
        selected: usize,
    },
    /// The detail pane: a list of (maybe-actionable) fields.
    Detail {
        /// Fields, in display order.
        fields: Vec<FieldView>,
    },
    /// A text-entry surface (capture / rename / body / tags / property).
    Input {
        /// What is being edited.
        label: String,
        /// Current buffer contents.
        buffer: String,
    },
    /// The command palette (which-key list).
    Palette {
        /// Offered commands.
        items: Vec<PaletteItemView>,
        /// Index of the highlighted item.
        cursor: usize,
    },
    /// The always-on which-key hint line.
    Hints {
        /// Rendered hint line.
        line: String,
    },
    /// An expanded composite widget (V2c): its name + the already-expanded
    /// content (a `closure-widget` block resolved via `closure-query`).
    Widget {
        /// Widget name.
        name: String,
        /// Expanded content.
        content: String,
    },
    /// Inert text.
    Text(String),
}

/// Build a [`Node::Widget`] from a name and its expanded content (V2c).
#[must_use]
pub fn widget_node(name: impl Into<String>, content: impl Into<String>) -> Node {
    Node::Widget {
        name: name.into(),
        content: content.into(),
    }
}

/// Build the default browse [`ViewTree`](Node) from a borrowed vault
/// (V3a).
///
/// Every headline becomes a row (vault iteration order), selection 0,
/// plus a hint line. Borrow-friendly (no [`Shell`] ownership) so callers
/// like the LLM `view-render` tool can snapshot the screen.
#[must_use]
pub fn browse_view(vault: &closure_store::Vault) -> Node {
    let rows: Vec<RowView> = vault
        .iter()
        .flat_map(|(_p, doc)| {
            doc.all_headlines().map(|h| RowView {
                id: h.id().to_string(),
                title: h.title().to_owned(),
                level: h.level(),
                todo: h.todo().map(ToOwned::to_owned),
            })
        })
        .collect();
    let line = format!("[Notion] {} headlines — type: filter", rows.len());
    Node::Pane {
        title: "closure".to_owned(),
        children: vec![Node::Rows { rows, selected: 0 }, Node::Hints { line }],
    }
}

/// Serialise a [`ViewTree`](Node) to a compact, LLM-readable snapshot
/// (V3a).
///
/// Captures what is on screen — panes, the selected row, visible rows,
/// detail fields, the focused input, palette, and widgets. Deterministic.
#[must_use]
pub fn serialize_view(node: &Node) -> String {
    let mut out = String::new();
    serialize_node(node, 0, &mut out);
    out
}

fn serialize_node(node: &Node, depth: usize, out: &mut String) {
    use std::fmt::Write as _;
    let pad = "  ".repeat(depth);
    match node {
        Node::Pane { title, children } => {
            let _ = writeln!(out, "{pad}PANE {title}");
            for c in children {
                serialize_node(c, depth + 1, out);
            }
        }
        Node::Rows { rows, selected } => {
            let _ = writeln!(out, "{pad}ROWS selected={selected} count={}", rows.len());
            for (i, r) in rows.iter().enumerate() {
                let mark = if i == *selected { '>' } else { ' ' };
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(String::new, |t| format!("{t} "));
                let _ = writeln!(out, "{pad}  {mark} {todo}{}", r.title);
            }
        }
        Node::Detail { fields } => {
            let _ = writeln!(out, "{pad}DETAIL");
            for f in fields {
                let act = f
                    .action
                    .as_ref()
                    .map_or_else(String::new, |a| format!(" [{}]", a.chord()));
                let _ = writeln!(out, "{pad}  {}: {}{act}", f.label, f.value);
            }
        }
        Node::Input { label, buffer } => {
            let _ = writeln!(out, "{pad}INPUT {label}=\"{buffer}\"");
        }
        Node::Palette { items, cursor } => {
            let _ = writeln!(out, "{pad}PALETTE cursor={cursor} count={}", items.len());
            for it in items {
                let _ = writeln!(out, "{pad}  [{}] {}", it.action.chord(), it.label);
            }
        }
        Node::Hints { line } => {
            let _ = writeln!(out, "{pad}HINTS {line}");
        }
        Node::Widget { name, content } => {
            let _ = writeln!(out, "{pad}WIDGET {name}");
            for l in content.lines() {
                let _ = writeln!(out, "{pad}  {l}");
            }
        }
        Node::Text(t) => {
            let _ = writeln!(out, "{pad}TEXT {t}");
        }
    }
}

/// One captured network flow + the action decided for it (V7a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffEvent {
    /// Candidate string (`"<host>:<port> <proto>"`).
    pub candidate: String,
    /// The action a rule decided, if any.
    pub action: Option<closure_sniffer::Action>,
}

/// Headless state for the interactive sniffer surface (V7a).
///
/// A pure state machine over [`closure_sniffer::CaptureBackend`]: a live
/// event list, a cursor, a substring filter, and per-flow allow/block
/// toggles that mutate the blocklist rules. Unit-testable without a
/// terminal — the same pattern as the launcher [`App`]; a shell (V7b)
/// renders it as a [`ViewTree`](Node).
#[derive(Debug, Clone, Default)]
pub struct SnifferApp {
    events: Vec<SniffEvent>,
    selected: usize,
    filter: String,
    rules: Vec<closure_sniffer::Rule>,
}

impl SnifferApp {
    /// A fresh sniffer surface.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture `candidate`, deciding its action via `backend` (and any
    /// user rules already added). Appended to the event list.
    pub fn record(&mut self, candidate: &str, backend: &impl closure_sniffer::CaptureBackend) {
        let action = self
            .user_action(candidate)
            .or_else(|| backend.match_action(candidate));
        self.events.push(SniffEvent {
            candidate: candidate.to_owned(),
            action,
        });
    }

    /// The action a user rule decides for `candidate`, if any (user rules
    /// take precedence over the backend).
    fn user_action(&self, candidate: &str) -> Option<closure_sniffer::Action> {
        closure_sniffer::match_first(candidate, &self.rules).map(|r| r.action)
    }

    /// Every captured event, in capture order.
    #[must_use]
    pub fn events(&self) -> &[SniffEvent] {
        &self.events
    }

    /// The blocklist rules the toggles have added (for persistence to the
    /// `sniffer_blocklist` config).
    #[must_use]
    pub fn rules(&self) -> &[closure_sniffer::Rule] {
        &self.rules
    }

    /// Set the substring filter applied to [`Self::filtered`].
    pub fn set_filter(&mut self, filter: &str) {
        self.filter.clear();
        self.filter.push_str(filter);
        self.selected = 0;
    }

    /// Events whose candidate contains the filter (all when empty).
    #[must_use]
    pub fn filtered(&self) -> Vec<&SniffEvent> {
        self.events
            .iter()
            .filter(|e| self.filter.is_empty() || e.candidate.contains(&self.filter))
            .collect()
    }

    /// Move the cursor to filtered-row `i` (clamped).
    pub fn select(&mut self, i: usize) {
        let last = self.filtered().len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// The cursor index into [`Self::filtered`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Add a rule for the selected flow and re-decide every event.
    fn rule_for_selected(&mut self, action: closure_sniffer::Action) {
        let Some(candidate) = self
            .filtered()
            .get(self.selected)
            .map(|e| e.candidate.clone())
        else {
            return;
        };
        self.rules.push(closure_sniffer::Rule {
            id: format!("user-{}", self.rules.len()),
            pattern: candidate.clone(),
            action,
        });
        // Re-decide the matching events under the new user rule.
        for e in &mut self.events {
            if e.candidate == candidate {
                e.action = Some(action);
            }
        }
    }

    /// Block the selected flow (adds a `Block` rule).
    pub fn block_selected(&mut self) {
        self.rule_for_selected(closure_sniffer::Action::Block);
    }

    /// Allow the selected flow (adds an `Allow` rule, overriding any
    /// backend block).
    pub fn allow_selected(&mut self) {
        self.rule_for_selected(closure_sniffer::Action::Allow);
    }

    /// A description of the selected flow (candidate + action), or `None`.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        let e = self.filtered().get(self.selected).copied()?;
        let action = e
            .action
            .map_or_else(|| "(no rule)".to_owned(), |a| format!("{a:?}"));
        Some(format!("{}\naction: {action}", e.candidate))
    }

    /// The declarative [`Node`] tree for the sniffer surface (V7b): the
    /// filtered flow list, a detail pane offering block/allow (each
    /// carrying its chord, the V1 invariant), and the which-key hints.
    #[must_use]
    pub fn view(&self, mode: closure_config::InputMode) -> Node {
        let rows: Vec<RowView> = self
            .filtered()
            .iter()
            .map(|e| RowView {
                id: e.candidate.clone(),
                title: e.candidate.clone(),
                level: 1,
                todo: e.action.map(|a| format!("{a:?}")),
            })
            .collect();
        let mut children = vec![Node::Rows {
            rows,
            selected: self.selected,
        }];
        if let Some(e) = self.filtered().get(self.selected).copied() {
            let mut fields = vec![FieldView {
                label: "flow".to_owned(),
                value: e.candidate.clone(),
                action: None,
            }];
            fields.push(FieldView {
                label: "action".to_owned(),
                value: e
                    .action
                    .map_or_else(|| "(no rule)".to_owned(), |a| format!("{a:?}")),
                action: None,
            });
            fields.push(FieldView {
                label: "block".to_owned(),
                value: String::new(),
                action: Action::new(mode, "block-flow"),
            });
            fields.push(FieldView {
                label: "allow".to_owned(),
                value: String::new(),
                action: Action::new(mode, "allow-flow"),
            });
            children.push(Node::Detail { fields });
        }
        children.push(Node::Hints {
            line: format!("[{mode:?}] sniffer — {} flows", self.events.len()),
        });
        Node::Pane {
            title: "sniffer".to_owned(),
            children,
        }
    }
}

/// The kind of a [`Node`], for the type-level UI capability matrix (V1c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// [`Node::Pane`].
    Pane,
    /// [`Node::Rows`].
    Rows,
    /// [`Node::Detail`].
    Detail,
    /// [`Node::Input`].
    Input,
    /// [`Node::Palette`].
    Palette,
    /// [`Node::Hints`].
    Hints,
    /// [`Node::Widget`].
    Widget,
    /// [`Node::Text`].
    Text,
}

impl Node {
    /// This node's [`NodeKind`].
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::Pane { .. } => NodeKind::Pane,
            Self::Rows { .. } => NodeKind::Rows,
            Self::Detail { .. } => NodeKind::Detail,
            Self::Input { .. } => NodeKind::Input,
            Self::Palette { .. } => NodeKind::Palette,
            Self::Hints { .. } => NodeKind::Hints,
            Self::Widget { .. } => NodeKind::Widget,
            Self::Text(_) => NodeKind::Text,
        }
    }
}

/// Every node kind, in definition order (single source of truth).
pub const ALL_NODE_KINDS: &[NodeKind] = &[
    NodeKind::Pane,
    NodeKind::Rows,
    NodeKind::Detail,
    NodeKind::Input,
    NodeKind::Palette,
    NodeKind::Hints,
    NodeKind::Widget,
    NodeKind::Text,
];

/// The floor every shell must render to host the launcher (I7 for UI):
/// a labelled region, a list, the which-key line, inert text.
pub const MINIMAL_NODE_KINDS: &[NodeKind] = &[
    NodeKind::Pane,
    NodeKind::Rows,
    NodeKind::Hints,
    NodeKind::Text,
];

/// Kinds the TUI renderer (`closure_tui::render_view`) handles — all of
/// them (its match is exhaustive, so this cannot silently drift: adding
/// a `NodeKind` without a render arm is a compile error).
pub const TUI_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// Kinds the web renderer (`closure_shell_web::render_view`) handles —
/// all of them, exhaustively.
pub const WEB_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// The node kinds a shell does *not* render (`ALL_NODE_KINDS` minus
/// `kinds`). Empty for a complete renderer.
#[must_use]
pub fn missing_node_kinds(kinds: &[NodeKind]) -> Vec<NodeKind> {
    ALL_NODE_KINDS
        .iter()
        .copied()
        .filter(|k| !kinds.contains(k))
        .collect()
}

/// Render the shell × node-kind venn/diff table (code = single source of
/// truth; mirrors `closure shells`). `closure ui-matrix` prints it.
#[must_use]
pub fn ui_matrix_table() -> String {
    use std::fmt::Write as _;
    let shells: &[(&str, &[NodeKind])] = &[
        ("MIN", MINIMAL_NODE_KINDS),
        ("TUI", TUI_NODE_KINDS),
        ("WEB", WEB_NODE_KINDS),
    ];
    let mut out =
        String::from("UI node-kind matrix (which shells render which ViewTree nodes)\n\n");
    let _ = write!(out, "{:<9}", "NodeKind");
    for (name, _) in shells {
        let _ = write!(out, " | {name}");
    }
    out.push('\n');
    for kind in ALL_NODE_KINDS {
        let _ = write!(out, "{:<9}", format!("{kind:?}"));
        for (_, set) in shells {
            let mark = if set.contains(kind) { " X " } else { "   " };
            let _ = write!(out, " | {mark}");
        }
        out.push('\n');
    }
    out.push_str("\nLegend: X = renders this node kind. MIN = the floor (I7).\n");
    out
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
        let body = self.detail(shell).map(|d| d.body).unwrap_or_default();
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
                if k == "TODO" {
                    "todo: TODO"
                } else {
                    "todo: DONE"
                }
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
                return format!(
                    "[{:?}] type: filter   {}",
                    self.input_mode,
                    self.command_hints()
                );
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

    /// The declarative [`Node`] tree describing the current screen (V1).
    ///
    /// Pure function of state — any embedder (TUI, web, egui, gpui)
    /// renders the same tree. Every actionable node carries its chord by
    /// construction ([`Action`]), so the "show keybinding everywhere"
    /// rule cannot be violated.
    #[must_use]
    pub fn view(&self, shell: &Shell) -> Node {
        let hints = Node::Hints {
            line: self.key_hints(),
        };
        let m = self.input_mode;
        let input_pane = |label: &str, buffer: &str| Node::Pane {
            title: label.to_owned(),
            children: vec![
                Node::Input {
                    label: label.to_owned(),
                    buffer: buffer.to_owned(),
                },
                hints.clone(),
            ],
        };
        match self.mode {
            Mode::Browse => {
                let rows = self
                    .rows(shell)
                    .into_iter()
                    .map(|r| RowView {
                        id: r.id,
                        title: r.title,
                        level: r.level,
                        todo: r.todo,
                    })
                    .collect();
                let mut children = vec![Node::Rows {
                    rows,
                    selected: self.selected,
                }];
                if let Some(d) = self.detail(shell) {
                    children.push(Self::detail_node(m, &d));
                }
                children.push(hints);
                Node::Pane {
                    title: "closure".to_owned(),
                    children,
                }
            }
            Mode::Palette => {
                let items = PALETTE_COMMANDS
                    .iter()
                    .filter_map(|(label, canonical)| {
                        let matches = self.capture_buf.is_empty()
                            || closure_query::fuzzy_score(&self.capture_buf, label).is_some();
                        if !matches {
                            return None;
                        }
                        Action::new(m, *canonical).map(|action| PaletteItemView {
                            label: (*label).to_owned(),
                            action,
                        })
                    })
                    .collect();
                Node::Pane {
                    title: "palette".to_owned(),
                    children: vec![
                        Node::Palette {
                            items,
                            cursor: self.palette_cursor,
                        },
                        hints,
                    ],
                }
            }
            Mode::Capture => input_pane("capture", &self.capture_buf),
            Mode::Rename => input_pane("rename", &self.capture_buf),
            Mode::AddSibling => input_pane("add sibling", &self.capture_buf),
            Mode::EditBody => input_pane("edit body", &self.body_buf),
            Mode::TagsEdit => input_pane("tags", &self.tags_buf),
            Mode::PropertyEdit => Node::Pane {
                title: "property".to_owned(),
                children: vec![
                    Node::Input {
                        label: "key".to_owned(),
                        buffer: self.prop_key.clone(),
                    },
                    Node::Input {
                        label: "value".to_owned(),
                        buffer: self.prop_value.clone(),
                    },
                    hints,
                ],
            },
        }
    }

    /// Build the detail pane node, attaching the click-to-edit action
    /// (with its chord, V1 invariant) to each editable field.
    fn detail_node(mode: closure_config::InputMode, d: &Detail) -> Node {
        let mut fields = vec![FieldView {
            label: "title".to_owned(),
            value: d.title.clone(),
            action: Action::new(mode, "rename"),
        }];
        fields.push(FieldView {
            label: "todo".to_owned(),
            value: d.todo.clone().unwrap_or_default(),
            action: Action::new(mode, "toggle-todo"),
        });
        if let Some(p) = d.priority {
            fields.push(FieldView {
                label: "priority".to_owned(),
                value: p.to_string(),
                action: Action::new(mode, "cycle-priority"),
            });
        }
        fields.push(FieldView {
            label: "tags".to_owned(),
            value: d.tags.join(" "),
            action: Action::new(mode, "edit-tags"),
        });
        for (k, v) in &d.properties {
            fields.push(FieldView {
                label: k.clone(),
                value: v.clone(),
                action: Action::new(mode, "edit-property"),
            });
        }
        fields.push(FieldView {
            label: "body".to_owned(),
            value: d.body.clone(),
            action: Action::new(mode, "edit-body"),
        });
        Node::Detail { fields }
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
    /// Read-only list of headlines linking to the selected one.
    Backlinks,
    /// Read-only agenda: scheduled/deadline entries across the vault.
    Agenda,
    /// Read-only list of every `#+BEGIN_SRC` block across the vault.
    Blocks,
    /// Editing the selected headline's tags (space-separated buffer).
    TagsEdit,
    /// Editing a property on the selected headline (`key value` buffer).
    PropertyEdit,
}

/// Which read-only list a generic list surface is showing (drives the
/// shared navigation handler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Agenda,
    Blocks,
}

/// Which field a single-line modal field-edit surface is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Tags,
    Property,
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
    /// Headline id whose backlinks the Backlinks surface is showing.
    link_target: Option<String>,
    /// Target id + single-line buffer for the TagsEdit/PropertyEdit
    /// surfaces (tags: space-separated; property: `key value`).
    field_target: Option<String>,
    field_buf: String,
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
            link_target: None,
            field_target: None,
            field_buf: String::new(),
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

    /// The chord strokes entered so far but not yet resolved, joined
    /// (e.g. `"g"` after pressing `g`, `"C-x"` mid emacs chord). Empty
    /// when nothing is pending. Backs the which-key popup.
    #[must_use]
    pub fn pending_chord(&self) -> String {
        self.pending.join(" ")
    }

    /// While a multi-stroke chord is pending, the `(remaining, command)`
    /// completions from the active mode's keymap — every binding whose
    /// chord extends the pending prefix, with the prefix stripped.
    /// Empty when nothing is pending. Sorted by remaining chord.
    #[must_use]
    pub fn completions(&self) -> Vec<(String, String)> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let prefix = format!("{} ", self.pending.join(" "));
        let mut out: Vec<(String, String)> = closure_input::mode_keymap(self.mode)
            .iter()
            .filter_map(|(chord, cmd)| {
                chord
                    .strip_prefix(&prefix)
                    .map(|rest| (rest.to_owned(), (*cmd).to_owned()))
            })
            .collect();
        out.sort();
        out
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
            ModalSurface::Backlinks => self.on_backlinks_key(shell, key),
            ModalSurface::Agenda => self.on_list_key(shell, key, ListKind::Agenda),
            ModalSurface::Blocks => self.on_list_key(shell, key, ListKind::Blocks),
            ModalSurface::TagsEdit => self.on_field_key(shell, key, text, FieldKind::Tags),
            ModalSurface::PropertyEdit => self.on_field_key(shell, key, text, FieldKind::Property),
            ModalSurface::Browse => self.on_browse_key(shell, key, ctrl, alt, text),
        }
    }

    /// Single-line field editor (tags / property): Enter commits through
    /// the Shell setter (I8), Esc cancels, Backspace deletes, printable
    /// chars append. Tags split on whitespace; property splits on the
    /// first space into `key value`.
    fn on_field_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>, kind: FieldKind) {
        match key {
            "escape" => {
                self.field_target = None;
                self.field_buf.clear();
                self.surface = ModalSurface::Browse;
            }
            "enter" => {
                if let Some(id) = self.field_target.take() {
                    let bid = closure_core::BlockId::from_existing(&id);
                    match kind {
                        FieldKind::Tags => {
                            let tags: Vec<String> = self
                                .field_buf
                                .split_whitespace()
                                .map(ToOwned::to_owned)
                                .collect();
                            let _ = shell.set_tags(&bid, &tags);
                        }
                        FieldKind::Property => {
                            if let Some((k, v)) = self.field_buf.split_once(' ') {
                                let _ = shell.set_property(&bid, k.trim(), v.trim());
                            } else if !self.field_buf.trim().is_empty() {
                                let _ = shell.set_property(&bid, self.field_buf.trim(), "");
                            }
                        }
                    }
                }
                self.field_buf.clear();
                self.surface = ModalSurface::Browse;
            }
            "backspace" => {
                self.field_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.field_buf.push(c);
                }
            }
        }
    }

    /// The single-line field-edit buffer (tags/property).
    #[must_use]
    pub fn field_buffer(&self) -> &str {
        &self.field_buf
    }

    /// Generic up/down/Esc navigation for the read-only list surfaces
    /// (agenda, blocks) whose rows don't drive a jump.
    fn on_list_key(&mut self, shell: &Shell, key: &str, kind: ListKind) {
        let len = match kind {
            ListKind::Agenda => self.agenda_rows(shell).len(),
            ListKind::Blocks => self.block_rows(shell).len(),
        };
        match key {
            "escape" => {
                self.selected = 0;
                self.surface = ModalSurface::Browse;
            }
            "down" | "j" => self.selected = (self.selected + 1).min(len.saturating_sub(1)),
            "up" | "k" => self.selected = self.selected.saturating_sub(1),
            "enter" => self.jump_list_row(shell, self.selected),
            _ => {}
        }
    }

    /// Activate row `i` on the active list surface: navigate Browse to
    /// the target headline (agenda: by id; blocks: first headline of the
    /// block's file) and return to Browse. Out-of-range just returns to
    /// Browse. No-op shape on non-list surfaces.
    pub fn jump_list_row(&mut self, shell: &Shell, i: usize) {
        let target_path = match self.surface {
            ModalSurface::Agenda => {
                let id = shell.vault.agenda().into_iter().nth(i).map(|e| e.id);
                self.surface = ModalSurface::Browse;
                self.selected = 0;
                if let Some(id) = id
                    && let Some(idx) = self.rows(shell).iter().position(|r| r.id == id)
                {
                    self.selected = idx;
                }
                return;
            }
            ModalSurface::Blocks => self.block_rows(shell).into_iter().nth(i).map(|(p, _, _)| p),
            _ => None,
        };
        self.surface = ModalSurface::Browse;
        self.selected = 0;
        if let Some(path) = target_path
            && let Some(idx) = self.rows(shell).iter().position(|r| r.path == path)
        {
            self.selected = idx;
        }
    }

    /// Agenda rows `(date, title, path)` across the vault, sorted by
    /// date then title (as [`closure_store::Vault::agenda`] returns).
    #[must_use]
    pub fn agenda_rows(&self, shell: &Shell) -> Vec<(String, String, String)> {
        shell
            .vault
            .agenda()
            .into_iter()
            .map(|e| (e.date, e.title, e.path.display().to_string()))
            .collect()
    }

    /// Every `#+BEGIN_SRC` block across the vault as `(path, lang,
    /// first-line)` rows, in per-file document order.
    #[must_use]
    pub fn block_rows(&self, shell: &Shell) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (path, doc) in shell.vault.iter() {
            // Reuse the tested prose/code segmenter over the whole file
            // source (catches preamble + headline blocks uniformly).
            for seg in segment_body(&doc.source()) {
                if let BodySegment::Code { lang, text } = seg {
                    let first = text.lines().next().unwrap_or("").trim().to_owned();
                    out.push((path.display().to_string(), lang, first));
                }
            }
        }
        out
    }

    /// Backlinks list keys: up/down move, Enter jumps to the selected
    /// backlink (navigates Browse to it), Esc returns to Browse.
    fn on_backlinks_key(&mut self, shell: &Shell, key: &str) {
        match key {
            "escape" => {
                self.link_target = None;
                self.selected = 0;
                self.surface = ModalSurface::Browse;
            }
            "down" | "j" => {
                let last = self.backlink_rows(shell).len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
            "up" | "k" => self.selected = self.selected.saturating_sub(1),
            "enter" => {
                // Jump: make the chosen backlink the Browse selection.
                if let Some((_, title)) = self.backlink_rows(shell).get(self.selected).cloned() {
                    self.link_target = None;
                    self.surface = ModalSurface::Browse;
                    if let Some(idx) = self.rows(shell).iter().position(|r| r.title == title) {
                        self.selected = idx;
                    }
                }
            }
            _ => {}
        }
    }

    /// Headlines that link to the headline the Backlinks surface was
    /// opened on: `(path, title)` rows from the vault backlink index.
    #[must_use]
    pub fn backlink_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        let Some(target) = &self.link_target else {
            return Vec::new();
        };
        shell
            .vault
            .backlinks_of(target)
            .iter()
            .map(|(path, src_id)| {
                let title = shell
                    .vault
                    .find_by_id(src_id)
                    .map_or_else(|| src_id.to_string(), |(h, _)| h.title().to_owned());
                (path.display().to_string(), title)
            })
            .collect()
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
        shell: &mut Shell,
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

    fn run_command(&mut self, shell: &mut Shell, cmd: &str) {
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
            "backlinks" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    self.link_target = Some(row.id);
                    self.selected = 0;
                    self.surface = ModalSurface::Backlinks;
                }
            }
            "agenda" => {
                self.selected = 0;
                self.surface = ModalSurface::Agenda;
            }
            "block-list" => {
                self.selected = 0;
                self.surface = ModalSurface::Blocks;
            }
            "toggle-todo" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    let next = match self.detail(shell).and_then(|d| d.todo) {
                        None => Some("TODO"),
                        Some(k) if k == "TODO" => Some("DONE"),
                        Some(_) => None,
                    };
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.set_todo(&bid, next);
                }
            }
            "cycle-priority" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    let next = match self.detail(shell).and_then(|d| d.priority) {
                        None => Some('A'),
                        Some('A') => Some('B'),
                        Some('B') => Some('C'),
                        Some(_) => None,
                    };
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.set_priority(&bid, next);
                }
            }
            "edit-tags" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    let tags = self.detail(shell).map(|d| d.tags).unwrap_or_default();
                    self.field_target = Some(row.id);
                    self.field_buf = tags.join(" ");
                    self.surface = ModalSurface::TagsEdit;
                }
            }
            "edit-property" => {
                if let Some(row) = self.rows(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf.clear();
                    self.surface = ModalSurface::PropertyEdit;
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
