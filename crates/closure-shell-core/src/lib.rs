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
pub use closure_sniffer::Action as FlowAction;
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

    /// Promote a headline one level through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn promote(&mut self, id: &closure_core::BlockId) -> Result<(), closure_store::VaultError> {
        self.vault.promote(id)
    }

    /// Demote a headline one level through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn demote(&mut self, id: &closure_core::BlockId) -> Result<(), closure_store::VaultError> {
        self.vault.demote(id)
    }

    /// Move `id`'s subtree right after `after`'s through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn move_after(
        &mut self,
        id: &closure_core::BlockId,
        after: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.move_after(id, after)
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

/// One agenda row for the GUI agenda pane, flags precomputed
/// against an injected today so shells stay hermetic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaRow {
    /// Date in YYYY-MM-DD form.
    pub date: String,
    /// SCHEDULED or DEADLINE.
    pub kind: String,
    /// Headline title.
    pub title: String,
    /// True when date equals the injected today.
    pub is_today: bool,
    /// True when date lies before the injected today.
    pub is_overdue: bool,
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
    /// Leading status glyph (icon-as-data, G5a) — e.g. a todo-state
    /// marker. The embedder maps it to a real icon.
    pub icon: Option<String>,
    /// Metadata chips (G5a) — tags, a priority letter, … shown after the
    /// title as Notion-style badges.
    pub badges: Vec<String>,
}

impl RowView {
    /// A row with no icon and no badges (the common case).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        level: u8,
        todo: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            level,
            todo,
            icon: None,
            badges: Vec::new(),
        }
    }

    /// Set the leading icon glyph (G5a).
    #[must_use]
    pub fn with_icon(mut self, icon: Option<String>) -> Self {
        self.icon = icon;
        self
    }

    /// Set the metadata badge chips (G5a).
    #[must_use]
    pub fn with_badges(mut self, badges: Vec<String>) -> Self {
        self.badges = badges;
        self
    }
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
    /// A multi-pane split layout (G1a): child panes arranged along an
    /// axis. The foundation for a real editor surface (sidebar + main +
    /// detail). Renderers lay the panes out along [`SplitDir`]; the
    /// hermetic guarantee is the pane *set + order + axis*, not pixels.
    Split {
        /// Axis the panes are arranged along.
        direction: SplitDir,
        /// Child panes, in render order.
        panes: Vec<Self>,
    },
    /// A modal overlay (G1b): a titled layer floating above the base
    /// surface — the command palette, a confirm dialog, a prompt. The
    /// embedder paints it as a focused dialog; the hermetic guarantee is
    /// the title + body content, not the dimming/animation.
    Modal {
        /// Dialog heading (also the accessible label).
        title: String,
        /// The content shown inside the overlay.
        body: Box<Self>,
    },
    /// A transient notification (G1c): the severity-classed toast a shell
    /// flashes for an async outcome (sync done, eval failed, …). The
    /// embedder decides the placement/auto-dismiss; the hermetic guarantee
    /// is the severity + message.
    Toast {
        /// Severity, driving styling + the a11y live-region politeness.
        level: ToastLevel,
        /// The message shown.
        text: String,
    },
}

/// Severity of a [`Node::Toast`] notification (G1c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastLevel {
    /// Neutral information.
    Info,
    /// A completed action.
    Success,
    /// A non-fatal caution.
    Warning,
    /// A failure.
    Error,
}

impl ToastLevel {
    /// Stable lowercase tag for serialisation / CSS class suffixes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// The ARIA live-region role: assertive `alert` for warning/error,
    /// polite `status` for info/success.
    #[must_use]
    pub const fn aria_role(self) -> &'static str {
        match self {
            Self::Warning | Self::Error => "alert",
            Self::Info | Self::Success => "status",
        }
    }
}

/// The axis a [`Node::Split`] arranges its panes along (G1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SplitDir {
    /// Panes side by side, left to right.
    Row,
    /// Panes stacked, top to bottom.
    Column,
}

impl SplitDir {
    /// Stable lowercase tag for serialisation / CSS class suffixes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Column => "column",
        }
    }
}

/// Build a [`Node::Split`] from an axis and its panes (G1a).
#[must_use]
pub const fn split_node(direction: SplitDir, panes: Vec<Node>) -> Node {
    Node::Split { direction, panes }
}

/// Build a [`Node::Modal`] overlay from a title and its body (G1b).
#[must_use]
pub fn modal_node(title: impl Into<String>, body: Node) -> Node {
    Node::Modal {
        title: title.into(),
        body: Box::new(body),
    }
}

/// Build a [`Node::Toast`] notification from a severity and message (G1c).
#[must_use]
pub fn toast_node(level: ToastLevel, text: impl Into<String>) -> Node {
    Node::Toast {
        level,
        text: text.into(),
    }
}

/// An `#rrggbb` colour token (G2). Holds a static hex string; the shells
/// map it to a native colour (CSS value, ratatui `Color::Rgb`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub &'static str);

impl Color {
    /// The `#rrggbb` hex string.
    #[must_use]
    pub const fn hex(self) -> &'static str {
        self.0
    }

    /// Parse the hex to an `(r, g, b)` byte triple. Malformed input
    /// resolves to black — never a panic (I5), so a bad theme token can't
    /// crash a render.
    #[must_use]
    pub fn rgb(self) -> (u8, u8, u8) {
        let h = self.0.strip_prefix('#').unwrap_or(self.0);
        if h.len() != 6 || !h.is_ascii() {
            return (0, 0, 0);
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
        (byte(0), byte(2), byte(4))
    }
}

/// A semantic colour slot in a [`Theme`] palette (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRole {
    /// Foreground / primary text.
    Fg,
    /// Background / surface.
    Bg,
    /// Accent / interactive highlight.
    Accent,
    /// De-emphasised / secondary text.
    Muted,
    /// Selection / active-row highlight.
    Selection,
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
    /// Success severity.
    Success,
    /// Second-level heading (doom-vibrant outline-2 magenta).
    Heading2,
    /// Third-level heading (doom-vibrant outline-3 violet).
    Heading3,
    /// Inline/source code (doom-vibrant org-code orange).
    Code,
}

/// The named colour slots of a theme (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Foreground / primary text.
    pub fg: Color,
    /// Background / surface.
    pub bg: Color,
    /// Accent / interactive highlight.
    pub accent: Color,
    /// De-emphasised / secondary text.
    pub muted: Color,
    /// Selection / active-row highlight.
    pub selection: Color,
    /// Error severity.
    pub error: Color,
    /// Warning severity.
    pub warning: Color,
    /// Success severity.
    pub success: Color,
    /// Second-level heading colour.
    pub heading2: Color,
    /// Third-level heading colour.
    pub heading3: Color,
    /// Inline/source code colour.
    pub code: Color,
}

/// The spacing scale of a theme, in pixels (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacing {
    /// Base spacing unit.
    pub unit_px: u16,
    /// Inter-element gap.
    pub gap_px: u16,
}

/// The typography of a theme (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typography {
    /// Body font stack.
    pub font_family: &'static str,
    /// Monospace font stack (code blocks).
    pub mono_family: &'static str,
    /// Base font size, in pixels.
    pub base_px: u16,
}

/// A declarative, typed theme: palette + spacing + typography as data
/// (G2). Resolved from the free-form `config.theme` string via
/// [`Theme::from_name`]; each shell maps the tokens to its native style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Stable lowercase theme name (`dark` / `light` / `high-contrast`).
    pub name: &'static str,
    /// Colour palette.
    pub palette: Palette,
    /// Spacing scale.
    pub spacing: Spacing,
    /// Typography.
    pub typography: Typography,
}

impl Theme {
    /// The default dark theme.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            name: "dark",
            palette: Palette {
                fg: Color("#cdd6f4"),
                bg: Color("#1e1e2e"),
                accent: Color("#89b4fa"),
                muted: Color("#6c7086"),
                selection: Color("#45475a"),
                error: Color("#f38ba8"),
                warning: Color("#f9e2af"),
                success: Color("#a6e3a1"),
                heading2: Color("#cba6f7"),
                heading3: Color("#b4befe"),
                code: Color("#fab387"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Inter, system-ui, sans-serif",
                mono_family: "JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// The light theme.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            name: "light",
            palette: Palette {
                fg: Color("#4c4f69"),
                bg: Color("#eff1f5"),
                accent: Color("#1e66f5"),
                muted: Color("#9ca0b0"),
                selection: Color("#ccd0da"),
                error: Color("#d20f39"),
                warning: Color("#df8e1d"),
                success: Color("#40a02b"),
                heading2: Color("#8839ef"),
                heading3: Color("#7287fd"),
                code: Color("#fe640b"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Inter, system-ui, sans-serif",
                mono_family: "JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// The high-contrast theme: pure white on black, larger base size.
    #[must_use]
    pub const fn high_contrast() -> Self {
        Self {
            name: "high-contrast",
            palette: Palette {
                fg: Color("#ffffff"),
                bg: Color("#000000"),
                accent: Color("#ffff00"),
                muted: Color("#c0c0c0"),
                selection: Color("#0000ff"),
                error: Color("#ff0000"),
                warning: Color("#ffaa00"),
                success: Color("#00ff00"),
                heading2: Color("#ff00ff"),
                heading3: Color("#00ffff"),
                code: Color("#ffa500"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Inter, system-ui, sans-serif",
                mono_family: "JetBrains Mono, ui-monospace, monospace",
                base_px: 16,
            },
        }
    }

    /// The Doom Emacs `doom-vibrant` palette (the user's colorscheme;
    /// gui-color face values). Org face mapping: TODO green, DONE
    /// muted, date yellow, code orange, outline-1 blue / outline-2
    /// magenta / outline-3 violet.
    #[must_use]
    pub const fn doom_vibrant() -> Self {
        Self {
            name: "doom-vibrant",
            palette: Palette {
                fg: Color("#bbc2cf"),
                bg: Color("#242730"),
                accent: Color("#51afef"),
                muted: Color("#62686e"),
                selection: Color("#3d4451"),
                error: Color("#ff665c"),
                warning: Color("#fcce7b"),
                success: Color("#7bc275"),
                heading2: Color("#c57bdb"),
                heading3: Color("#a991f1"),
                code: Color("#e69055"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Inter, system-ui, sans-serif",
                mono_family: "JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// Resolve a theme from the free-form `config.theme` string
    /// (case-insensitive): `light`, `high-contrast`/`hc`,
    /// `doom-vibrant`/`vibrant`, else `dark`.
    #[must_use]
    pub const fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("light") {
            Self::light()
        } else if name.eq_ignore_ascii_case("high-contrast") || name.eq_ignore_ascii_case("hc") {
            Self::high_contrast()
        } else if name.eq_ignore_ascii_case("doom-vibrant") || name.eq_ignore_ascii_case("vibrant")
        {
            Self::doom_vibrant()
        } else {
            Self::dark()
        }
    }

    /// The colour for a semantic [`ColorRole`].
    #[must_use]
    pub const fn color(&self, role: ColorRole) -> Color {
        match role {
            ColorRole::Fg => self.palette.fg,
            ColorRole::Bg => self.palette.bg,
            ColorRole::Accent => self.palette.accent,
            ColorRole::Muted => self.palette.muted,
            ColorRole::Selection => self.palette.selection,
            ColorRole::Error => self.palette.error,
            ColorRole::Warning => self.palette.warning,
            ColorRole::Success => self.palette.success,
            ColorRole::Heading2 => self.palette.heading2,
            ColorRole::Heading3 => self.palette.heading3,
            ColorRole::Code => self.palette.code,
        }
    }
}

/// Build a [`Node::Widget`] from a name and its expanded content (V2c).
#[must_use]
pub fn widget_node(name: impl Into<String>, content: impl Into<String>) -> Node {
    Node::Widget {
        name: name.into(),
        content: content.into(),
    }
}

/// Marker shown on a folded row (`:VISIBILITY: folded`).
const FOLD_MARKER: &str = "▸";

/// Whether the headline is folded — the org-standard
/// `:VISIBILITY: folded` property (the same one Emacs org-mode reads),
/// so fold state lives in the document and persists between runs.
fn headline_is_folded(h: &closure_core::DocHeadline) -> bool {
    h.properties()
        .iter()
        .any(|(k, v)| k == "VISIBILITY" && v == "folded")
}

/// Whether the row with block id `id` is currently folded — the
/// renderer's source for the `▸` marker and the fold-arrow click.
#[must_use]
pub fn is_row_folded(shell: &Shell, id: &str) -> bool {
    row_is_folded(shell, id)
}

/// Whether the row with block id `id` is currently folded.
fn row_is_folded(shell: &Shell, id: &str) -> bool {
    let bid = closure_core::BlockId::from_existing(id);
    shell
        .vault
        .find_by_id(&bid)
        .is_some_and(|(h, _)| headline_is_folded(h))
}

/// Flip the `:VISIBILITY:` property on `id` between `folded` and `all`
/// through the registry (I8, undoable I3). Returns the new folded state,
/// or `None` when the write failed.
fn toggle_visibility(shell: &mut Shell, id: &closure_core::BlockId) -> Option<bool> {
    let folded = shell
        .vault
        .find_by_id(id)
        .is_some_and(|(h, _)| headline_is_folded(h));
    let next = if folded { "all" } else { "folded" };
    shell
        .set_property(id, "VISIBILITY", next)
        .ok()
        .map(|()| !folded)
}

/// Map a TODO keyword to a leading status glyph (G5a): `DONE`-like
/// keywords get a filled marker, open ones a hollow marker, anything else
/// a diamond.
fn todo_glyph(keyword: &str) -> &'static str {
    match keyword {
        "DONE" | "CANCELLED" | "KILL" => "●",
        "TODO" | "NEXT" | "WAIT" => "○",
        _ => "◆",
    }
}

/// Overlay ephemeral peer presence onto rows (Q11-C3).
///
/// A `◉ <peer>` badge is appended to the row whose id a peer is
/// focused on. Pure — presence never touches document state or the
/// undo tree; every shell that renders badges (G5a) shows it with no
/// new code.
#[must_use]
pub fn with_presence(mut rows: Vec<RowView>, presence: &[(String, String)]) -> Vec<RowView> {
    for row in &mut rows {
        for (peer, block) in presence {
            if row.id == *block {
                row.badges.push(format!("◉ {peer}"));
            }
        }
    }
    rows
}

/// Deterministic seeded vault generator (Q12-B1).
///
/// `files` org files of `headlines_per_file` level-1 headlines with
/// stable `:ID:`s, varied TODO/tags/bodies — the big-vault fixture
/// without committing thousands of files. Same `(files, headlines,
/// seed)` always yields identical bytes (I6); every file parses and
/// roundtrips (I1, tested). Pure xorshift, no dependency.
#[must_use]
pub fn gen_vault(files: usize, headlines_per_file: usize, seed: u64) -> Vec<(String, String)> {
    use std::fmt::Write as _;
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).max(1);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let words = [
        "parser", "vault", "agenda", "widget", "kernel", "backlink", "capture", "formula",
    ];
    let mut out = Vec::with_capacity(files);
    for f in 0..files {
        let mut src = String::new();
        for h in 0..headlines_per_file {
            let r = next();
            let todo = match r % 3 {
                0 => "TODO ",
                1 => "DONE ",
                _ => "",
            };
            let ri = usize::try_from(r % 1_000_003).unwrap_or(0);
            let w1 = words[(ri / 3) % words.len()];
            let w2 = words[(ri / 7) % words.len()];
            let tag = if r % 5 == 0 { " :work:" } else { "" };
            let _ = writeln!(src, "* {todo}{w1} {w2} {f}-{h}{tag}");
            let _ = writeln!(src, ":PROPERTIES:");
            let _ = writeln!(
                src,
                ":ID: 01H{:023X}",
                (u128::from(r) << 16) | ((f as u128) << 8) | h as u128
            );
            let _ = writeln!(src, ":END:");
            let _ = writeln!(src, "body {w2} line {}", r % 97);
        }
        out.push((format!("gen-{f:03}.org"), src));
    }
    out
}

/// Build the default browse [`ViewTree`](Node) from a borrowed vault
/// (V3a).
///
/// Every headline becomes a row (vault iteration order), selection 0,
/// plus a hint line. Borrow-friendly (no [`Shell`] ownership) so callers
/// like the LLM `view-render` tool can snapshot the screen. Rows carry an
/// icon (TODO glyph) + badges (tags) as data (G5a).
#[must_use]
pub fn browse_view(vault: &closure_store::Vault) -> Node {
    let rows: Vec<RowView> = vault
        .iter()
        .flat_map(|(_p, doc)| {
            doc.all_headlines().map(|h| {
                RowView::new(
                    h.id().to_string(),
                    h.title(),
                    h.level(),
                    h.todo().map(ToOwned::to_owned),
                )
                .with_icon(h.todo().map(|t| todo_glyph(t).to_owned()))
                .with_badges(h.tags().iter().map(ToOwned::to_owned).collect())
            })
        })
        .collect();
    let line = format!("[Notion] {} headlines — type: filter", rows.len());
    Node::Pane {
        title: "closure".to_owned(),
        children: vec![Node::Rows { rows, selected: 0 }, Node::Hints { line }],
    }
}

/// Escape a string for a JSON double-quoted literal (dep-free).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Serialise a [`ViewTree`](Node) to compact JSON (V13), dep-free (no
/// `serde`).
///
/// Each node object carries its kind tag (`k`), its
/// [`aria_role`](Node::aria_role), and its data — enough for a tiny
/// client-side renderer to rebuild the `DOM`, so a self-contained HTML
/// export can render the declarative tree with no server and no toolchain.
#[must_use]
// One flat arm per `Node` kind — exhaustive by design (V1c); splitting
// the match would only hide the one-to-one kind→JSON mapping.
#[allow(clippy::too_many_lines)]
pub fn view_to_json(node: &Node) -> String {
    use std::fmt::Write as _;
    let role = json_str(node.aria_role());
    match node {
        Node::Pane { title, children } => {
            let kids: Vec<String> = children.iter().map(view_to_json).collect();
            format!(
                "{{\"k\":\"pane\",\"role\":{role},\"title\":{},\"children\":[{}]}}",
                json_str(title),
                kids.join(",")
            )
        }
        Node::Rows { rows, selected } => {
            let items = rows.iter().fold(String::new(), |mut s, r| {
                if !s.is_empty() {
                    s.push(',');
                }
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_str);
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_str);
                let badges = r
                    .badges
                    .iter()
                    .map(|b| json_str(b))
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = write!(
                    s,
                    "{{\"title\":{},\"todo\":{todo},\"level\":{},\"icon\":{icon},\"badges\":[{badges}]}}",
                    json_str(&r.title),
                    r.level
                );
                s
            });
            format!("{{\"k\":\"rows\",\"role\":{role},\"selected\":{selected},\"rows\":[{items}]}}")
        }
        Node::Detail { fields } => {
            let items = fields.iter().fold(String::new(), |mut s, f| {
                if !s.is_empty() {
                    s.push(',');
                }
                let chord = f
                    .action
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |a| json_str(a.chord()));
                let _ = write!(
                    s,
                    "{{\"label\":{},\"value\":{},\"chord\":{chord}}}",
                    json_str(&f.label),
                    json_str(&f.value)
                );
                s
            });
            format!("{{\"k\":\"detail\",\"role\":{role},\"fields\":[{items}]}}")
        }
        Node::Input { label, buffer } => format!(
            "{{\"k\":\"input\",\"role\":{role},\"label\":{},\"buffer\":{}}}",
            json_str(label),
            json_str(buffer)
        ),
        Node::Palette { items, cursor } => {
            let its = items.iter().fold(String::new(), |mut s, it| {
                if !s.is_empty() {
                    s.push(',');
                }
                let _ = write!(
                    s,
                    "{{\"label\":{},\"chord\":{}}}",
                    json_str(&it.label),
                    json_str(it.action.chord())
                );
                s
            });
            format!("{{\"k\":\"palette\",\"role\":{role},\"cursor\":{cursor},\"items\":[{its}]}}")
        }
        Node::Hints { line } => {
            format!(
                "{{\"k\":\"hints\",\"role\":{role},\"line\":{}}}",
                json_str(line)
            )
        }
        Node::Widget { name, content } => format!(
            "{{\"k\":\"widget\",\"role\":{role},\"name\":{},\"content\":{}}}",
            json_str(name),
            json_str(content)
        ),
        Node::Text(t) => format!(
            "{{\"k\":\"text\",\"role\":{role},\"text\":{}}}",
            json_str(t)
        ),
        Node::Split { direction, panes } => {
            let kids: Vec<String> = panes.iter().map(view_to_json).collect();
            format!(
                "{{\"k\":\"split\",\"role\":{role},\"dir\":{},\"panes\":[{}]}}",
                json_str(direction.as_str()),
                kids.join(",")
            )
        }
        Node::Modal { title, body } => format!(
            "{{\"k\":\"modal\",\"role\":{role},\"title\":{},\"body\":{}}}",
            json_str(title),
            view_to_json(body)
        ),
        Node::Toast { level, text } => format!(
            "{{\"k\":\"toast\",\"role\":{role},\"level\":{},\"text\":{}}}",
            json_str(level.as_str()),
            json_str(text)
        ),
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
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(String::new, |g| format!("{g} "));
                let badges = if r.badges.is_empty() {
                    String::new()
                } else {
                    format!("  :{}:", r.badges.join(":"))
                };
                let _ = writeln!(out, "{pad}  {mark} {icon}{todo}{}{badges}", r.title);
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
        Node::Split { direction, panes } => {
            let _ = writeln!(out, "{pad}SPLIT {}", direction.as_str());
            for p in panes {
                serialize_node(p, depth + 1, out);
            }
        }
        Node::Modal { title, body } => {
            let _ = writeln!(out, "{pad}MODAL {title}");
            serialize_node(body, depth + 1, out);
        }
        Node::Toast { level, text } => {
            let _ = writeln!(out, "{pad}TOAST {} {text}", level.as_str());
        }
    }
}

/// Type-level shell capabilities (V11).
///
/// Applies the Yesod "turn runtime bugs into compile-time errors" rule to
/// the shell/capability matrix: a shell may only invoke a capability it
/// *statically* declares via [`Supports`]. The sibling
/// [`NodeKind`]/`ui_matrix` data describes what each shell renders at
/// run-time; this makes a *wrong* invocation a compile error.
pub mod caps {
    mod sealed {
        pub trait Sealed {}
    }

    /// A capability marker (sealed — only the markers below implement it).
    pub trait Capability: sealed::Sealed {}

    macro_rules! capability {
        ($(#[$m:meta])* $name:ident) => {
            $(#[$m])*
            #[derive(Debug, Clone, Copy)]
            pub struct $name;
            impl sealed::Sealed for $name {}
            impl Capability for $name {}
        };
    }

    capability!(/// Read/browse the vault.
        Browse);
    capability!(/// Mutating edits (rename/add/delete/set).
        Edit);
    capability!(/// org-capture creation.
        Capture);
    capability!(/// Fuzzy / full-text search.
        Search);
    capability!(/// Babel / eval / tangle.
        Eval);
    capability!(/// Notion-style database views.
        Database);

    /// `S: Supports<C>` iff shell `S` provides capability `C`. A shell
    /// type implements it once per capability it offers.
    pub trait Supports<C: Capability> {}

    /// Compile-time proof that shell `S` supports capability `C`.
    ///
    /// A no-op at run-time; its only purpose is the bound `S: Supports<C>`,
    /// so a shell that does not declare a capability cannot call into it.
    ///
    /// Supported combinations compile:
    /// ```
    /// use closure_shell_core::caps::{capability_gate, TuiShell, Edit};
    /// capability_gate::<TuiShell, Edit>();
    /// ```
    ///
    /// An unsupported one does **not** — a whole class of shell/capability
    /// mismatch bugs cannot be written:
    /// ```compile_fail
    /// use closure_shell_core::caps::{capability_gate, ReadOnlyWebShell, Edit};
    /// capability_gate::<ReadOnlyWebShell, Edit>();
    /// ```
    pub const fn capability_gate<S: Supports<C>, C: Capability>() {}

    /// The full-featured TUI shell.
    #[derive(Debug, Clone, Copy)]
    pub struct TuiShell;
    impl Supports<Browse> for TuiShell {}
    impl Supports<Edit> for TuiShell {}
    impl Supports<Capture> for TuiShell {}
    impl Supports<Search> for TuiShell {}
    impl Supports<Eval> for TuiShell {}
    impl Supports<Database> for TuiShell {}

    /// The read-only web shell (browse + search; no editing).
    #[derive(Debug, Clone, Copy)]
    pub struct ReadOnlyWebShell;
    impl Supports<Browse> for ReadOnlyWebShell {}
    impl Supports<Search> for ReadOnlyWebShell {}
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
            .map(|e| {
                RowView::new(
                    e.candidate.clone(),
                    e.candidate.clone(),
                    1,
                    e.action.map(|a| format!("{a:?}")),
                )
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

/// Interactive 3-way conflict resolution surface (V9b).
///
/// Lists the [`FieldConflict`](closure_crdt::FieldConflict)s detected by
/// [`closure_crdt::conflicts`] and applies the user's ours/theirs choice
/// through the vault command path (undoable, I3/I8). Rendered as a
/// [`ViewTree`](Node) with the `resolve-ours`/`resolve-theirs` chords.
#[derive(Debug, Clone)]
pub struct ConflictApp {
    conflicts: Vec<closure_crdt::FieldConflict>,
    selected: usize,
    input_mode: closure_config::InputMode,
}

impl ConflictApp {
    /// Build a resolver over `conflicts` for input `mode`.
    #[must_use]
    pub const fn new(
        conflicts: Vec<closure_crdt::FieldConflict>,
        mode: closure_config::InputMode,
    ) -> Self {
        Self {
            conflicts,
            selected: 0,
            input_mode: mode,
        }
    }

    /// The outstanding conflicts.
    #[must_use]
    pub fn conflicts(&self) -> &[closure_crdt::FieldConflict] {
        &self.conflicts
    }

    /// The cursor index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Move the cursor (clamped).
    pub fn select(&mut self, i: usize) {
        let last = self.conflicts.len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// Resolve the selected conflict to our value.
    ///
    /// # Errors
    ///
    /// Propagates the vault command error.
    pub fn resolve_ours(&mut self, shell: &mut Shell) -> Result<(), closure_store::VaultError> {
        let Some(c) = self.conflicts.get(self.selected) else {
            return Ok(());
        };
        let (block, field, value) = (c.block.clone(), c.field, c.ours.clone());
        self.apply(shell, &block, field, &value)
    }

    /// Resolve the selected conflict to their value.
    ///
    /// # Errors
    ///
    /// Propagates the vault command error.
    pub fn resolve_theirs(&mut self, shell: &mut Shell) -> Result<(), closure_store::VaultError> {
        let Some(c) = self.conflicts.get(self.selected) else {
            return Ok(());
        };
        let (block, field, value) = (c.block.clone(), c.field, c.theirs.clone());
        self.apply(shell, &block, field, &value)
    }

    /// Apply `value` to the conflicting field via the command path, then
    /// drop the now-resolved conflict.
    fn apply(
        &mut self,
        shell: &mut Shell,
        block: &closure_core::BlockId,
        field: closure_crdt::ConflictField,
        value: &str,
    ) -> Result<(), closure_store::VaultError> {
        match field {
            closure_crdt::ConflictField::Title => shell.rename_headline(block, value)?,
            closure_crdt::ConflictField::Body => shell.set_body(block, value)?,
        }
        self.conflicts.remove(self.selected);
        if self.selected >= self.conflicts.len() {
            self.selected = self.conflicts.len().saturating_sub(1);
        }
        Ok(())
    }

    /// The declarative [`Node`] tree: the conflict list + a detail pane
    /// showing base/ours/theirs with the resolve actions (chords carried
    /// per the V1 invariant).
    #[must_use]
    pub fn view(&self) -> Node {
        let m = self.input_mode;
        let rows: Vec<RowView> = self
            .conflicts
            .iter()
            .map(|c| {
                RowView::new(
                    c.block.to_string(),
                    format!("{:?}: {}", c.field, c.block),
                    1,
                    None,
                )
            })
            .collect();
        let mut children = vec![Node::Rows {
            rows,
            selected: self.selected,
        }];
        if let Some(c) = self.conflicts.get(self.selected) {
            children.push(Node::Detail {
                fields: vec![
                    FieldView {
                        label: "base".to_owned(),
                        value: c.base.clone().unwrap_or_default(),
                        action: None,
                    },
                    FieldView {
                        label: "ours".to_owned(),
                        value: c.ours.clone(),
                        action: Action::new(m, "resolve-ours"),
                    },
                    FieldView {
                        label: "theirs".to_owned(),
                        value: c.theirs.clone(),
                        action: Action::new(m, "resolve-theirs"),
                    },
                ],
            });
        }
        children.push(Node::Hints {
            line: format!("[{m:?}] {} conflicts", self.conflicts.len()),
        });
        Node::Pane {
            title: "conflicts".to_owned(),
            children,
        }
    }
}

/// The interaction state of an element, for focus-ring / hover / pressed /
/// disabled styling (G5b). Derived by [`Interactions::state_of`] with a
/// fixed precedence; the embedder maps it to native styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementState {
    /// No interaction.
    Normal,
    /// Pointer is over the element.
    Hovered,
    /// Element holds keyboard focus.
    Focused,
    /// Element is being pressed / activated.
    Active,
    /// Element is non-interactive (absorbing — wins over every other
    /// state).
    Disabled,
}

impl ElementState {
    /// A stable lowercase style class (P6) each shell maps to its native
    /// focus-ring / hover / pressed / disabled styling.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Hovered => "hovered",
            Self::Focused => "focused",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

/// The interaction-state machine for a list of elements (G5b).
///
/// Tracks which element index is focused, hovered, and pressed, plus a set
/// of disabled indices. [`Self::state_of`] resolves these to a single
/// [`ElementState`] with precedence `Disabled > Active > Focused >
/// Hovered > Normal`. Pure + deterministic — every shell paints from this
/// one tested source, the pixels are the embedder's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interactions {
    focused: Option<usize>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    disabled: Vec<usize>,
}

impl Interactions {
    /// Give keyboard focus to element `i`.
    pub const fn focus(&mut self, i: usize) {
        self.focused = Some(i);
    }

    /// Drop keyboard focus.
    pub const fn blur(&mut self) {
        self.focused = None;
    }

    /// The currently focused element index, if any.
    #[must_use]
    pub const fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Set (or clear, with `None`) the hovered element.
    pub const fn hover(&mut self, i: Option<usize>) {
        self.hovered = i;
    }

    /// Begin pressing element `i` (becomes [`ElementState::Active`]).
    pub const fn press(&mut self, i: usize) {
        self.pressed = Some(i);
    }

    /// Release the press.
    pub const fn release(&mut self) {
        self.pressed = None;
    }

    /// Enable or disable element `i`.
    pub fn set_disabled(&mut self, i: usize, disabled: bool) {
        let present = self.disabled.iter().position(|&d| d == i);
        match (disabled, present) {
            (true, None) => self.disabled.push(i),
            (false, Some(p)) => {
                self.disabled.remove(p);
            }
            _ => {}
        }
    }

    /// Move focus to the next element (wrapping within `count`).
    pub fn focus_next(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.focused = Some(self.focused.map_or(0, |f| (f + 1) % count));
    }

    /// Move focus to the previous element (wrapping within `count`).
    pub fn focus_prev(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.focused = Some(self.focused.map_or(count - 1, |f| (f + count - 1) % count));
    }

    /// Resolve element `i`'s [`ElementState`] under the fixed precedence.
    #[must_use]
    pub fn state_of(&self, i: usize) -> ElementState {
        if self.disabled.contains(&i) {
            ElementState::Disabled
        } else if self.pressed == Some(i) {
            ElementState::Active
        } else if self.focused == Some(i) {
            ElementState::Focused
        } else if self.hovered == Some(i) {
            ElementState::Hovered
        } else {
            ElementState::Normal
        }
    }
}

/// The kind of a feedback item (G7): a severity, or a progress percentage
/// for a running long op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackKind {
    /// Neutral information.
    Info,
    /// A completed action.
    Success,
    /// A non-fatal caution.
    Warning,
    /// A failure.
    Error,
    /// A long op in progress, `0..=100`%.
    Progress(u8),
}

/// One entry in the [`Feedback`] queue (G7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackItem {
    /// Severity / progress.
    pub kind: FeedbackKind,
    /// Message (for progress, the operation label).
    pub text: String,
}

/// The async feedback surface (G7): a queue of typed notifications +
/// progress for long ops (sync / eval / llm).
///
/// Mutated as state — `notify` appends a severity message, `progress`
/// updates a labelled progress entry in place (so a running op reports
/// incrementally instead of stacking). [`Self::to_nodes`] renders every
/// item as a [`Node::Toast`] (G1c), so *every* shell already displays the
/// feedback with no per-shell notification code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Feedback {
    items: Vec<FeedbackItem>,
}

impl Feedback {
    /// Append a severity notification.
    pub fn notify(&mut self, level: ToastLevel, text: impl Into<String>) {
        let kind = match level {
            ToastLevel::Info => FeedbackKind::Info,
            ToastLevel::Success => FeedbackKind::Success,
            ToastLevel::Warning => FeedbackKind::Warning,
            ToastLevel::Error => FeedbackKind::Error,
        };
        self.items.push(FeedbackItem {
            kind,
            text: text.into(),
        });
    }

    /// Report progress for the op labelled `label` — updates the existing
    /// progress entry in place, or starts one.
    pub fn progress(&mut self, label: impl Into<String>, percent: u8) {
        let label = label.into();
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|i| i.text == label && matches!(i.kind, FeedbackKind::Progress(_)))
        {
            item.kind = FeedbackKind::Progress(percent);
        } else {
            self.items.push(FeedbackItem {
                kind: FeedbackKind::Progress(percent),
                text: label,
            });
        }
    }

    /// The current feedback items, oldest first.
    #[must_use]
    pub fn items(&self) -> &[FeedbackItem] {
        &self.items
    }

    /// Drop every item (e.g. once a batch of ops has settled).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Render the queue as [`Node::Toast`] nodes (G7) — progress becomes a
    /// polite `Info` toast with the label + percentage.
    #[must_use]
    pub fn to_nodes(&self) -> Vec<Node> {
        self.items
            .iter()
            .map(|i| match i.kind {
                FeedbackKind::Info => toast_node(ToastLevel::Info, i.text.clone()),
                FeedbackKind::Success => toast_node(ToastLevel::Success, i.text.clone()),
                FeedbackKind::Warning => toast_node(ToastLevel::Warning, i.text.clone()),
                FeedbackKind::Error => toast_node(ToastLevel::Error, i.text.clone()),
                FeedbackKind::Progress(p) => {
                    toast_node(ToastLevel::Info, format!("{} {p}%", i.text))
                }
            })
            .collect()
    }
}

/// Compose a [`Feedback`] queue onto a base [`ViewTree`](Node) as toast
/// nodes (P6).
///
/// Every shell already renders [`Node::Toast`] (G1c), so this is how
/// notifications + progress reach every window from the one shared queue. A
/// `Pane` gets the toasts appended to its children; any other node is
/// wrapped in a `Pane` with the toasts after it. Empty feedback returns the
/// base unchanged.
#[must_use]
pub fn with_feedback(base: Node, feedback: &Feedback) -> Node {
    let toasts = feedback.to_nodes();
    if toasts.is_empty() {
        return base;
    }
    match base {
        Node::Pane {
            title,
            mut children,
        } => {
            children.extend(toasts);
            Node::Pane { title, children }
        }
        other => {
            let mut children = vec![other];
            children.extend(toasts);
            Node::Pane {
                title: String::new(),
                children,
            }
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
    /// [`Node::Split`].
    Split,
    /// [`Node::Modal`].
    Modal,
    /// [`Node::Toast`].
    Toast,
}

impl Node {
    /// The semantic ARIA role for this node (V12a), so an embedder can
    /// emit screen-reader-navigable output. Derived per kind, like
    /// [`Self::kind`].
    #[must_use]
    pub const fn aria_role(&self) -> &'static str {
        match self {
            Self::Pane { .. } | Self::Widget { .. } => "region",
            Self::Rows { .. } => "list",
            Self::Detail { .. } | Self::Split { .. } => "group",
            Self::Modal { .. } => "dialog",
            Self::Toast { level, .. } => level.aria_role(),
            Self::Input { .. } => "textbox",
            Self::Palette { .. } => "listbox",
            Self::Hints { .. } => "status",
            Self::Text(_) => "note",
        }
    }

    /// The accessible label for this node, where it has a natural one
    /// (pane title, input label, widget name); `None` otherwise (V12a).
    #[must_use]
    pub fn aria_label(&self) -> Option<&str> {
        match self {
            Self::Pane { title, .. } | Self::Modal { title, .. } => Some(title),
            Self::Input { label, .. } => Some(label),
            Self::Widget { name, .. } => Some(name),
            _ => None,
        }
    }

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
            Self::Split { .. } => NodeKind::Split,
            Self::Modal { .. } => NodeKind::Modal,
            Self::Toast { .. } => NodeKind::Toast,
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
    NodeKind::Split,
    NodeKind::Modal,
    NodeKind::Toast,
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

/// Kinds the GTK4 renderer (`closure_shell_gtk::widget_tree`) handles —
/// all of them, exhaustively (G3: its `match` over `Node` is total, so a
/// new kind is a compile error there too).
pub const GTK_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// Kinds the Qt6/QML renderer (`closure_shell_qt::qml_view`) handles —
/// all of them, exhaustively (G4).
pub const QT_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

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
        ("GTK", GTK_NODE_KINDS),
        ("QT", QT_NODE_KINDS),
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
/// Palette commands as `(display, canonical, section, description)` (G6).
/// `section` groups them in the command palette; `description` is the
/// human one-liner shown beside the chord.
const PALETTE_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("next-file", "next-file", "Navigate", "Go to the next file"),
    (
        "prev-file",
        "prev-file",
        "Navigate",
        "Go to the previous file",
    ),
    ("open", "open-file", "Navigate", "Open the selected file"),
    ("capture", "capture-start", "Edit", "Capture a new entry"),
    (
        "add-sibling",
        "add-sibling",
        "Edit",
        "Add a sibling headline",
    ),
    ("rename", "rename", "Edit", "Rename the headline"),
    ("delete", "delete", "Edit", "Delete the headline"),
    ("cycle-mode", "cycle-mode", "Mode", "Switch the input mode"),
    (
        "fold",
        "toggle-fold",
        "Navigate",
        "Fold or unfold the selected subtree",
    ),
    ("quit", "quit", "App", "Quit closure"),
];

/// Section order for the command palette (G6); sections render in this
/// order, empty ones dropped.
const PALETTE_SECTIONS: &[&str] = &["Navigate", "Edit", "Mode", "App"];

/// A command entry in the [`command_palette`] (G6): a label + human
/// description + its actionable chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Display label.
    pub label: String,
    /// Human one-line description.
    pub description: String,
    /// The command + its chord.
    pub action: Action,
}

/// A titled group of palette entries (G6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSection {
    /// Section heading.
    pub title: String,
    /// Entries in fuzzy-ranked order.
    pub items: Vec<PaletteEntry>,
}

/// The polished command palette (G6).
///
/// Every command grouped into sections, fuzzy-filtered + ranked by
/// `query`, each entry carrying a description + its chord in `mode`. Empty
/// sections are dropped. One hermetic source every GUI renders.
/// M-x rule (I4): every command bound in the mode's keymap appears —
/// curated entries keep their sections/descriptions, the rest land in
/// a final "Command" section labelled by their canonical name.
#[must_use]
pub fn command_palette(query: &str, mode: closure_config::InputMode) -> Vec<PaletteSection> {
    let mut sections: Vec<PaletteSection> = PALETTE_SECTIONS
        .iter()
        .filter_map(|section| {
            let mut scored: Vec<(u32, PaletteEntry)> = PALETTE_COMMANDS
                .iter()
                .filter(|(.., sec, _)| sec == section)
                .filter_map(|(label, canonical, _, desc)| {
                    let score = if query.is_empty() {
                        Some(0)
                    } else {
                        closure_query::fuzzy_score(query, label)
                    }?;
                    let action = Action::new(mode, *canonical)?;
                    Some((
                        score,
                        PaletteEntry {
                            label: (*label).to_owned(),
                            description: (*desc).to_owned(),
                            action,
                        },
                    ))
                })
                .collect();
            if scored.is_empty() {
                return None;
            }
            scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
            Some(PaletteSection {
                title: (*section).to_owned(),
                items: scored.into_iter().map(|(_, e)| e).collect(),
            })
        })
        .collect();
    let curated: std::collections::BTreeSet<&str> =
        PALETTE_COMMANDS.iter().map(|(_, c, ..)| *c).collect();
    let mut rest: std::collections::BTreeSet<&str> = closure_input::mode_keymap(mode)
        .iter()
        .map(|(_, cmd)| *cmd)
        .filter(|cmd| !curated.contains(cmd))
        .collect();
    let mut scored: Vec<(u32, PaletteEntry)> = Vec::new();
    for cmd in std::mem::take(&mut rest) {
        let score = if query.is_empty() {
            Some(0)
        } else {
            closure_query::fuzzy_score(query, cmd)
        };
        if let Some(score) = score
            && let Some(action) = Action::new(mode, cmd)
        {
            scored.push((
                score,
                PaletteEntry {
                    label: cmd.to_owned(),
                    description: format!("Run {cmd}"),
                    action,
                },
            ));
        }
    }
    if !scored.is_empty() {
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        sections.push(PaletteSection {
            title: "Command".to_owned(),
            items: scored.into_iter().map(|(_, e)| e).collect(),
        });
    }
    sections
}

/// Serialise a [`command_palette`] to a deterministic text snapshot (G6) —
/// golden-testable, and the form a shell can render verbatim.
#[must_use]
pub fn serialize_palette(sections: &[PaletteSection]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for s in sections {
        let _ = writeln!(out, "SECTION {}", s.title);
        for e in &s.items {
            let _ = writeln!(
                out,
                "  [{}] {} — {}",
                e.action.chord(),
                e.label,
                e.description
            );
        }
    }
    out
}

/// One entry of the Notion-style "/" block menu.
///
/// The insertion is *org text*, not a private block model: whatever is
/// picked is what lands in the file, so a block inserted in the GUI is
/// a block Emacs reads (I1). `cursor` is where the caret goes
/// afterwards, as a char offset into `text` — inside the code block,
/// after the checkbox, between the link brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTemplate {
    /// Menu label.
    pub label: &'static str,
    /// The org text inserted at the cursor.
    pub text: &'static str,
    /// Caret position afterwards, in chars from the start of `text`.
    pub cursor: usize,
}

/// The block templates whose labels fuzzy-match `query`, best first;
/// an empty query lists all of them, and no match lists none.
#[must_use]
pub fn block_templates(query: &str) -> Vec<BlockTemplate> {
    // `cursor` counts chars, so each entry is written with its caret
    // target spelled out rather than a magic number.
    const fn t(label: &'static str, text: &'static str, cursor: usize) -> BlockTemplate {
        BlockTemplate {
            label,
            text,
            cursor,
        }
    }
    let all = [
        t("Heading", "** ", 3),
        t("To-do", "- [ ] ", 6),
        t("Done", "- [X] ", 6),
        t("Bullet", "- ", 2),
        t("Numbered", "1. ", 3),
        // Caret on the empty middle line of the block.
        t("Code", "#+BEGIN_SRC sh\n\n#+END_SRC", 15),
        t("Quote", "#+BEGIN_QUOTE\n\n#+END_QUOTE", 14),
        t("Example", "#+BEGIN_EXAMPLE\n\n#+END_EXAMPLE", 16),
        t("Table", "| ", 2),
        t("Link", "[[][]]", 2),
        t("Property", ":PROPERTIES:\n:KEY: value\n:END:", 14),
        t("Divider", "-----", 5),
    ];
    let mut scored: Vec<(u32, BlockTemplate)> = all
        .into_iter()
        .filter_map(|tpl| {
            let score = if query.is_empty() {
                Some(0)
            } else {
                closure_query::fuzzy_score(query, tpl.label)
            }?;
            Some((score, tpl))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, t)| t).collect()
}

/// What a right-click landed on, selecting which context menu to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTarget {
    /// An outline row: the structural commands.
    Row,
    /// The body editor or its preview: editing and code blocks.
    Body,
    /// A detail-pane field: the per-field edit commands.
    Detail,
}

/// The context menu for `target` in `mode`.
///
/// Derived from the mode's keymap, never hand-written per shell: every
/// entry carries the chord that actually runs it (I4), which is how
/// the mouse-only path stays a way to *discover* the keyboard one.
/// A command with no binding in `mode` is dropped rather than shown
/// with a blank chord — the menu shrinks instead of lying.
#[must_use]
pub fn context_menu(
    target: ContextTarget,
    mode: closure_config::InputMode,
) -> Vec<PaletteItemView> {
    let entries: &[(&str, &str)] = match target {
        ContextTarget::Row => &[
            ("Rename", "rename"),
            ("Cycle TODO", "toggle-todo"),
            ("Cycle priority", "cycle-priority"),
            ("Edit tags", "edit-tags"),
            ("Edit property", "edit-property"),
            ("Edit body", "edit-body"),
            ("Fold / unfold", "toggle-fold"),
            ("Promote", "promote"),
            ("Demote", "demote"),
            ("Move up", "move-subtree-up"),
            ("Move down", "move-subtree-down"),
            ("Add sibling", "add-sibling"),
            ("Backlinks", "backlinks"),
            ("Undo history", "undo-history"),
            ("Delete subtree", "delete"),
        ],
        ContextTarget::Body => &[
            ("Edit body", "edit-body"),
            ("Source blocks", "block-list"),
            ("Backlinks", "backlinks"),
            ("Undo", "undo"),
            ("Redo", "redo"),
        ],
        ContextTarget::Detail => &[
            ("Rename", "rename"),
            ("Cycle TODO", "toggle-todo"),
            ("Cycle priority", "cycle-priority"),
            ("Edit tags", "edit-tags"),
            ("Edit property", "edit-property"),
            ("Edit body", "edit-body"),
        ],
    };
    entries
        .iter()
        .filter_map(|(label, command)| {
            Action::new(mode, *command).map(|action| PaletteItemView {
                label: (*label).to_owned(),
                action,
            })
        })
        .collect()
}

/// The Notion-style slash command menu (G5c).
///
/// Every palette command whose name fuzzy-matches `query`, as actionable
/// [`PaletteItemView`]s carrying their chord in `mode` (empty query ⇒
/// all). Pure + deterministic — the "/" affordance as data, the same
/// command set every shell drives.
#[must_use]
pub fn slash_menu(query: &str, mode: closure_config::InputMode) -> Vec<PaletteItemView> {
    let mut scored: Vec<(u32, PaletteItemView)> = PALETTE_COMMANDS
        .iter()
        .filter_map(|(name, canonical, _, _)| {
            let score = if query.is_empty() {
                Some(0)
            } else {
                closure_query::fuzzy_score(query, name)
            }?;
            let action = Action::new(mode, *canonical)?;
            Some((
                score,
                PaletteItemView {
                    label: (*name).to_owned(),
                    action,
                },
            ))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, it)| it).collect()
}

/// The action behind the Notion block "+" insert affordance (G5c).
///
/// The `add-sibling` command in `mode`, carrying its chord (so the "+"
/// button shows its keybinding, V1). `None` if unbound.
#[must_use]
pub fn block_insert_action(mode: closure_config::InputMode) -> Option<Action> {
    Action::new(mode, "add-sibling")
}

/// A drag-to-reorder gesture (G5c) — a pure state machine.
///
/// [`Self::begin`] records the dragged index, [`Self::over`] the hover
/// target, and [`Self::drop`] yields `(from, to)` once (then resets). The
/// actual move is a registry command at the call site (I8); this only
/// tracks the gesture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DragReorder {
    from: Option<usize>,
    to: Option<usize>,
}

impl DragReorder {
    /// Begin dragging the element at `i`.
    pub const fn begin(&mut self, i: usize) {
        self.from = Some(i);
    }

    /// Update the hover target to `i`.
    pub const fn over(&mut self, i: usize) {
        self.to = Some(i);
    }

    /// The row the gesture picked up, while it is still in flight.
    ///
    /// A drag is unusable without an insertion indicator, and a shell
    /// can only paint one if it can read the gesture mid-flight —
    /// [`Self::drop`] answers once, at the end, and then resets.
    #[must_use]
    pub const fn source(&self) -> Option<usize> {
        self.from
    }

    /// The row the pointer is currently over, or `None` until it has
    /// moved off the source.
    #[must_use]
    pub const fn target(&self) -> Option<usize> {
        self.to
    }

    /// Abandon the gesture.
    pub const fn cancel(&mut self) {
        self.from = None;
        self.to = None;
    }

    /// Complete the gesture: `(from, to)` if a drag was in progress (the
    /// target defaults to the source when the pointer never moved), then
    /// reset. `None` if no drag began.
    pub const fn drop(&mut self) -> Option<(usize, usize)> {
        let result = match self.from {
            Some(from) => Some((from, if let Some(t) = self.to { t } else { from })),
            None => None,
        };
        self.from = None;
        self.to = None;
        result
    }
}

/// The index order after a drag-reorder move (G5c).
///
/// Moves the element at `from` to position `to` in a list of `len`
/// (remove + re-insert). Out-of-range `from`/`to` yields the identity
/// order — never a panic (I5).
#[must_use]
pub fn reorder_indices(len: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if from >= len || to >= len {
        return order;
    }
    let moved = order.remove(from);
    order.insert(to, moved);
    order
}

/// A typed key event from a shell's window (P1).
///
/// The named key, whether Ctrl is held, and the typed character (if
/// printable). Shells translate their native event into this and hand it
/// to [`App::dispatch`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyEvent {
    /// The key name (`"enter"`, `"escape"`, `"down"`, or the char as a
    /// string for a printable key).
    pub key: String,
    /// Whether Ctrl was held.
    pub ctrl: bool,
    /// The typed character, for printable keys.
    pub text: Option<char>,
}

impl KeyEvent {
    /// A key event from its parts.
    #[must_use]
    pub fn new(key: impl Into<String>, ctrl: bool, text: Option<char>) -> Self {
        Self {
            key: key.into(),
            ctrl,
            text,
        }
    }

    /// A named non-printable key (`enter`, `escape`, `down`, …).
    #[must_use]
    pub fn key(name: impl Into<String>) -> Self {
        Self::new(name, false, None)
    }

    /// A `Ctrl`-modified key (the chord leader the shells share).
    #[must_use]
    pub fn ctrl(name: impl Into<String>) -> Self {
        Self::new(name, true, None)
    }

    /// A printable typed character (`key` = the char, `text` = the char).
    #[must_use]
    pub fn char(c: char) -> Self {
        Self::new(c.to_string(), false, Some(c))
    }
}

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
            .filter_map(|(name, canonical, _, _)| {
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

    /// Fold or unfold the selected subtree (`C-f` / palette `fold`).
    ///
    /// Writes the org-standard `:VISIBILITY:` property through the
    /// registry (I8, undoable I3), so the fold persists between program
    /// runs and is honoured by Emacs org-mode too.
    pub fn toggle_fold(&mut self, shell: &mut Shell) {
        let rows = self.rows(shell);
        let Some(row) = rows.get(self.selected) else {
            return;
        };
        let title = row.title.clone();
        let bid = closure_core::BlockId::from_existing(&row.id);
        match toggle_visibility(shell, &bid) {
            Some(true) => self.set_status(&format!("folded: {title}")),
            Some(false) => self.set_status(&format!("unfolded: {title}")),
            None => self.set_status(&format!("fold failed: {title}")),
        }
        self.selected = self.selected.min(self.rows(shell).len().saturating_sub(1));
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

    /// Property key buffer (read) for the `PropertyEdit` surface.
    #[must_use]
    pub fn property_key(&self) -> &str {
        &self.prop_key
    }

    /// Property value buffer (read) for the `PropertyEdit` surface.
    #[must_use]
    pub fn property_value(&self) -> &str {
        &self.prop_value
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
            // Fold-aware outline walk: a `:VISIBILITY: folded` headline
            // hides its descendants — but only in the outline listing; a
            // live query searches into folds (like org isearch).
            let mut hide_below: Option<u8> = None;
            for h in doc.all_headlines() {
                if self.query.is_empty() {
                    if let Some(limit) = hide_below {
                        if h.level() > limit {
                            continue;
                        }
                        hide_below = None;
                    }
                    if headline_is_folded(h) {
                        hide_below = Some(h.level());
                    }
                }
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
                    .map(|r| {
                        let icon = r.todo.as_deref().map(|t| todo_glyph(t).to_owned());
                        let badges = if row_is_folded(shell, &r.id) {
                            vec![FOLD_MARKER.to_owned()]
                        } else {
                            Vec::new()
                        };
                        RowView::new(r.id, r.title, r.level, r.todo)
                            .with_icon(icon)
                            .with_badges(badges)
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
                    .filter_map(|(label, canonical, _, _)| {
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
    /// The unified input→state→view step (P1): apply `event` via the
    /// mode-aware [`Self::on_key`], then return the fresh
    /// [`ViewTree`](Node). The ONE call every shell's window delegates to,
    /// so editing behaviour is the tested core, not per-shell key logic.
    pub fn dispatch(&mut self, shell: &mut Shell, event: &KeyEvent) -> Node {
        self.on_key(shell, &event.key, event.ctrl, event.text);
        self.view(shell)
    }

    /// Apply one key in the active mode (the low-level handler behind
    /// [`Self::dispatch`]): routes to the per-mode key handler, mutating
    /// through [`Shell`] (I8). Most shells call `dispatch` instead, which
    /// also returns the refreshed view.
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
            "fold" => self.toggle_fold(shell),
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
            "f" if ctrl => self.toggle_fold(shell),
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
    /// Renaming the selected headline (single-line title buffer,
    /// prefilled with the current title).
    Rename,
    /// Typing the title of a new sibling inserted after the selected row.
    AddSibling,
    /// Fuzzy command palette over the shared [`command_palette`] source.
    Palette,
    /// Read-only undo-tree of the selected row's document (I3).
    UndoHistory,
    /// Every headline in the selected row's file, flat.
    Headlines,
    /// Notion-style database table over the whole vault.
    DbView,
    /// Full-text search over headline *bodies* (the outline search
    /// only sees titles).
    BodySearch,
    /// Captured network flows with their allow/block rules (X3).
    Sniffer,
    /// CRDT field conflicts awaiting an ours/theirs decision.
    Conflicts,
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
    Rename,
    AddSibling,
}

/// The body editor's vim-style mode (org-edit-special surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Typing inserts at the cursor; `Esc` drops to [`Self::Normal`].
    Insert,
    /// `h`/`j`/`k`/`l` navigate, `i`/`a`/`o` insert, `x` deletes,
    /// `v` selects, `dd`/`yy`/`p` cut/copy/paste lines, `Esc` cancels
    /// the edit.
    Normal,
    /// Charwise selection from an anchor (`v`): motions extend, `y`
    /// yanks, `d`/`x` delete, `Esc` returns to Normal.
    Visual,
    /// Linewise selection from an anchor line (V): motions extend by
    /// whole lines, y yanks them, d/x delete them, Esc returns.
    VisualLine,
}

/// A modal multi-line text editor with a real cursor — the state
/// behind the org-edit-special surface.
///
/// Pure and unicode-safe (the cursor is a byte offset kept on a `char`
/// boundary); a shell paints `text()` + `cursor_line_col()` and feeds
/// keys through [`ModalApp`].
#[derive(Debug, Clone)]
pub struct BodyEditor {
    buf: String,
    cursor: usize,
    mode: EditorMode,
    /// Visual-mode selection anchor (byte offset).
    anchor: usize,
    /// The yank/kill register shared by vim (`y`/`d`/`p`) and the
    /// readline chords (`C-k`/`C-u`/`C-w`/`C-y`).
    register: String,
    /// Whether the register holds whole lines (`dd`/`yy` → `p` pastes
    /// below the current line).
    linewise: bool,
    /// First stroke of a two-stroke Normal command (`d` of `dd`).
    pending: Option<char>,
    /// Pending vim count in Normal/Visual modes (0 = none).
    count: usize,
    /// Editor-local undo snapshots (buffer, cursor), newest last.
    undo_stack: Vec<(String, usize)>,
    /// Redo snapshots cleared by any fresh edit.
    redo_stack: Vec<(String, usize)>,
    /// Armed on every INSERT entry: the first buffer-changing INSERT
    /// edit takes one checkpoint, so the whole burst undoes as a unit
    /// (vim rule, G4).
    insert_armed: bool,
}

impl Default for BodyEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyEditor {
    /// Empty editor in Insert mode.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            mode: EditorMode::Insert,
            anchor: 0,
            register: String::new(),
            linewise: false,
            pending: None,
            count: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            insert_armed: false,
        }
    }

    /// Load `text`, cursor at the end, Insert mode (the edit-body flow).
    pub fn load(&mut self, text: String) {
        self.cursor = text.len();
        self.buf = text;
        self.mode = EditorMode::Insert;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.insert_armed = true;
    }

    /// The buffer contents.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.buf
    }

    /// The vim-style mode.
    #[must_use]
    pub const fn mode(&self) -> EditorMode {
        self.mode
    }

    /// The cursor as a byte offset into the buffer — what
    /// [`Self::replace_to_cursor`] addresses.
    #[must_use]
    pub const fn cursor_byte(&self) -> usize {
        self.cursor
    }

    /// Park the cursor at byte offset `byte`, clamped to the buffer and
    /// to a char boundary.
    ///
    /// Unlike the motions, this crosses lines: an inserted multi-line
    /// template needs the caret placed *inside* it, which `left`/`up`
    /// cannot express because they deliberately stop at newlines.
    pub fn set_cursor_byte(&mut self, byte: usize) {
        let mut at = byte.min(self.buf.len());
        while at > 0 && !self.buf.is_char_boundary(at) {
            at -= 1;
        }
        self.cursor = at;
    }

    /// Cursor as `(line, column)` — both 0-based, column in chars.
    #[must_use]
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let before = &self.buf[..self.cursor];
        let line = before.matches('\n').count();
        let col = before
            .rsplit_once('\n')
            .map_or(before, |(_, tail)| tail)
            .chars()
            .count();
        (line, col)
    }

    /// Clear buffer + cursor (cancelling an edit).
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.mode = EditorMode::Insert;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.insert_armed = true;
    }

    /// Switch to Normal (from Insert `Esc`).
    pub fn to_normal(&mut self) {
        self.mode = EditorMode::Normal;
        // vim rule: leaving Insert steps the cursor back onto the last
        // typed char (clamped at the line start by left()).
        self.left();
    }

    /// Switch to Insert at the cursor (`i`), arming the burst
    /// checkpoint (G4).
    pub const fn to_insert(&mut self) {
        self.mode = EditorMode::Insert;
        self.insert_armed = true;
    }

    /// G4: the first buffer-changing edit of an INSERT burst records
    /// one pre-edit snapshot, so `Esc` + `u` undoes the whole burst.
    /// A no-op outside Insert mode or once the burst has checkpointed.
    fn insert_guard(&mut self) {
        if self.mode == EditorMode::Insert && self.insert_armed {
            self.checkpoint();
            self.insert_armed = false;
        }
    }

    /// Insert `c` at the cursor.
    pub fn insert_char(&mut self, c: char) {
        self.insert_guard();
        self.buf.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert `s` at the cursor, cursor after it.
    pub fn insert_str(&mut self, s: &str) {
        self.insert_guard();
        self.buf.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the char before the cursor (Insert Backspace).
    pub fn backspace(&mut self) {
        if let Some((i, _)) = self.buf[..self.cursor].char_indices().next_back() {
            self.insert_guard();
            self.buf.remove(i);
            self.cursor = i;
        }
    }

    /// Delete the char under the cursor (Normal `x`).
    pub fn delete_at(&mut self) {
        if self.cursor < self.buf.len() {
            self.insert_guard();
            self.buf.remove(self.cursor);
        }
    }

    /// Move one char left (clamped to the line start).
    pub fn left(&mut self) {
        if let Some((i, c)) = self.buf[..self.cursor].char_indices().next_back()
            && c != '\n'
        {
            self.cursor = i;
        }
    }

    /// Move one char right (clamped to the line end).
    pub fn right(&mut self) {
        if let Some(c) = self.buf[self.cursor..].chars().next()
            && c != '\n'
        {
            self.cursor += c.len_utf8();
        }
    }

    /// Move to the start of the current line (`0`).
    pub fn line_home(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }

    /// Move to the end of the current line (`$`).
    pub fn line_end_motion(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }

    /// Move up one line, column clamped.
    pub fn up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line > 0 {
            self.goto_line_col(line - 1, col);
        }
    }

    /// Move down one line, column clamped.
    pub fn down(&mut self) {
        let (line, col) = self.cursor_line_col();
        self.goto_line_col(line + 1, col);
    }

    /// Open a new line below the current one and enter Insert (`o`).
    /// The caller (`o`) checkpoints before the open, so the burst
    /// checkpoint stays disarmed — `o` plus typing undoes as one unit.
    pub fn open_below(&mut self) {
        self.cursor = self.line_end(self.cursor);
        self.buf.insert(self.cursor, '\n');
        self.cursor += 1;
        self.mode = EditorMode::Insert;
        self.insert_armed = false;
    }

    /// Byte offset of the start of the line containing `pos`.
    fn line_start(&self, pos: usize) -> usize {
        self.buf[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    /// Byte offset of the end (before `\n`) of the line containing `pos`.
    fn line_end(&self, pos: usize) -> usize {
        self.buf[pos..]
            .find('\n')
            .map_or(self.buf.len(), |i| pos + i)
    }

    /// Place the cursor at `line`/`col` (both clamped — a line past the
    /// end lands on the last line, the mouse-click rule).
    fn goto_line_col(&mut self, line: usize, col: usize) {
        let starts: Vec<usize> = self
            .buf
            .split_inclusive('\n')
            .scan(0usize, |acc, l| {
                let s = *acc;
                *acc += l.len();
                Some(s)
            })
            .collect();
        let Some(&start) = starts.get(line.min(starts.len().saturating_sub(1))) else {
            return;
        };
        let end = self.line_end(start);
        let mut pos = start;
        for c in self.buf[start..end].chars().take(col) {
            pos += c.len_utf8();
        }
        self.cursor = pos;
    }

    /// Select the whitespace-delimited word containing the cursor
    /// (double-click): anchor at the word start, Visual mode, cursor on
    /// its last char. Whitespace under the cursor selects nothing.
    pub fn select_word_at_cursor(&mut self) {
        let positions: Vec<(usize, char)> = self.buf.char_indices().collect();
        let Some(mut i) = positions.iter().position(|&(off, _)| off == self.cursor) else {
            return;
        };
        if positions[i].1.is_whitespace() {
            return;
        }
        while i > 0 && !positions[i - 1].1.is_whitespace() {
            i -= 1;
        }
        let mut j = i;
        while j + 1 < positions.len() && !positions[j + 1].1.is_whitespace() {
            j += 1;
        }
        self.anchor = positions[i].0;
        self.cursor = positions[j].0;
        self.mode = EditorMode::Visual;
    }

    /// Mouse drag extends a charwise Visual selection: on the first drag
    /// event that actually moves the cursor, the pre-drag cursor becomes
    /// the anchor and the mode switches to Visual; every drag event then
    /// moves the cursor to the clamped line and col, exactly like a
    /// click does. A drag that stays on the clicked cell (a click's
    /// micro-movement) leaves the mode untouched.
    pub fn drag_to(&mut self, line: usize, col: usize) {
        let from = self.cursor;
        self.goto_line_col(line, col);
        if !matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) && self.cursor != from
        {
            self.anchor = from;
            self.mode = EditorMode::Visual;
        }
    }

    /// Move to the start of the next word (simple rule, not full vim).
    fn word_forward(&mut self) {
        let positions: Vec<(usize, char)> = self.buf.char_indices().collect();
        let Some(mut i) = positions.iter().position(|&(off, _)| off == self.cursor) else {
            return;
        };
        // Skip the current word (if the cursor sits on one), then the
        // whitespace run (newlines included); clamp at the buffer end.
        while i < positions.len() && !positions[i].1.is_whitespace() {
            i += 1;
        }
        while i < positions.len() && positions[i].1.is_whitespace() {
            i += 1;
        }
        if i < positions.len() {
            self.cursor = positions[i].0;
        }
    }

    /// Move to the start of the previous word (simple rule).
    fn word_backward(&mut self) {
        let positions: Vec<(usize, char)> = self.buf.char_indices().collect();
        let mut pos = if self.cursor == self.buf.len() {
            positions.len()
        } else {
            match positions.iter().position(|&(i, _)| i == self.cursor) {
                Some(p) => p,
                None => return,
            }
        };
        // Skip whitespace immediately before the cursor, then walk back
        // to the start of the word (char before is whitespace or start).
        while pos > 0 && positions[pos - 1].1.is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !positions[pos - 1].1.is_whitespace() {
            pos -= 1;
        }
        self.cursor = if pos < positions.len() {
            positions[pos].0
        } else {
            0
        };
    }

    /// The text of the line the cursor is on.
    #[must_use]
    pub fn current_line(&self) -> &str {
        &self.buf[self.line_start(self.cursor)..self.line_end(self.cursor)]
    }

    /// Byte offset where the word containing the cursor starts (the
    /// dabbrev prefix start; word chars are alphanumeric plus
    /// `_`/`#`/`+`/`:` so org keywords like `#+BEGIN_SRC` complete).
    #[must_use]
    pub fn word_start(&self) -> usize {
        let mut start = self.cursor;
        for (i, c) in self.buf[..self.cursor].char_indices().rev() {
            if c.is_alphanumeric() || matches!(c, '_' | '#' | '+' | ':') {
                start = i;
            } else {
                break;
            }
        }
        start
    }

    /// The word fragment between [`Self::word_start`] and the cursor —
    /// the dabbrev completion prefix.
    #[must_use]
    pub fn word_prefix(&self) -> &str {
        &self.buf[self.word_start()..self.cursor]
    }

    /// Replace `start..cursor` with `text`, cursor after it (the
    /// completion-accept edit). `start` must be a char boundary at or
    /// before the cursor.
    pub fn replace_to_cursor(&mut self, start: usize, text: &str) {
        if start <= self.cursor && self.buf.is_char_boundary(start) {
            self.insert_guard();
            self.buf.replace_range(start..self.cursor, text);
            self.cursor = start + text.len();
        }
    }

    /// Undo the last editor-local edit (Normal u).
    pub fn undo_local(&mut self) {
        if let Some((buf, cur)) = self.undo_stack.pop() {
            self.redo_stack.push((self.buf.clone(), self.cursor));
            self.buf = buf;
            self.cursor = cur;
        }
    }

    /// Redo the last editor-local undo (Normal C-r).
    pub fn redo_local(&mut self) {
        if let Some((buf, cur)) = self.redo_stack.pop() {
            self.undo_stack.push((self.buf.clone(), self.cursor));
            self.buf = buf;
            self.cursor = cur;
        }
    }

    /// One Normal/Visual-mode key (motions, `i`/`a`/`o`/`x`, `v`,
    /// `dd`/`yy`/`p`, Visual `y`/`d`). `Esc` leaves Visual or clears a
    /// pending stroke; the *caller* cancels the edit on `Esc` when
    /// [`Self::pending_stroke`] is clear and the mode is Normal.
    // One arm per vim key: splitting the vocabulary would hide the
    // mode dispatch (same precedent as `run_command`).
    #[allow(clippy::too_many_lines)]
    pub fn modal_key(&mut self, key: &str) {
        if key.len() == 1
            && let Some(d) = key.chars().next().and_then(|c| c.to_digit(10))
        {
            let d = usize::try_from(d).unwrap_or(9);
            if !(d == 0 && self.count == 0) {
                self.count = self.count * 10 + d;
                return;
            }
        }
        // Two-stroke commands first (`dd` / `yy`).
        if self.mode == EditorMode::Normal {
            match (self.pending.take(), key) {
                (Some('d'), "d") => {
                    self.checkpoint();
                    let n = self.take_count();
                    self.delete_line(n);
                    return;
                }
                (Some('y'), "y") => {
                    let n = self.take_count();
                    self.yank_line(n);
                    return;
                }
                (None, "d") => {
                    self.pending = Some('d');
                    return;
                }
                (None, "y") => {
                    self.pending = Some('y');
                    return;
                }
                _ => {}
            }
        }
        match key {
            "h" | "left" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.left();
                }
            }
            "l" | "right" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.right();
                }
            }
            "j" | "down" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.down();
                }
            }
            "k" | "up" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.up();
                }
            }
            "0" => self.line_home(),
            "$" => self.line_end_motion(),
            "w" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.word_forward();
                }
            }
            "b" => {
                let n = self.take_count();
                for _ in 0..n {
                    self.word_backward();
                }
            }
            "i" => {
                self.count = 0;
                self.to_insert();
            }
            "a" => {
                self.count = 0;
                self.right();
                self.to_insert();
            }
            "o" => {
                self.count = 0;
                self.checkpoint();
                self.open_below();
            }
            "v" => {
                self.count = 0;
                self.anchor = self.cursor;
                self.mode = EditorMode::Visual;
            }
            "V" => {
                self.count = 0;
                self.anchor = self.cursor;
                self.mode = EditorMode::VisualLine;
            }
            "escape" if self.mode == EditorMode::Normal && self.count > 0 => self.count = 0,
            "escape" if matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) => {
                self.mode = EditorMode::Normal;
            }
            "y" if self.mode == EditorMode::VisualLine => {
                let lo = self.line_start(self.anchor.min(self.cursor));
                let hi_line_end = self.line_end(self.anchor.max(self.cursor));
                let hi = if hi_line_end < self.buf.len() {
                    hi_line_end + 1
                } else {
                    hi_line_end
                };
                let mut text = self.buf[lo..hi].to_owned();
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                self.register = text;
                self.linewise = true;
                self.cursor = lo;
                self.mode = EditorMode::Normal;
            }
            "d" | "x" if self.mode == EditorMode::VisualLine => {
                self.checkpoint();
                let lo = self.line_start(self.anchor.min(self.cursor));
                let hi_line_end = self.line_end(self.anchor.max(self.cursor));
                let hi = if hi_line_end < self.buf.len() {
                    hi_line_end + 1
                } else {
                    hi_line_end
                };
                let mut text = self.buf[lo..hi].to_owned();
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                self.register = text;
                self.linewise = true;
                self.buf.replace_range(lo..hi, "");
                self.cursor = if lo > 0 { self.line_start(lo - 1) } else { 0 };
                self.mode = EditorMode::Normal;
            }
            "y" if self.mode == EditorMode::Visual => {
                let (lo, hi) = self.selection();
                self.register = self.buf[lo..hi].to_owned();
                self.linewise = false;
                self.cursor = lo;
                self.mode = EditorMode::Normal;
            }
            "d" | "x" if self.mode == EditorMode::Visual => {
                self.checkpoint();
                let (lo, hi) = self.selection();
                self.register = self.buf[lo..hi].to_owned();
                self.linewise = false;
                self.buf.replace_range(lo..hi, "");
                self.cursor = lo;
                self.mode = EditorMode::Normal;
            }
            "u" => self.undo_local(),
            "x" => {
                self.checkpoint();
                let n = self.take_count();
                if n == 1 {
                    self.delete_at();
                } else {
                    let start = self.cursor;
                    let line_end = self.line_end(start);
                    let mut end = start;
                    for (i, ch) in self.buf[start..].char_indices().take(n) {
                        let pos = start + i + ch.len_utf8();
                        if pos > line_end {
                            break;
                        }
                        end = pos;
                    }
                    if end > start {
                        self.register = self.buf[start..end].to_owned();
                        self.linewise = false;
                        self.buf.replace_range(start..end, "");
                    }
                }
            }
            "p" => {
                self.checkpoint();
                let n = self.take_count();
                for _ in 0..n {
                    self.paste();
                }
            }
            _ => {}
        }
    }

    /// The pending first stroke of a two-stroke command, if any.
    #[must_use]
    pub const fn pending_stroke(&self) -> Option<char> {
        self.pending
    }

    /// The pending vim count (0 = none) - the caller's cancel guard.
    #[must_use]
    pub const fn pending_count(&self) -> usize {
        self.count
    }

    fn take_count(&mut self) -> usize {
        let n = self.count.max(1);
        self.count = 0;
        n
    }

    /// Record the current state before a mutating edit (bounded at 50).
    fn checkpoint(&mut self) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((self.buf.clone(), self.cursor));
        self.redo_stack.clear();
    }

    /// Selection range for the renderer, or None outside visual modes.
    ///
    /// Returns the charwise selection range in Visual and the line range
    /// from `line_start` of min to `line_end` of max in `VisualLine`.
    #[must_use]
    pub fn visual_selection(&self) -> Option<(usize, usize)> {
        if self.mode == EditorMode::Visual {
            Some(self.selection())
        } else if self.mode == EditorMode::VisualLine {
            let lo = self.line_start(self.anchor.min(self.cursor));
            let hi = self.line_end(self.anchor.max(self.cursor));
            Some((lo, hi))
        } else {
            None
        }
    }

    /// The inclusive Visual selection as an exclusive byte range.
    fn selection(&self) -> (usize, usize) {
        let (lo, hi) = if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        let end = self.buf[hi..]
            .chars()
            .next()
            .filter(|c| *c != '\n')
            .map_or(hi, |c| hi + c.len_utf8());
        (lo, end)
    }

    /// `yy`: copy the current line (linewise register).
    pub fn yank_line(&mut self, n: usize) {
        let n = n.max(1);
        let lo = self.line_start(self.cursor);
        let mut hi = lo;
        for _ in 0..n {
            let e = self.line_end(hi);
            hi = e;
            if hi < self.buf.len() {
                hi += 1;
            } else {
                break;
            }
        }
        let mut text = self.buf[lo..hi].to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = text;
        self.linewise = true;
    }

    /// `dd`: cut the current line (linewise register).
    pub fn delete_line(&mut self, n: usize) {
        let n = n.max(1);
        let lo = self.line_start(self.cursor);
        let mut hi = lo;
        for _ in 0..n {
            let e = self.line_end(hi);
            hi = e;
            if hi < self.buf.len() {
                hi += 1;
            } else {
                break;
            }
        }
        let mut text = self.buf[lo..hi].to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = text;
        self.linewise = true;
        // Deleting through the last (newline-less) line eats the
        // preceding newline so no dangling terminator remains.
        let lo_cut = if hi >= self.buf.len() && !self.buf[lo..hi].ends_with('\n') && lo > 0 {
            lo - 1
        } else {
            lo
        };
        self.buf.replace_range(lo_cut..hi, "");
        self.cursor = if lo_cut > 0 {
            self.line_start(lo_cut - 1)
        } else {
            0
        };
    }

    /// `p`: paste the register — linewise below the current line,
    /// charwise after the cursor.
    pub fn paste(&mut self) {
        if self.register.is_empty() {
            return;
        }
        if self.linewise {
            let end = self.line_end(self.cursor);
            let text = format!("\n{}", self.register.trim_end_matches('\n'));
            self.buf.insert_str(end, &text);
            self.cursor = end + 1;
        } else {
            let pos = self.buf[self.cursor..]
                .chars()
                .next()
                .filter(|c| *c != '\n')
                .map_or(self.cursor, |c| self.cursor + c.len_utf8());
            let text = self.register.clone();
            self.buf.insert_str(pos, &text);
            self.cursor = pos;
        }
    }

    /// Readline `C-k`: kill from the cursor to the end of the line
    /// into the register.
    pub fn kill_rest_of_line(&mut self) {
        let end = self.line_end(self.cursor);
        if end > self.cursor {
            self.insert_guard();
            self.register = self.buf[self.cursor..end].to_owned();
            self.linewise = false;
            self.buf.replace_range(self.cursor..end, "");
        }
    }

    /// Readline `C-u`: kill from the line start to the cursor into the
    /// register.
    pub fn kill_to_line_start(&mut self) {
        let start = self.line_start(self.cursor);
        if start < self.cursor {
            self.insert_guard();
            self.register = self.buf[start..self.cursor].to_owned();
            self.linewise = false;
            self.buf.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    /// Readline `C-w`: delete the word (plus trailing spaces) before
    /// the cursor into the register.
    pub fn delete_word_back(&mut self) {
        let line_start = self.line_start(self.cursor);
        let s = &self.buf[line_start..self.cursor];
        let trimmed = s.trim_end_matches(' ');
        let word = trimmed.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
        let start = line_start + word.len();
        if start < self.cursor {
            self.insert_guard();
            self.register = self.buf[start..self.cursor].to_owned();
            self.linewise = false;
            self.buf.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    /// Readline `C-y`: insert the register at the cursor.
    pub fn yank_insert(&mut self) {
        let text = self.register.clone();
        self.insert_str(&text);
    }

    /// TAB in Insert mode: org-tempo expansion when the current line is
    /// a template trigger (`<s` → `#+BEGIN_SRC …`, like Emacs
    /// org-tempo), otherwise a two-space soft indent. After `<s` the
    /// cursor sits at the end of the `#+BEGIN_SRC ` line so the
    /// language can be typed immediately.
    pub fn tempo_expand_or_indent(&mut self) {
        let template: Option<(&str, &str)> = match self.current_line().trim() {
            "<s" => Some(("#+BEGIN_SRC ", "#+END_SRC")),
            "<e" => Some(("#+BEGIN_EXAMPLE", "#+END_EXAMPLE")),
            "<q" => Some(("#+BEGIN_QUOTE", "#+END_QUOTE")),
            "<c" => Some(("#+BEGIN_CENTER", "#+END_CENTER")),
            "<C" => Some(("#+BEGIN_COMMENT", "#+END_COMMENT")),
            "<v" => Some(("#+BEGIN_VERSE", "#+END_VERSE")),
            _ => None,
        };
        if let Some((begin, end)) = template {
            let start = self.line_start(self.cursor);
            let stop = self.line_end(self.cursor);
            self.buf
                .replace_range(start..stop, &format!("{begin}\n\n{end}"));
            self.cursor = start + begin.len();
        } else {
            self.insert_str("  ");
        }
    }
}

/// Org keywords always offered by the body-editor completion, beside
/// the dabbrev words mined from the vault.
const ORG_COMPLETION_KEYWORDS: &[&str] = &[
    "TODO",
    "DONE",
    "NEXT",
    "WAIT",
    "CANCELLED",
    "SCHEDULED:",
    "DEADLINE:",
    ":PROPERTIES:",
    ":END:",
    "#+BEGIN_SRC",
    "#+END_SRC",
    "#+BEGIN_QUOTE",
    "#+END_QUOTE",
    "#+TITLE:",
    "#+FILETAGS:",
];

/// Fuzzy-ranked completion candidates for `prefix`.
///
/// Org keywords and vault words (≥ 3 chars) that fuzzy-match by
/// subsequence ([`closure_query::fuzzy_score`]) and differ from the
/// prefix; ranked score-descending, keywords first on ties, then by
/// name — deduped after sorting.
#[must_use]
pub fn body_completions(prefix: &str, vault: &closure_store::Vault) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<(u32, bool, String)> = Vec::new();
    for &k in ORG_COMPLETION_KEYWORDS {
        if k != prefix
            && let Some(score) = closure_query::fuzzy_score(prefix, k)
        {
            entries.push((score, false, k.to_owned()));
        }
    }
    for (_p, doc) in vault.iter() {
        for word in doc
            .source()
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        {
            if word.len() >= 3
                && word != prefix
                && let Some(score) = closure_query::fuzzy_score(prefix, word)
            {
                entries.push((score, true, word.to_owned()));
            }
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for (_score, _is_word, name) in entries {
        if seen.insert(name.clone()) {
            result.push(name);
        }
    }
    result
}

/// A live completion cycle in the body editor: the prefix start, the
/// candidate list, and the currently-applied index.
#[derive(Debug, Clone)]
struct CompletionSession {
    start: usize,
    items: Vec<String>,
    /// Applied candidate index; `None` while the popup only shows
    /// candidates (auto-popup) and nothing replaced the prefix yet.
    ix: Option<usize>,
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
    body: BodyEditor,
    completion: Option<CompletionSession>,
    edit_target: Option<String>,
    /// Headline id whose backlinks the Backlinks surface is showing.
    link_target: Option<String>,
    /// Target id + single-line buffer for the TagsEdit/PropertyEdit
    /// surfaces (tags: space-separated; property: `key value`).
    field_target: Option<String>,
    field_buf: String,
    /// Cursor into [`Self::palette_entries`] while the Palette is open.
    palette_cursor: usize,
    pending: Vec<String>,
    status: String,
    quit: bool,
    /// Explicit wheel-scroll viewport offset; None = follow selection.
    scroll_override: Option<usize>,
    /// Body-editor wheel viewport `(start, cursor_line_when_set)`; the
    /// override self-clears when the cursor line changes (G5).
    body_scroll: Option<(usize, usize)>,
    /// Cursor row inside the `UndoHistory` pane (Q2-U3).
    hist_cursor: usize,
    /// The Notion "/" block menu's query while it is open, and its
    /// cursor row. `None` when closed.
    slash: Option<(String, usize)>,
    /// The sniffer surface's captured flows and rules (X3).
    sniffer: SnifferApp,
    /// The conflict surface's pending CRDT decisions.
    conflicts: ConflictApp,
    /// Whether the LLM may read the *rendered* view (V3b).
    ///
    /// Off until explicitly granted, and revocable at any time —
    /// `toggle-llm-render` is the live toggle, bound in every keymap so
    /// it shows up in which-key. Mirrors
    /// `closure_llm::LlmPermissions::toggle_render`; a session created
    /// from this app takes its render grant from here.
    llm_render: bool,
    /// Memoised outline rows — see [`ModalApp::rows`]. Interior
    /// mutability because `rows` is a `&self` query on the render
    /// path; the memo is exact, guarded by the vault revision and the
    /// active filter, so it can never serve a stale list.
    row_memo: std::cell::RefCell<Option<RowMemo>>,
    /// How many full vault walks the memo has paid for. Observability
    /// for the render budget (`rows_recomputes`), asserted in tests.
    row_recomputes: std::cell::Cell<u64>,
}

/// A filled outline-row memo together with the key it is valid under.
#[derive(Debug, Clone)]
struct RowMemo {
    /// Vault revision the rows were derived from.
    revision: u64,
    /// Active fuzzy filter (empty outside the Search surface).
    filter: String,
    /// The derived rows, shared so handing them out costs a refcount
    /// bump rather than a deep clone of every headline string.
    rows: std::sync::Arc<Vec<Row>>,
}

impl ModalApp {
    /// New modal app in the given editing mode, Browse surface.
    #[must_use]
    pub fn new(mode: InputMode) -> Self {
        Self {
            slash: None,
            sniffer: SnifferApp::new(),
            conflicts: ConflictApp::new(Vec::new(), mode),
            llm_render: false,
            mode,
            surface: ModalSurface::Browse,
            selected: 0,
            query: String::new(),
            capture_buf: String::new(),
            body: BodyEditor::new(),
            completion: None,
            edit_target: None,
            link_target: None,
            field_target: None,
            field_buf: String::new(),
            palette_cursor: 0,
            pending: Vec::new(),
            status: String::new(),
            quit: false,
            scroll_override: None,
            body_scroll: None,
            hist_cursor: 0,
            row_memo: std::cell::RefCell::new(None),
            row_recomputes: std::cell::Cell::new(0),
        }
    }

    /// Run `command` directly — the mouse path: a clicked which-key
    /// chip or palette row dispatches the SAME command a chord would
    /// (I8; no shell-private verbs). Key handling resolves chords to
    /// exactly this entry point.
    pub fn run(&mut self, shell: &mut Shell, command: &str) {
        self.run_command(shell, command);
    }

    /// Which-key items for the active mode, as structured
    /// `(chord, command)` pairs a GUI renders as clickable chips —
    /// sourced from [`closure_input::mode_keymap`] (I4), never a
    /// hand-maintained list.
    #[must_use]
    pub fn hint_items(&self) -> Vec<(String, String)> {
        closure_input::mode_keymap(self.mode)
            .iter()
            .map(|(c, cmd)| ((*c).to_owned(), (*cmd).to_owned()))
            .collect()
    }

    /// Which-key data grouped for the Doom-style popup: every keymap
    /// pair once, grouped by its palette section ("Command" when
    /// uncurated), groups in section order, entries chord-sorted (I4).
    #[must_use]
    pub fn which_key_groups(&self) -> Vec<(String, Vec<(String, String)>)> {
        let section_of = |cmd: &str| -> &str {
            PALETTE_COMMANDS
                .iter()
                .find(|(_, canonical, ..)| *canonical == cmd)
                .map_or("Command", |(.., sec, _)| sec)
        };
        let mut groups: Vec<(String, Vec<(String, String)>)> = PALETTE_SECTIONS
            .iter()
            .chain(std::iter::once(&"Command"))
            .map(|s| ((*s).to_owned(), Vec::new()))
            .collect();
        for (chord, cmd) in closure_input::mode_keymap(self.mode) {
            let sec = section_of(cmd);
            if let Some((_, v)) = groups.iter_mut().find(|(t, _)| t == sec) {
                v.push(((*chord).to_owned(), (*cmd).to_owned()));
            }
        }
        groups.retain(|(_, v)| !v.is_empty());
        for (_, v) in &mut groups {
            v.sort();
        }
        groups
    }

    /// Palette rows for the current filter: the shared
    /// [`command_palette`] source flattened in section order, so the
    /// modal palette shows the same grouped, described, chord-carrying
    /// entries as every other shell (G6).
    #[must_use]
    pub fn palette_entries(&self) -> Vec<PaletteEntry> {
        command_palette(&self.field_buf, self.mode)
            .into_iter()
            .flat_map(|s| s.items)
            .collect()
    }

    /// Cursor into [`Self::palette_entries`].
    #[must_use]
    pub const fn palette_cursor(&self) -> usize {
        self.palette_cursor
    }

    /// Palette keys: typing filters, Up/Down move, Enter runs the
    /// highlighted command through [`Self::run`], Esc cancels.
    fn on_palette_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.field_buf.clear();
                self.palette_cursor = 0;
                self.surface = ModalSurface::Browse;
            }
            "down" => {
                let last = self.palette_entries().len().saturating_sub(1);
                self.palette_cursor = (self.palette_cursor + 1).min(last);
            }
            "up" => self.palette_cursor = self.palette_cursor.saturating_sub(1),
            "backspace" => {
                self.field_buf.pop();
                self.palette_cursor = 0;
            }
            "enter" => self.commit_palette(shell),
            _ => {
                if let Some(c) = text {
                    self.field_buf.push(c);
                    self.palette_cursor = 0;
                }
            }
        }
    }

    /// Run the palette entry under the cursor and close the palette.
    fn commit_palette(&mut self, shell: &mut Shell) {
        let pick = self
            .palette_entries()
            .get(self.palette_cursor)
            .map(|e| e.action.command().to_owned());
        self.field_buf.clear();
        self.palette_cursor = 0;
        self.surface = ModalSurface::Browse;
        if let Some(cmd) = pick {
            self.run_command(shell, &cmd);
        }
    }

    /// Mouse path for the palette: clicking row `i` runs that entry —
    /// the same commit Enter performs on a moved cursor.
    pub fn palette_click(&mut self, shell: &mut Shell, i: usize) {
        if self.surface != ModalSurface::Palette {
            return;
        }
        self.palette_cursor = i.min(self.palette_entries().len().saturating_sub(1));
        self.commit_palette(shell);
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
    /// The Search overlay context line: search glyph, live query, caret
    /// bar, and the live match count, pluralized.
    #[must_use]
    pub fn search_context(&self, shell: &Shell) -> String {
        let n = self.rows_shared(shell).len();
        let m = if n == 1 { "match" } else { "matches" };
        format!("\u{2315} {}\u{258f} \u{b7} {} {}", self.query(), n, m)
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
        self.rows_shared(shell).as_ref().clone()
    }

    /// The same outline rows as [`Self::rows`], shared rather than
    /// cloned — the render path's entry point.
    ///
    /// A frame asks for the row list from the context line, the
    /// outline pane, the detail pane and from inside every mouse
    /// listener. Deriving it walks every document and allocates five
    /// strings per headline, so the result is memoised against
    /// `(vault revision, active filter)`: an unchanged vault and an
    /// unchanged query hand back the previous `Arc` for the cost of a
    /// refcount bump. Both key components are exactly what the
    /// derivation reads, so the memo cannot go stale — a mutation
    /// bumps [`closure_store::Vault::revision`] and a keystroke in the
    /// search overlay changes the filter.
    #[must_use]
    pub fn rows_shared(&self, shell: &Shell) -> std::sync::Arc<Vec<Row>> {
        let filter = if self.surface == ModalSurface::Search {
            self.query.as_str()
        } else {
            ""
        };
        let revision = shell.vault.revision();
        {
            let memo = self.row_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.revision == revision
                && m.filter == filter
            {
                return std::sync::Arc::clone(&m.rows);
            }
        }
        let rows = std::sync::Arc::new(Self::derive_rows(shell, filter));
        self.row_recomputes.set(self.row_recomputes.get() + 1);
        *self.row_memo.borrow_mut() = Some(RowMemo {
            revision,
            filter: filter.to_owned(),
            rows: std::sync::Arc::clone(&rows),
        });
        rows
    }

    /// Drop the memo so the next [`Self::rows_shared`] pays for a full
    /// walk. Nothing in normal operation needs this — the revision key
    /// handles invalidation — but it lets a caller (or a test) demand
    /// ground truth.
    pub fn invalidate_rows(&mut self) {
        *self.row_memo.borrow_mut() = None;
    }

    /// Ground truth: derive the outline rows without consulting or
    /// filling the memo. What [`Self::rows_shared`] must always agree
    /// with, and the escape hatch for a caller that would rather pay
    /// the walk than trust a cache.
    #[must_use]
    pub fn rows_uncached(&self, shell: &Shell) -> Vec<Row> {
        let filter = if self.surface == ModalSurface::Search {
            self.query.as_str()
        } else {
            ""
        };
        Self::derive_rows(shell, filter)
    }

    /// How many full vault walks [`Self::rows_shared`] has paid for
    /// since this app was created. The render budget is "at most one
    /// per actual change", and the tests assert it.
    #[must_use]
    pub const fn rows_recomputes(&self) -> u64 {
        self.row_recomputes.get()
    }

    /// The uncached derivation behind [`Self::rows_shared`].
    fn derive_rows(shell: &Shell, filter: &str) -> Vec<Row> {
        let mut scored: Vec<(u32, Row)> = Vec::new();
        for (p, doc) in shell.vault.iter() {
            // Fold-aware outline walk (same rule as the launcher App):
            // folds hide descendants in the listing, search sees through.
            let mut hide_below: Option<u8> = None;
            for h in doc.all_headlines() {
                if filter.is_empty() {
                    if let Some(limit) = hide_below {
                        if h.level() > limit {
                            continue;
                        }
                        hide_below = None;
                    }
                    if headline_is_folded(h) {
                        hide_below = Some(h.level());
                    }
                }
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
        self.scroll_override = None;
        let last = self.rows_shared(shell).len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// Wheel scrolling: move the viewport by `delta` rows (negative =
    /// up), clamped to the row range for a page-sized window. Does not
    /// move the selection; any selection movement clears the override
    /// and [`Self::view_window`] returns to its keep-selection-visible
    /// rule.
    pub fn scroll_by(&mut self, delta: i32, shell: &Shell, page: usize) {
        let rows = self.rows_shared(shell).len();
        let page = page.max(1);
        let max_off = rows.saturating_sub(page);
        let base = self
            .scroll_override
            .unwrap_or_else(|| self.selected.saturating_sub(page - 1).min(max_off));
        let step = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
        let new = if delta < 0 {
            base.saturating_sub(step)
        } else {
            base.saturating_add(step).min(max_off)
        };
        self.scroll_override = Some(new);
    }

    /// The visible slice of rows for a viewport of `page` rows, plus its
    /// start offset, chosen so the selection stays on screen. Stateless
    /// (offset derived from the selection each call); mirrors
    /// [`App::view_window`].
    #[must_use]
    pub fn view_window(&self, shell: &Shell, page: usize) -> (usize, Vec<Row>) {
        if let Some(o) = self.scroll_override {
            let rows = self.rows_shared(shell);
            let page = page.max(1);
            if rows.len() > page {
                let off = o.min(rows.len() - page);
                return (off, rows[off..off + page].to_vec());
            }
            return (0, rows.as_ref().clone());
        }
        let rows = self.rows_shared(shell);
        if page == 0 || rows.len() <= page {
            return (0, rows.as_ref().clone());
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
        let rows = self.rows_shared(shell);
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
            ModalSurface::EditBody => self.on_editbody_key(shell, key, ctrl, alt, text),
            ModalSurface::Backlinks => self.on_backlinks_key(shell, key),
            ModalSurface::Agenda => self.on_list_key(shell, key, ListKind::Agenda),
            ModalSurface::Blocks => self.on_list_key(shell, key, ListKind::Blocks),
            ModalSurface::TagsEdit => self.on_field_key(shell, key, text, FieldKind::Tags),
            ModalSurface::PropertyEdit => self.on_field_key(shell, key, text, FieldKind::Property),
            ModalSurface::Rename => self.on_field_key(shell, key, text, FieldKind::Rename),
            ModalSurface::AddSibling => self.on_field_key(shell, key, text, FieldKind::AddSibling),
            ModalSurface::Palette => self.on_palette_key(shell, key, text),
            ModalSurface::UndoHistory => match key {
                // Navigable pane (Q2-U3): j/k walk, Enter jumps the
                // undo tree to the cursor node, Esc/q dismiss.
                "j" | "down" => {
                    let last = self.undo_history_rows(shell).len().saturating_sub(1);
                    self.hist_cursor = (self.hist_cursor + 1).min(last);
                }
                "k" | "up" => self.hist_cursor = self.hist_cursor.saturating_sub(1),
                "enter" => {
                    let target = self.hist_cursor;
                    self.jump_undo_history(shell, target);
                }
                "escape" | "q" => self.surface = ModalSurface::Browse,
                _ => {}
            },
            ModalSurface::Headlines => {
                self.on_pane_key(key, self.headline_rows(shell).len());
            }
            ModalSurface::DbView => {
                self.on_pane_key(key, self.db_rows(shell).1.len());
            }
            ModalSurface::BodySearch => self.on_body_search_key(shell, key, text),
            ModalSurface::Sniffer => self.on_sniffer_key(shell, key),
            ModalSurface::Conflicts => self.on_conflicts_key(shell, key),
            ModalSurface::Browse => self.on_browse_key(shell, key, ctrl, alt, text),
        }
    }

    /// Shared navigation for the read-only panes: j/k walk, Escape
    /// leaves. `len` is the pane's row count, so the cursor clamps to
    /// what is actually painted.
    fn on_pane_key(&mut self, key: &str, len: usize) {
        match key {
            "j" | "down" => self.selected = (self.selected + 1).min(len.saturating_sub(1)),
            "k" | "up" => self.selected = self.selected.saturating_sub(1),
            "escape" | "q" => {
                self.selected = 0;
                self.surface = ModalSurface::Browse;
            }
            _ => {}
        }
    }

    /// The body-search overlay: typing narrows, Enter jumps to the hit,
    /// Escape leaves and clears.
    fn on_body_search_key(&mut self, shell: &Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.surface = ModalSurface::Browse;
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            "down" | "up" => {
                let last = self.body_search_rows(shell).len().saturating_sub(1);
                if key == "down" {
                    self.selected = (self.selected + 1).min(last);
                } else {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            "enter" => {
                if let Some((id, _)) = self.body_search_rows(shell).get(self.selected).cloned() {
                    self.query.clear();
                    self.surface = ModalSurface::Browse;
                    self.select_id(shell, &id);
                }
            }
            _ => {
                if let Some(c) = text {
                    self.query.push(c);
                    self.selected = 0;
                }
            }
        }
    }

    /// The flow list: a/b write an allow/block rule for the selected
    /// flow through the same commands the chords run (I8).
    fn on_sniffer_key(&mut self, shell: &mut Shell, key: &str) {
        match key {
            "j" | "down" => self.sniffer.select(self.sniffer_cursor() + 1),
            "k" | "up" => self.sniffer.select(self.sniffer_cursor().saturating_sub(1)),
            "a" => self.run_command(shell, "allow-flow"),
            "b" => self.run_command(shell, "block-flow"),
            "escape" | "q" => self.surface = ModalSurface::Browse,
            _ => {}
        }
    }

    /// The conflict list: o/t take ours/theirs for the selected field.
    fn on_conflicts_key(&mut self, shell: &mut Shell, key: &str) {
        match key {
            "j" | "down" => self.conflicts.select(self.conflicts.selected() + 1),
            "k" | "up" => self
                .conflicts
                .select(self.conflicts.selected().saturating_sub(1)),
            "o" => self.run_command(shell, "resolve-ours"),
            "t" => self.run_command(shell, "resolve-theirs"),
            "escape" | "q" => self.surface = ModalSurface::Browse,
            _ => {}
        }
    }

    /// Cursor row in the sniffer pane.
    #[must_use]
    pub const fn sniffer_cursor(&self) -> usize {
        self.sniffer.selected()
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
                        FieldKind::Rename => {
                            if !self.field_buf.trim().is_empty() {
                                let _ = shell.rename_headline(&bid, self.field_buf.trim());
                            }
                        }
                        FieldKind::AddSibling => {
                            if !self.field_buf.trim().is_empty() {
                                let _ = shell.add_sibling(&bid, self.field_buf.trim());
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
                    && let Some(idx) = self.rows_shared(shell).iter().position(|r| r.id == id)
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
            && let Some(idx) = self.rows_shared(shell).iter().position(|r| r.path == path)
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

    /// Mouse click into the body editor: place the cursor at
    /// `line`/`col` (clamped), keep the mode, end any completion
    /// session.
    pub fn body_click(&mut self, line: usize, col: usize) {
        self.completion = None;
        self.body.goto_line_col(line, col);
    }

    /// Double-click into the body editor: select the word under the
    /// position ([`BodyEditor::select_word_at_cursor`], Visual mode).
    pub fn body_double_click(&mut self, line: usize, col: usize) {
        self.body_click(line, col);
        self.body.select_word_at_cursor();
    }

    /// Mouse drag into the body editor: extend a charwise Visual
    /// selection from the click anchor to line/col
    /// ([`BodyEditor::drag_to`]); ends any completion session like
    /// [`Self::body_click`] does.
    pub fn body_drag(&mut self, line: usize, col: usize) {
        self.completion = None;
        self.body.drag_to(line, col);
    }

    /// First visible line of the body-editor pane (G5): an explicit
    /// wheel override while the cursor stays put, else follow the
    /// cursor (0 when it fits, otherwise the cursor on the last
    /// visible line) — the outline `view_window` rule for the body.
    #[must_use]
    pub fn body_scroll_start(&self, viewport: usize) -> usize {
        let (cl, _) = self.body.cursor_line_col();
        if let Some((start, at)) = self.body_scroll
            && at == cl
        {
            return start;
        }
        if cl < viewport { 0 } else { cl + 1 - viewport }
    }

    /// Wheel-scroll the body-editor viewport by `delta` lines (G5),
    /// clamped to `0..=lines - viewport`; any cursor-line change
    /// silently drops the override (the sibling of the outline's
    /// `scroll_by`).
    pub fn body_scroll_by(&mut self, delta: i32, viewport: usize) {
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let cur = self.body_scroll_start(viewport);
        let new = cur
            .saturating_add_signed(isize::try_from(delta).unwrap_or(0))
            .min(max);
        let (cl, _) = self.body.cursor_line_col();
        self.body_scroll = Some((new, cl));
    }

    /// Complete a drag-and-drop row reorder (G3): move the row at
    /// outline index `from` among its siblings until it sits at `to`,
    /// stepping the registry move commands through [`Self::run`] (I8,
    /// each step undoable, I3) — the selection follows the moved row.
    /// An out-of-range `from` or `from == to` changes nothing; the
    /// walk stops at sibling/parent/file boundaries, so an oversized
    /// `to` clamps to the last reachable slot. Ids are never
    /// regenerated by a move (I2).
    pub fn drag_drop_rows(&mut self, shell: &mut Shell, from: usize, to: usize) {
        let rows = self.rows_shared(shell);
        let Some(row) = rows.get(from) else { return };
        if from == to {
            return;
        }
        let id = row.id.clone();
        self.selected = from;
        let cmd = if to < from {
            "move-subtree-up"
        } else {
            "move-subtree-down"
        };
        for _ in 0..rows.len() {
            let Some(cur) = self.rows_shared(shell).iter().position(|r| r.id == id) else {
                break;
            };
            if (to < from && cur <= to) || (to > from && cur >= to) {
                break;
            }
            self.run(shell, cmd);
            if self.rows_shared(shell).iter().position(|r| r.id == id) == Some(cur) {
                break; // a boundary refused the move
            }
        }
    }

    /// The selected row's document undo-tree, flattened for the
    /// `UndoHistory` pane ([`Document::history_view`], I3). Empty when
    /// nothing is selected or no edit is recorded yet.
    #[must_use]
    pub fn undo_history_rows(&self, shell: &Shell) -> Vec<closure_core::HistoryRow> {
        self.rows_shared(shell)
            .get(self.selected)
            .and_then(|row| {
                let path = std::path::PathBuf::from(&row.path);
                shell
                    .vault
                    .document(&path)
                    .map(closure_core::Document::history_view)
            })
            .unwrap_or_default()
    }

    /// Cursor row inside the `UndoHistory` pane (Q2-U3).
    #[must_use]
    pub const fn undo_history_cursor(&self) -> usize {
        self.hist_cursor
    }

    /// Jump the selected row's document to history node `index`
    /// ([`closure_store::Vault::jump_history_in`] — composed undo/redo
    /// primitives, persisted) and return to Browse.
    fn jump_undo_history(&mut self, shell: &mut Shell, index: usize) {
        if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
            let path = std::path::PathBuf::from(&row.path);
            match shell.vault.jump_history_in(&path, index) {
                Ok(()) => "jumped".clone_into(&mut self.status),
                Err(e) => self.status = format!("jump failed: {e}"),
            }
            self.selected = self
                .selected
                .min(self.rows_shared(shell).len().saturating_sub(1));
        }
        self.surface = ModalSurface::Browse;
    }

    /// Mouse path into the `UndoHistory` pane: clicking row `i` jumps
    /// there, exactly like Enter (Q2-U3).
    pub fn undo_history_click(&mut self, shell: &mut Shell, i: usize) {
        self.hist_cursor = i;
        self.jump_undo_history(shell, i);
    }

    /// Re-point the selection at the row with `id` (the selection
    /// follows a moved subtree); no-op when the id is gone.
    fn select_id(&mut self, shell: &Shell, id: &str) {
        self.select_by_id(shell, id);
    }

    /// Move the outline selection to the row carrying `id`, reporting
    /// whether it was found.
    ///
    /// The list surfaces — headlines, body search, backlinks, the
    /// database table — answer in block ids, so this is how a clicked
    /// row (or a moved subtree) translates back into a selection. An
    /// unknown id leaves the cursor alone rather than resetting it.
    pub fn select_by_id(&mut self, shell: &Shell, id: &str) -> bool {
        if let Some(i) = self.rows_shared(shell).iter().position(|r| r.id == id) {
            self.selected = i;
            return true;
        }
        false
    }

    /// Index of the selected row's nearest sibling (same file, same
    /// level, no lower-level row between), forward or backward. `None`
    /// at the ends or when a parent boundary intervenes.
    fn sibling_index(&self, shell: &Shell, forward: bool) -> Option<usize> {
        let rows = self.rows_shared(shell);
        let cur = rows.get(self.selected)?;
        let (path, level) = (cur.path.clone(), cur.level);
        let scan: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(self.selected + 1..rows.len())
        } else {
            Box::new((0..self.selected).rev())
        };
        for j in scan {
            if rows[j].path != path || rows[j].level < level {
                return None;
            }
            if rows[j].level == level {
                return Some(j);
            }
        }
        None
    }

    /// Read-only agenda rows with today/overdue flags; today is injected
    /// (`YYYY-MM-DD`) so tests stay hermetic. Order = [`Vault::agenda`]
    /// (date ascending, title tie-break).
    #[must_use]
    pub fn agenda_context(&self, shell: &Shell, today: &str) -> Vec<AgendaRow> {
        shell
            .vault
            .agenda()
            .into_iter()
            .map(|e| AgendaRow {
                is_today: e.date == today,
                is_overdue: e.date.as_str() < today,
                kind: match e.kind {
                    closure_store::AgendaKind::Scheduled => "SCHEDULED".to_owned(),
                    closure_store::AgendaKind::Deadline => "DEADLINE".to_owned(),
                },
                date: e.date,
                title: e.title,
            })
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
            "enter" => self.jump_to_selected_backlink(shell),
            _ => {}
        }
    }

    fn jump_to_selected_backlink(&mut self, shell: &Shell) {
        // Jump: make the chosen backlink the Browse selection.
        if let Some((_, title)) = self.backlink_rows(shell).get(self.selected).cloned() {
            self.link_target = None;
            self.surface = ModalSurface::Browse;
            if let Some(idx) = self
                .rows_shared(shell)
                .iter()
                .position(|r| r.title == title)
            {
                self.selected = idx;
            }
        }
    }

    /// Mouse path for the Backlinks surface: clicking row i jumps to
    /// that linking headline - the same jump Enter performs. Out-of-range
    /// clicks and an empty list are safe no-ops.
    pub fn backlink_click(&mut self, shell: &Shell, i: usize) {
        if self.surface != ModalSurface::Backlinks {
            return;
        }
        if self.backlink_rows(shell).get(i).is_none() {
            return;
        }
        self.selected = i;
        self.jump_to_selected_backlink(shell);
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

    /// Body editor keys (org-edit-special), vim-modal (contract revised
    /// 2026-07-04): `C-<enter>` commits from either mode. INSERT types
    /// at the cursor, `Esc` drops to NORMAL. NORMAL navigates
    /// (`h`/`j`/`k`/`l`/arrows/`0`/`$`), edits (`i`/`a`/`o`/`x`), and
    /// `Esc` cancels the edit.
    fn on_editbody_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if key == "enter" && ctrl {
            self.commit_edit_body(shell);
            return;
        }
        // While the "/" menu is open it owns navigation and accept;
        // everything else falls through, so typing keeps editing the
        // buffer and the query follows along in `sync_slash`.
        if self.slash.is_some() {
            match key {
                "escape" => {
                    self.slash = None;
                    return;
                }
                "enter" => {
                    self.accept_slash();
                    return;
                }
                "down" | "up" => {
                    if let Some((query, cursor)) = self.slash.as_mut() {
                        let last = block_templates(query).len().saturating_sub(1);
                        *cursor = if key == "down" {
                            (*cursor + 1).min(last)
                        } else {
                            cursor.saturating_sub(1)
                        };
                    }
                    return;
                }
                _ => {}
            }
        }
        self.edit_body_key(shell, key, ctrl, alt, text);
        if self.body.mode() == EditorMode::Insert {
            self.sync_slash();
        } else {
            self.slash = None;
        }
    }

    /// The Notion "/" menu's query while it is open.
    #[must_use]
    pub fn slash_query(&self) -> Option<&str> {
        self.slash.as_ref().map(|(q, _)| q.as_str())
    }

    /// The templates the open menu is offering, best match first.
    #[must_use]
    pub fn slash_items(&self) -> Vec<BlockTemplate> {
        self.slash
            .as_ref()
            .map(|(q, _)| block_templates(q))
            .unwrap_or_default()
    }

    /// Cursor row in the open menu.
    #[must_use]
    pub fn slash_cursor(&self) -> usize {
        self.slash.as_ref().map_or(0, |(_, c)| *c)
    }

    /// Accept menu entry `i` from a click — the same accept Enter
    /// performs (I8). An index past the end leaves the menu alone.
    pub fn slash_click(&mut self, i: usize) {
        if i >= self.slash_items().len() {
            return;
        }
        if let Some((_, cursor)) = self.slash.as_mut() {
            *cursor = i;
        }
        self.accept_slash();
    }

    /// Open the "/" menu, or close it, from what is actually in the
    /// buffer — so backspacing past the slash closes it and no
    /// separate bookkeeping can drift out of sync with the text.
    ///
    /// The trigger is a `/` that *starts a word*: at the beginning of a
    /// line or after whitespace. `and/or`, a URL and a date are text,
    /// not commands. A space after the slash ends it, the same way it
    /// ends a Notion slash command.
    fn sync_slash(&mut self) {
        let (_, col) = self.body.cursor_line_col();
        let before: String = self.body.current_line().chars().take(col).collect();
        let Some(pos) = before.rfind('/') else {
            self.slash = None;
            return;
        };
        let starts_word = before[..pos]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let query = &before[pos + 1..];
        if !starts_word || query.contains(char::is_whitespace) {
            self.slash = None;
            return;
        }
        let cursor = self.slash_cursor();
        let last = block_templates(query).len().saturating_sub(1);
        self.slash = Some((query.to_owned(), cursor.min(last)));
    }

    /// Replace the `/query` trigger with the selected template and put
    /// the caret where that template asks for it.
    fn accept_slash(&mut self) {
        let Some((query, cursor)) = self.slash.take() else {
            return;
        };
        let Some(tpl) = block_templates(&query).into_iter().nth(cursor) else {
            return;
        };
        // The trigger is the slash plus the query, both immediately
        // behind the cursor.
        let start = self.body.cursor_byte().saturating_sub(1 + query.len());
        self.body.replace_to_cursor(start, tpl.text);
        // The caret goes where the template asks — often inside a
        // multi-line block, which the newline-stopping motions cannot
        // reach, so address it directly.
        let offset = tpl
            .text
            .char_indices()
            .nth(tpl.cursor)
            .map_or(tpl.text.len(), |(b, _)| b);
        self.body.set_cursor_byte(start + offset);
    }

    /// The body editor's own key handling, unaware of the "/" menu.
    fn edit_body_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match self.body.mode() {
            EditorMode::Insert => match key {
                // G4: the first buffer-changing edit checkpoints the
                // burst (BodyEditor::insert_guard), so Esc+u undoes it.
                "n" if ctrl => self.cycle_completion(shell, true),
                "p" if ctrl => self.cycle_completion(shell, false),
                // Readline chords (the "normal input field" set).
                "a" if ctrl => self.body.line_home(),
                "e" if ctrl => self.body.line_end_motion(),
                "b" if ctrl => self.body.left(),
                "f" if ctrl => self.body.right(),
                "d" if ctrl => self.body.delete_at(),
                // Desktop-standard word ops (Q5): ctrl/alt+arrows jump
                // words, ctrl+backspace kills the word (same as C-w).
                "left" if ctrl || alt => self.body.word_backward(),
                "right" if ctrl || alt => self.body.word_forward(),
                "k" if ctrl => {
                    self.completion = None;
                    self.body.kill_rest_of_line();
                }
                "u" if ctrl => {
                    self.completion = None;
                    self.body.kill_to_line_start();
                }
                // C-w and the desktop ctrl+backspace share the kill.
                "w" | "backspace" if ctrl => {
                    self.completion = None;
                    self.body.delete_word_back();
                }
                "y" if ctrl => {
                    self.completion = None;
                    self.body.yank_insert();
                }
                "escape" => {
                    self.completion = None;
                    self.body.to_normal();
                }
                "enter" => {
                    self.completion = None;
                    self.body.insert_char('\n');
                }
                "backspace" => {
                    self.completion = None;
                    self.body.backspace();
                }
                "tab" => {
                    // An active completion session wins over org-tempo:
                    // TAB accepts — the applied candidate stays; an
                    // unapplied popup applies its first candidate.
                    match self.completion.take() {
                        Some(s) if s.ix.is_none() => {
                            if let Some(first) = s.items.first() {
                                self.body.replace_to_cursor(s.start, first);
                            }
                        }
                        Some(_) => {}
                        None => self.body.tempo_expand_or_indent(),
                    }
                }
                _ => {
                    if let Some(c) = text.filter(|_| !ctrl) {
                        self.completion = None;
                        self.body.insert_char(c);
                    }
                }
            },
            EditorMode::Normal | EditorMode::Visual | EditorMode::VisualLine => {
                if ctrl && key == "r" {
                    self.body.redo_local();
                    return;
                }
                // Esc on a quiet Normal surface cancels the edit; every
                // other key (incl. Esc mid-chord / in Visual) is the
                // editor's own modal vocabulary.
                if key == "escape"
                    && self.body.mode() == EditorMode::Normal
                    && self.body.pending_stroke().is_none()
                    && self.body.pending_count() == 0
                {
                    self.edit_target = None;
                    self.body.clear();
                    self.surface = ModalSurface::Browse;
                } else {
                    self.body.modal_key(key);
                }
            }
        }
    }

    /// The body editor buffer (read).
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        self.body.text()
    }

    /// The body editor's vim mode (for the mode indicator).
    #[must_use]
    pub const fn body_mode(&self) -> EditorMode {
        self.body.mode()
    }

    /// Candidates of the live completion cycle (empty when none) — the
    /// popup a GUI renders beside the caret.
    #[must_use]
    pub fn body_completion_items(&self) -> &[String] {
        self.completion.as_ref().map_or(&[], |s| &s.items)
    }

    /// Index of the currently-applied completion candidate.
    #[must_use]
    pub fn body_completion_ix(&self) -> Option<usize> {
        self.completion.as_ref().and_then(|s| s.ix)
    }

    /// `C-n`/`C-p`: start or continue a completion cycle over the word
    /// before the cursor — org keywords + vault dabbrev words
    /// ([`body_completions`]). Each step replaces the prefix/previous
    /// candidate in place; any other key ends the session.
    fn cycle_completion(&mut self, shell: &Shell, forward: bool) {
        if let Some(s) = &mut self.completion {
            let n = s.items.len();
            // From an unapplied popup the first step lands on the
            // first (or last) candidate instead of skipping it.
            let ix = match (s.ix, forward) {
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
                (None, true) => 0,
                (None, false) => n - 1,
            };
            s.ix = Some(ix);
            let (start, text) = (s.start, s.items[ix].clone());
            self.body.replace_to_cursor(start, &text);
            return;
        }
        let start = self.body.word_start();
        let prefix = self.body.word_prefix().to_owned();
        let mut items = body_completions(&prefix, &shell.vault);
        // The popup (and the cycle) shows the top 8 ranked candidates.
        items.truncate(8);
        if items.is_empty() {
            "no completions".clone_into(&mut self.status);
            return;
        }
        let text = items[0].clone();
        self.body.replace_to_cursor(start, &text);
        self.completion = Some(CompletionSession {
            start,
            items,
            ix: Some(0),
        });
    }

    /// Whether the GUI should auto-open the completion popup after its
    /// typing-idle delay: INSERT in the body editor, no session yet, a
    /// word prefix of at least 3 chars with candidates behind it.
    #[must_use]
    pub fn completion_should_popup(&self, shell: &Shell) -> bool {
        self.surface == ModalSurface::EditBody
            && self.body.mode() == EditorMode::Insert
            && self.completion.is_none()
            && self.body.word_prefix().chars().count() >= 3
            && !body_completions(self.body.word_prefix(), &shell.vault).is_empty()
    }

    /// Open the completion popup without applying anything: candidates
    /// show, the buffer stays untouched until `C-n`/`C-p`/TAB.
    pub fn open_completion_popup(&mut self, shell: &Shell) {
        if !self.completion_should_popup(shell) {
            return;
        }
        let start = self.body.word_start();
        let mut items = body_completions(self.body.word_prefix(), &shell.vault);
        items.truncate(8);
        self.completion = Some(CompletionSession {
            start,
            items,
            ix: None,
        });
    }

    /// The body editor cursor as `(line, column)` for the caret.
    #[must_use]
    pub fn body_cursor(&self) -> (usize, usize) {
        self.body.cursor_line_col()
    }

    /// The body editor's Visual selection byte range for the renderer.
    #[must_use]
    pub fn body_selection(&self) -> Option<(usize, usize)> {
        self.body.visual_selection()
    }

    /// Commit the body buffer to the target headline through the kernel
    /// command (I8), then return to Browse. No-op if not editing.
    pub fn commit_edit_body(&mut self, shell: &mut Shell) {
        if let Some(id) = self.edit_target.take() {
            let bid = closure_core::BlockId::from_existing(&id);
            let mut body = self.body.text().to_owned();
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            match shell.set_body(&bid, &body) {
                Ok(()) => "body saved".clone_into(&mut self.status),
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
        self.body.clear();
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
                let last = self.rows_shared(shell).len().saturating_sub(1);
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

    // A flat one-arm-per-command dispatch reads clearest as one match;
    // the same precedent as `view_to_json` / `qml_item`.
    #[allow(clippy::too_many_lines)]
    fn run_command(&mut self, shell: &mut Shell, cmd: &str) {
        let last = self.rows_shared(shell).len().saturating_sub(1);
        match cmd {
            "next-file" => {
                self.scroll_override = None;
                self.selected = (self.selected + 1).min(last);
            }
            "prev-file" => {
                self.scroll_override = None;
                self.selected = self.selected.saturating_sub(1);
            }
            "first-file" => {
                self.scroll_override = None;
                self.selected = 0;
            }
            "last-file" => {
                self.scroll_override = None;
                self.selected = last;
            }
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
                if let Some(row) = self.rows_shared(shell).get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "backlinks" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.link_target = Some(row.id);
                    self.selected = 0;
                    self.surface = ModalSurface::Backlinks;
                }
            }
            "undo-history" => {
                self.surface = ModalSurface::UndoHistory;
                // The cursor opens on the active node (Q2-U3).
                self.hist_cursor = self
                    .undo_history_rows(shell)
                    .iter()
                    .position(|r| r.is_current)
                    .unwrap_or(0);
                "undo history — j/k move · RET jump · Esc back".clone_into(&mut self.status);
            }
            "agenda" => {
                self.selected = 0;
                self.surface = ModalSurface::Agenda;
            }
            "block-list" => {
                self.selected = 0;
                self.surface = ModalSurface::Blocks;
            }
            "toggle-fold" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = toggle_visibility(shell, &bid);
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "toggle-todo" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
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
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
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
            "promote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.promote(&bid);
                }
            }
            "demote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.demote(&bid);
                }
            }
            "move-subtree-up" => {
                // Moving up = moving the previous sibling below us; the
                // selection follows the moved heading (org rule).
                if let Some(prev) = self.sibling_index(shell, false) {
                    let rows = self.rows_shared(shell);
                    let (p, s) = (rows[prev].id.clone(), rows[self.selected].id.clone());
                    let _ = shell.move_after(
                        &closure_core::BlockId::from_existing(&p),
                        &closure_core::BlockId::from_existing(&s),
                    );
                    self.select_id(shell, &s);
                }
            }
            "move-subtree-down" => {
                if let Some(next) = self.sibling_index(shell, true) {
                    let rows = self.rows_shared(shell);
                    let (s, n) = (rows[self.selected].id.clone(), rows[next].id.clone());
                    let _ = shell.move_after(
                        &closure_core::BlockId::from_existing(&s),
                        &closure_core::BlockId::from_existing(&n),
                    );
                    self.select_id(shell, &s);
                }
            }
            "add-heading" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.add_sibling(&bid, "untitled");
                }
            }
            "edit-tags" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let tags = self.detail(shell).map(|d| d.tags).unwrap_or_default();
                    self.field_target = Some(row.id);
                    self.field_buf = tags.join(" ");
                    self.surface = ModalSurface::TagsEdit;
                }
            }
            "edit-property" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf.clear();
                    self.surface = ModalSurface::PropertyEdit;
                }
            }
            "edit-body" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.edit_target = Some(row.id);
                    self.body
                        .load(self.detail(shell).map(|d| d.body).unwrap_or_default());
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
            "rename" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf = row.title;
                    self.surface = ModalSurface::Rename;
                    "rename — Enter save, Esc cancel".clone_into(&mut self.status);
                }
            }
            "add-sibling" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf.clear();
                    self.surface = ModalSurface::AddSibling;
                    "add sibling — Enter save, Esc cancel".clone_into(&mut self.status);
                }
            }
            "delete" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    match shell.remove_subtree(&bid) {
                        Ok(()) => self.status = format!("deleted: {}", row.title),
                        Err(e) => self.status = format!("delete failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "undo" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let path = std::path::PathBuf::from(&row.path);
                    match shell.vault.undo_in(&path) {
                        Ok(()) => "undo".clone_into(&mut self.status),
                        Err(e) => self.status = format!("undo failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "redo" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let path = std::path::PathBuf::from(&row.path);
                    match shell.vault.redo_in(&path) {
                        Ok(()) => "redo".clone_into(&mut self.status),
                        Err(e) => self.status = format!("redo failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "palette" => {
                self.field_buf.clear();
                self.palette_cursor = 0;
                self.surface = ModalSurface::Palette;
                "palette — type to filter, Enter to run".clone_into(&mut self.status);
            }
            "headline-list" => {
                self.selected = 0;
                self.surface = ModalSurface::Headlines;
                "headlines — RET jump, Esc back".clone_into(&mut self.status);
            }
            "db-view" => {
                self.selected = 0;
                self.surface = ModalSurface::DbView;
                "database — RET jump, Esc back".clone_into(&mut self.status);
            }
            "body-search" => {
                self.query.clear();
                self.selected = 0;
                self.surface = ModalSurface::BodySearch;
                "body search — type to filter, RET jump, Esc back".clone_into(&mut self.status);
            }
            "toggle-llm-render" => {
                self.llm_render = !self.llm_render;
                self.status = format!(
                    "LLM render access {}",
                    if self.llm_render {
                        "granted"
                    } else {
                        "revoked"
                    }
                );
            }
            "sniffer" => {
                self.selected = 0;
                self.surface = ModalSurface::Sniffer;
                "flows — a allow, b block, Esc back".clone_into(&mut self.status);
            }
            "allow-flow" | "block-flow" => {
                if self.sniffer.events().is_empty() {
                    "no captured flows".clone_into(&mut self.status);
                } else {
                    if cmd == "allow-flow" {
                        self.sniffer.allow_selected();
                    } else {
                        self.sniffer.block_selected();
                    }
                    self.surface = ModalSurface::Sniffer;
                    self.status = self
                        .sniffer
                        .detail()
                        .unwrap_or_else(|| "flow rule updated".to_owned());
                }
            }
            "conflicts" => {
                self.selected = 0;
                self.surface = ModalSurface::Conflicts;
                "conflicts — o ours, t theirs, Esc back".clone_into(&mut self.status);
            }
            "resolve-ours" | "resolve-theirs" => {
                if self.conflicts.conflicts().is_empty() {
                    "no conflicts to resolve".clone_into(&mut self.status);
                } else {
                    let ours = cmd == "resolve-ours";
                    let result = if ours {
                        self.conflicts.resolve_ours(shell)
                    } else {
                        self.conflicts.resolve_theirs(shell)
                    };
                    let side = if ours { "ours" } else { "theirs" };
                    self.status = match result {
                        Ok(()) => format!("resolved {side}"),
                        Err(e) => format!("resolve failed: {e}"),
                    };
                    self.surface = ModalSurface::Conflicts;
                }
            }
            other => self.status = format!("{other}: unknown command"),
        }
    }

    /// Every headline in the selected row's file, as `(title, id)` —
    /// the flat per-file listing behind the Headlines surface.
    #[must_use]
    pub fn headline_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        let rows = self.rows_shared(shell);
        let Some(row) = rows.get(self.selected.min(rows.len().saturating_sub(1))) else {
            return Vec::new();
        };
        let path = std::path::PathBuf::from(&row.path);
        shell
            .vault
            .iter()
            .filter(|(p, _)| *p == path.as_path())
            .flat_map(|(_, doc)| doc.all_headlines())
            .map(|h| (h.title().to_owned(), h.id().to_string()))
            .collect()
    }

    /// The Notion-style database table over the whole vault: a header
    /// row plus one cell row per headline.
    ///
    /// Deliberately loose, the way Notion's databases are: every
    /// headline is a row, a missing value is an empty cell rather than
    /// an error, and the columns are the properties an org headline
    /// always has.
    #[must_use]
    pub fn db_rows(&self, shell: &Shell) -> (Vec<String>, Vec<Vec<String>>) {
        let header = ["title", "todo", "priority", "tags"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let rows = shell
            .vault
            .iter()
            .flat_map(|(_, doc)| doc.all_headlines())
            .map(|h| {
                vec![
                    h.title().to_owned(),
                    h.todo().unwrap_or_default().to_owned(),
                    h.priority().map(String::from).unwrap_or_default(),
                    h.tags().join(" "),
                ]
            })
            .collect();
        (header, rows)
    }

    /// Body-text hits for the current query, as `(id, "title — line")`.
    ///
    /// The outline search matches titles; this one matches the text
    /// underneath them, which is the only way to find a note you
    /// remember the contents but not the heading of. An empty query
    /// matches nothing — dumping the vault is not a search result.
    #[must_use]
    pub fn body_search_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        let needle = self.query.to_lowercase();
        let mut out = Vec::new();
        for (_, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                let body = h.body_text();
                if let Some(line) = body
                    .lines()
                    .find(|l| l.to_lowercase().contains(needle.as_str()))
                {
                    out.push((
                        h.id().to_string(),
                        format!("{} — {}", h.title(), line.trim()),
                    ));
                }
            }
        }
        out
    }

    /// Whether the LLM may read the rendered view (V3b). Off until
    /// `toggle-llm-render` grants it.
    #[must_use]
    pub const fn llm_render_access(&self) -> bool {
        self.llm_render
    }

    /// The sniffer surface's state, for painting and for feeding
    /// captures in.
    #[must_use]
    pub const fn sniffer(&self) -> &SnifferApp {
        &self.sniffer
    }

    /// Mutable sniffer state — a shell records live captures here.
    pub const fn sniffer_mut(&mut self) -> &mut SnifferApp {
        &mut self.sniffer
    }

    /// The pending CRDT conflicts, for painting.
    #[must_use]
    pub const fn conflicts(&self) -> &ConflictApp {
        &self.conflicts
    }

    /// Load the conflicts a merge produced, so the Conflicts surface
    /// has something to resolve.
    pub fn set_conflicts(&mut self, conflicts: Vec<closure_crdt::FieldConflict>) {
        self.conflicts = ConflictApp::new(conflicts, self.mode);
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
        "left" => "<left>".to_owned(),
        "right" => "<right>".to_owned(),
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
