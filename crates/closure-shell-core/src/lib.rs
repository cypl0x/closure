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
    pub fn capture(
        &mut self,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        let template = closure_store::CaptureTemplate {
            target: std::path::PathBuf::from("inbox.org"),
            headline_prefix: "TODO ".to_owned(),
            body: String::new(),
        };
        self.vault.capture(&template, title)
    }

    /// Capture a new `TODO` as the last child of `parent` (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`] from the capture.
    pub fn capture_under(
        &mut self,
        parent: &closure_core::BlockId,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        self.vault.capture_under(parent, "TODO ", title)
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
    /// Whether this headline's subtree is folded (`:VISIBILITY: folded`).
    ///
    /// Carried on the row because the outline needs it for every
    /// visible row on every frame, and the fold walk in `derive_rows`
    /// has already computed it — asking the vault again per row per
    /// frame is the same answer at wheel speed.
    pub folded: bool,
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

/// Split a CSS-shaped font stack into family names, best first.
///
/// The stacks are spelled the way CSS spells them because the web tier
/// drops them straight into a `font-family` rule. Every native toolkit
/// wants one family plus an ordered fallback list, and the gpui shell
/// used to hand the *whole string* to gpui's `font_family()` — asking
/// the platform for a font literally called
/// `"JetBrains Mono, ui-monospace, monospace"`, getting nothing, and
/// falling back to whatever the platform chose. Empty names are dropped
/// so a ragged stack cannot ask for a font with no name.
#[must_use]
pub fn font_stack(stack: &str) -> Vec<&str> {
    stack
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
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
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
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
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
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
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
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
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
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

/// The contiguous run of table lines around line `line`, as a line
/// range, or `None` when that line is not part of a table.
///
/// "Table" is what org means by it: consecutive lines whose first
/// non-space character is a pipe. A blank line — or any prose —
/// separates two tables rather than joining them.
#[must_use]
pub fn table_bounds(text: &str, line: usize) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let is_row = |l: &str| l.trim_start().starts_with('|');
    if !lines.get(line).is_some_and(|l| is_row(l)) {
        return None;
    }
    let mut start = line;
    while start > 0 && is_row(lines[start - 1]) {
        start -= 1;
    }
    let mut end = line + 1;
    while end < lines.len() && is_row(lines[end]) {
        end += 1;
    }
    Some(start..end)
}

/// Realign an org table: every column padded to its widest cell, and
/// every `|---+---|` rule redrawn to match.
///
/// This is the feature that makes plain-text tables bearable, and the
/// reason TAB in org does more than move. Cells are measured in
/// *characters*, so a non-ASCII cell still lines up. Idempotent —
/// pressing TAB twice must not keep growing the table — and it never
/// drops a cell from a half-typed row, only pads it out.
#[must_use]
pub fn align_table(table: &str) -> String {
    /// Split a row into its cells, dropping the outer pipes.
    fn cells(line: &str) -> Vec<&str> {
        let t = line.trim();
        let inner = t
            .strip_prefix('|')
            .map_or(t, |s| s.strip_suffix('|').unwrap_or(s));
        inner.split('|').map(str::trim).collect()
    }
    let is_rule = |line: &str| {
        let t = line.trim();
        t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | '+' | ' '))
    };
    let rows: Vec<&str> = table.lines().collect();
    if rows.is_empty() {
        return table.to_owned();
    }
    // Widths come from the content rows only; a rule has no content to
    // measure and would otherwise dictate the width.
    let mut widths: Vec<usize> = Vec::new();
    for row in rows.iter().filter(|r| !is_rule(r)) {
        for (i, cell) in cells(row).iter().enumerate() {
            let w = cell.chars().count();
            if i < widths.len() {
                widths[i] = widths[i].max(w);
            } else {
                widths.push(w);
            }
        }
    }
    let mut out = String::with_capacity(table.len());
    for row in rows {
        if is_rule(row) {
            out.push('|');
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    out.push('+');
                }
                for _ in 0..w + 2 {
                    out.push('-');
                }
            }
            out.push('|');
        } else {
            let row_cells = cells(row);
            out.push('|');
            for (i, w) in widths.iter().enumerate() {
                let cell = row_cells.get(i).copied().unwrap_or("");
                out.push(' ');
                out.push_str(cell);
                for _ in 0..w.saturating_sub(cell.chars().count()) {
                    out.push(' ');
                }
                out.push_str(" |");
            }
        }
        out.push('\n');
    }
    out
}

/// Byte offset of the next cell's content on `row`, from `at`.
///
/// `None` when there is no next cell — at the last one, or off a
/// table row entirely; the caller decides whether that means wrapping
/// to the following row. The offset lands on the cell's *content*,
/// past the padding, because that is where you want to start typing.
#[must_use]
pub fn next_table_cell(row: &str, at: usize) -> Option<usize> {
    if !row.trim_start().starts_with('|') || at >= row.len() {
        return None;
    }
    let pipe = row[at..].find('|').map(|off| at + off)?;
    let rest = &row[pipe + 1..];
    // The trailing pipe of the row is not a cell boundary.
    if rest.trim().is_empty() {
        return None;
    }
    let lead = rest.len() - rest.trim_start().len();
    Some(pipe + 1 + lead)
}

/// The byte range of the source-block *content* enclosing `at`, and
/// the block's language — the org-edit-special lookup.
///
/// `at` may sit anywhere in the block including on either fence line,
/// because point-on-`#+BEGIN_SRC` is how the command is usually
/// reached. The returned range covers the lines between the fences and
/// nothing else, so it can be replaced wholesale without disturbing
/// them. An unterminated block is not a block: a half-typed fence must
/// not swallow the rest of the buffer.
#[must_use]
pub fn enclosing_src_block(text: &str, at: usize) -> Option<(std::ops::Range<usize>, String)> {
    if at > text.len() {
        return None;
    }
    let mut open: Option<(usize, usize, String)> = None; // (fence start, content start, lang)
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let lower = line.trim_start().to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("#+begin_src") {
            let lang = rest.split_whitespace().next().unwrap_or("").to_owned();
            open = Some((offset, offset + line.len(), lang));
        } else if lower.starts_with("#+end_src")
            && let Some((fence_start, content_start, lang)) = open.take()
            && (fence_start..offset + line.len()).contains(&at)
        {
            return Some((content_start..offset, lang));
        }
        offset += line.len();
    }
    None
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

/// What we know about a peer we have been handed a ticket for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerState {
    /// Its ticket is on file; nothing has been sent yet.
    Known,
    /// A round completed; `blocks` is what we know about afterwards.
    Synced {
        /// Blocks in our replica after the merge.
        blocks: usize,
    },
    /// The last attempt failed, with this reason. Shown, not swallowed.
    Failed(String),
}

/// A peer in the sync surface's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// Where it listens.
    pub addr: std::net::SocketAddr,
    /// Its verifying key — frames it sends must be signed with this.
    pub key: closure_sync::VerifyingKey,
    /// What happened last.
    pub state: PeerState,
}

/// A one-line text field: the text, and where in it the cursor is.
///
/// The overlays — capture, search, the pairing ticket, the tag and
/// property fields — were plain `String`s that only knew `push` and
/// `pop`. That is a field you can only edit at the end: no `C-a`, no
/// `C-e`, no fixing a typo in the middle without retyping the tail,
/// and no `C-w`. Every surface reimplementing that badly is worse than
/// one type doing it once, so this is the field itself and the
/// surfaces route their keys into it.
///
/// Positions are byte offsets on a char boundary; the motions step by
/// *character*, because a captured German line is the normal case and
/// half a `ß` is not a thing to leave behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    cursor: usize,
}

impl LineInput {
    /// The text as typed so far.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset of the cursor within [`Self::text`].
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Replace the contents, cursor at the end — what restoring a
    /// rejected ticket or a remembered query wants.
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor = self.text.len();
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Take the text out, leaving the field empty.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Whether anything has been typed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Insert one character at the cursor.
    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a run of text at the cursor — a paste.
    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    /// Delete the character under the cursor.
    pub fn delete(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.text.replace_range(self.cursor..next, "");
        }
    }

    /// One character left.
    pub fn left(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.cursor = prev;
        }
    }

    /// One character right.
    pub fn right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.cursor = next;
        }
    }

    /// To the start of the line (`C-a`, Home).
    pub const fn home(&mut self) {
        self.cursor = 0;
    }

    /// To the end of the line (`C-e`, End).
    pub const fn end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Delete the word before the cursor (`C-w`, ctrl+backspace).
    pub fn delete_word_back(&mut self) {
        let start = self.word_start();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Delete from the cursor to the start of the line (`C-u`).
    pub fn kill_to_start(&mut self) {
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Delete from the cursor to the end of the line (`C-k`).
    pub fn kill_to_end(&mut self) {
        self.text.truncate(self.cursor);
    }

    /// Offer `key` to the field, reporting whether it was consumed.
    ///
    /// The surface keeps the keys that mean something to *it* —
    /// `enter`, `escape`, its own navigation — and hands the rest here,
    /// so every one-line field in the app answers to the same chords.
    pub fn key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>) -> bool {
        match key {
            "a" if ctrl => self.home(),
            "e" if ctrl => self.end(),
            "b" if ctrl => self.left(),
            "f" if ctrl => self.right(),
            "d" if ctrl => self.delete(),
            "k" if ctrl => self.kill_to_end(),
            "u" if ctrl => self.kill_to_start(),
            "w" if ctrl => self.delete_word_back(),
            "backspace" if ctrl || alt => self.delete_word_back(),
            "backspace" => self.backspace(),
            "delete" => self.delete(),
            "left" => self.left(),
            "right" => self.right(),
            "home" => self.home(),
            "end" => self.end(),
            _ => {
                // A bare character is text; the same letter under a
                // modifier is a chord nobody bound, and typing its
                // letter would be a surprise.
                let Some(c) = text.filter(|_| !ctrl && !alt) else {
                    return false;
                };
                self.insert_char(c);
            }
        }
        true
    }

    /// Byte offset of the character boundary before the cursor.
    fn prev_boundary(&self) -> Option<usize> {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
    }

    /// Byte offset of the boundary after the character at the cursor.
    fn next_boundary(&self) -> Option<usize> {
        self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
    }

    /// Where the word before the cursor starts: the run of whitespace
    /// immediately behind it, and then the word behind that.
    fn word_start(&self) -> usize {
        let head = &self.text[..self.cursor];
        let trimmed = head.trim_end();
        let without_word = trimmed.trim_end_matches(|c: char| !c.is_whitespace());
        without_word.len()
    }
}

/// Body lines a shell is assumed to be able to paint until it says
/// otherwise ([`ModalApp::set_body_viewport`]).
pub const BODY_VIEWPORT_DEFAULT: usize = 20;

/// One zoom step, as a ratio. Doom scales its font by an increment per
/// press; a ratio is the same idea in a world with no font table.
const ZOOM_STEP: f32 = 1.1;
/// Zoom ceiling (`1.1^15` ≈ 4.2×) and floor (`1.1^-7` ≈ 0.51×). Past
/// either end a "zoom level" stops being one: a wall of one glyph, or a
/// font nobody can read.
const ZOOM_MAX_STEPS: i8 = 15;
/// See [`ZOOM_MAX_STEPS`].
const ZOOM_MIN_STEPS: i8 = -7;

/// A key the body editor is holding until the rest of its chord
/// arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyPrefix {
    /// `z` — evil's viewport prefix (`zz` / `zt` / `zb`).
    Viewport,
}

/// Where the cursor line should end up in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFraming {
    /// Middle of the pane (`zz`, and `C-l`'s first press).
    Centre,
    /// First visible line (`zt`).
    Top,
    /// Last visible line (`zb`).
    Bottom,
}

/// Where pairing listens when nothing has said otherwise: the closure
/// port, on every interface.
///
/// It used to be `127.0.0.1:7420`, which made every ticket a lie the
/// moment it left the machine — the peer dialled its own loopback and
/// reached itself. Binding wide is only half the fix; the other half is
/// that a network-facing listener refuses inbound rounds until it has
/// been given a peer to trust ([`SyncApp::inbound_ready`]), and that
/// nothing binds at all until the user asks to listen.
pub const DEFAULT_SYNC_BIND: std::net::SocketAddr =
    std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 7420);

/// Collaboration state: our identity, the ticket we hand out, the
/// peers we have been given, and the replica that merges with them.
///
/// The socket work is deliberately *not* here — it blocks, and this
/// has to stay callable from a render thread. A shell dials the peer,
/// hands the outcome back through [`SyncApp::record_outcome`], and
/// merges what it received with [`SyncApp::merge_session`]. Everything
/// that decides *what* happens is here, and testable without a network.
pub struct SyncApp {
    name: String,
    /// What the ticket names — where a *peer* dials us. Not necessarily
    /// where we bind: `0.0.0.0` is bindable and undialable.
    addr: std::net::SocketAddr,
    /// The socket we open. `0.0.0.0:7420` accepts from the network;
    /// `127.0.0.1:…` keeps pairing on this machine.
    bind: std::net::SocketAddr,
    /// Which of our addresses to advertise, when the operator has said.
    /// `None` means detect it at bind time.
    advertise: Option<std::net::IpAddr>,
    signing: closure_sync::SigningKey,
    session: closure_sync::SyncSession,
    peers: Vec<Peer>,
    /// Bound listener, once a shell has asked to accept connections.
    listener: Option<std::sync::Arc<std::net::TcpListener>>,
}

/// Which of this host's addresses a peer should be told to dial.
///
/// An explicit choice wins — on a machine that is on a LAN *and* a
/// mesh VPN, both addresses are "local" and only the operator knows
/// which one the peer can route to. Otherwise the bind address is the
/// answer, except when it is the unspecified one (`0.0.0.0` / `::`),
/// which means "every interface" to `bind` and nothing at all to
/// `connect`; then we ask the routing table which source address it
/// would use to leave this host.
fn advertised_ip(bind: std::net::IpAddr, advertise: Option<std::net::IpAddr>) -> std::net::IpAddr {
    if let Some(ip) = advertise {
        return ip;
    }
    if !bind.is_unspecified() {
        return bind;
    }
    detect_outbound_ip().unwrap_or(match bind {
        std::net::IpAddr::V4(_) => std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        std::net::IpAddr::V6(_) => std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
    })
}

/// The source address the kernel would use to reach the outside world.
///
/// A connected UDP socket is the portable way to ask: `connect` on a
/// datagram socket only fixes the peer and picks a route — no packet
/// is sent, nothing is resolved, and no name server is consulted, so
/// this stays honest on an offline machine (it simply returns `None`).
/// The target is in TEST-NET-3 (RFC 5737), an address reserved for
/// documentation precisely so that nothing real is ever implied.
fn detect_outbound_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("203.0.113.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

impl std::fmt::Debug for SyncApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written so the signing key never reaches a log line;
        // `finish_non_exhaustive` says out loud that fields are held
        // back rather than forgotten.
        f.debug_struct("SyncApp")
            .field("name", &self.name)
            .field("addr", &self.addr)
            .field("bind", &self.bind)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}

impl SyncApp {
    /// A fresh identity for replica `name`, listening on `addr`.
    ///
    /// The keypair is generated per session: pairing is an explicit act
    /// in both directions, so there is nothing to persist until the
    /// user decides there is.
    #[must_use]
    pub fn new(name: &str, addr: std::net::SocketAddr) -> Self {
        Self::with_bind(name, addr, None)
    }

    /// An identity that binds one address and advertises another.
    ///
    /// The two are the same on a single-homed host and different on
    /// every interesting one: a machine reachable over a mesh VPN binds
    /// `0.0.0.0` and hands out its `100.x` address, because that is the
    /// one its peer can route to. Passing `None` for `advertise` asks
    /// for detection — see [`advertised_ip`].
    ///
    /// The ticket is correct immediately, before anything is bound:
    /// pairing starts by pasting it, and a ticket that only becomes
    /// true after the user finds the "listen" button is a trap.
    #[must_use]
    pub fn with_bind(
        name: &str,
        bind: std::net::SocketAddr,
        advertise: Option<std::net::IpAddr>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            addr: std::net::SocketAddr::new(advertised_ip(bind.ip(), advertise), bind.port()),
            bind,
            advertise,
            signing: closure_sync::generate_key(),
            session: closure_sync::SyncSession::new(name),
            peers: Vec::new(),
            listener: None,
        }
    }

    /// Point the (not yet opened) socket somewhere else.
    ///
    /// A shell reads `config.org` after the state exists, so the
    /// addresses arrive late; the keypair must survive that, which is
    /// why this is a setter and not a fresh [`Self::with_bind`] — a new
    /// identity would invalidate a ticket the user may already have
    /// handed over. Once a listener is open the bind address is a fact
    /// rather than a preference and only the advertised one moves.
    pub fn rebind(&mut self, bind: std::net::SocketAddr, advertise: Option<std::net::IpAddr>) {
        self.advertise = advertise;
        if self.listener.is_none() {
            self.bind = bind;
        }
        self.addr =
            std::net::SocketAddr::new(advertised_ip(self.bind.ip(), advertise), self.bind.port());
    }

    /// Our replica name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Our verifying key — what a peer checks our frames against.
    #[must_use]
    pub fn public_key(&self) -> closure_sync::VerifyingKey {
        self.signing.verifying_key()
    }

    /// Our signing key, for a shell that is about to dial out.
    #[must_use]
    pub const fn signing_key(&self) -> &closure_sync::SigningKey {
        &self.signing
    }

    /// The one line to hand to whoever you want to sync with.
    #[must_use]
    pub fn ticket(&self) -> String {
        closure_sync::SyncTicket {
            addr: self.addr,
            pubkey: self.public_key(),
        }
        .encode()
    }

    /// Peers we have tickets for.
    #[must_use]
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// Keys we trust to sign frames — what the transport is given.
    #[must_use]
    pub fn trusted_keys(&self) -> Vec<closure_sync::VerifyingKey> {
        self.peers.iter().map(|p| p.key).collect()
    }

    /// Our replica, for reading and for shipping to a peer.
    #[must_use]
    pub const fn session(&self) -> &closure_sync::SyncSession {
        &self.session
    }

    /// Add a peer from a pasted ticket.
    ///
    /// # Errors
    ///
    /// The decode failure as a message, or a refusal when the ticket is
    /// our own — pointing a vault at itself would merge it with itself
    /// forever. Pasting the same peer twice is a no-op, not an error.
    pub fn add_peer(&mut self, ticket: &str) -> Result<(), String> {
        let parsed = closure_sync::SyncTicket::decode(ticket).map_err(|e| format!("{e}"))?;
        if parsed.pubkey == self.public_key() {
            return Err("that is our own ticket — hand it to the other peer".to_owned());
        }
        if self.peers.iter().any(|p| p.key == parsed.pubkey) {
            return Ok(());
        }
        self.peers.push(Peer {
            addr: parsed.addr,
            key: parsed.pubkey,
            state: PeerState::Known,
        });
        Ok(())
    }

    /// Record how the last round with `addr` went.
    pub fn record_outcome(&mut self, addr: std::net::SocketAddr, result: Result<usize, String>) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.addr == addr) {
            peer.state = match result {
                Ok(blocks) => PeerState::Synced { blocks },
                Err(e) => PeerState::Failed(e),
            };
        }
    }

    /// Fold the vault's current state into our replica.
    pub fn snapshot(&mut self, shell: &Shell) {
        for (_, doc) in shell.vault.iter() {
            self.session.record_local(doc);
        }
    }

    /// Merge a peer's replica into ours, returning every divergence
    /// the automatic LWW would otherwise have resolved silently.
    pub fn merge_session(
        &mut self,
        other: &closure_sync::SyncSession,
    ) -> Vec<closure_crdt::FieldConflict> {
        self.session.receive_with_conflicts(other.outgoing())
    }

    /// Blocks our replica knows about.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.session.block_ids().count()
    }

    /// Write the converged replica back into the vault's files,
    /// returning how many fields actually changed.
    ///
    /// The replica converging is only half of a sync: until the merged
    /// state reaches the org files nothing the user can see has
    /// changed, and it is the vault — not the replica — that gets
    /// committed to git and opened in Emacs.
    ///
    /// Only blocks the vault already has are touched. A headline that
    /// exists solely in the peer's replica would need a file to live
    /// in and a place in the tree, and guessing either is worse than
    /// leaving it: convergence of *known* blocks is the promise.
    /// Writes go through the kernel commands like every other edit
    /// (I8), and a field that already matches is skipped, so a
    /// repeated round is a no-op rather than a rewrite of every file.
    pub fn apply_to_vault(&self, shell: &mut Shell) -> usize {
        let mut applied = 0usize;
        let ids: Vec<closure_core::BlockId> = self.session.block_ids().cloned().collect();
        for id in ids {
            let Some((headline, _)) = shell.vault.find_by_id(&id) else {
                continue;
            };
            let current_title = headline.title().to_owned();
            let current_body = headline.body_text().to_owned();
            if let Some(title) = self.session.title_of(&id)
                && title != current_title
                && shell.rename_headline(&id, title).is_ok()
            {
                applied += 1;
            }
            if let Some(body) = self.session.body_of(&id)
                && body != current_body
                && shell.set_body(&id, &body).is_ok()
            {
                applied += 1;
            }
        }
        applied
    }

    /// Bind a listener so a peer can dial *in*.
    ///
    /// Pairing needs both directions: handing over a ticket is useless
    /// if nothing answers at the address it names. Binding port 0 asks
    /// the OS for a free one, and the ticket is rewritten to the real
    /// address afterwards — a ticket naming a port nothing listens on
    /// is worse than no ticket.
    ///
    /// # Errors
    ///
    /// The bind failure as a message.
    pub fn listen(&mut self) -> Result<std::net::SocketAddr, String> {
        if let Some(listener) = &self.listener {
            return listener.local_addr().map_err(|e| format!("{e}"));
        }
        let listener = std::net::TcpListener::bind(self.bind).map_err(|e| format!("{e}"))?;
        let bound = listener.local_addr().map_err(|e| format!("{e}"))?;
        self.bind = bound;
        // The port is only knowable after the bind when it was 0, and
        // the address a peer dials is never the unspecified one we may
        // have just bound — so the ticket is recomputed from both.
        self.addr =
            std::net::SocketAddr::new(advertised_ip(bound.ip(), self.advertise), bound.port());
        self.listener = Some(std::sync::Arc::new(listener));
        Ok(bound)
    }

    /// The address our ticket names.
    #[must_use]
    pub const fn ticket_addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    /// The socket we bind (or have bound) — where we listen, as opposed
    /// to [`Self::ticket_addr`], which is where a peer dials.
    #[must_use]
    pub const fn bind_addr(&self) -> std::net::SocketAddr {
        self.bind
    }

    /// Whether it is safe to accept an inbound round right now.
    ///
    /// The transport verifies every frame's signature, but a peer is
    /// only *authenticated* against a trusted set; an empty one is
    /// integrity-only mode, where any self-consistent signature is
    /// accepted. On loopback that set being empty means "anyone on this
    /// machine", which is the user. On `0.0.0.0` it would mean anyone
    /// who can reach the port — and an inbound round writes titles and
    /// bodies into the vault. So a network-facing listener answers only
    /// once it has been given someone to trust.
    ///
    /// # Errors
    ///
    /// The reason to show the user, naming the thing they have to do.
    pub fn inbound_ready(&self) -> Result<(), String> {
        if self.bind.ip().is_loopback() || !self.peers.is_empty() {
            return Ok(());
        }
        Err(format!(
            "listening on {} but trusting nobody — paste your peer's ticket first, \
             or nothing that dials in can be told apart from a stranger",
            self.bind
        ))
    }

    /// The bound listener, for a shell that wants to accept on its own
    /// thread.
    #[must_use]
    pub fn listener(&self) -> Option<std::sync::Arc<std::net::TcpListener>> {
        self.listener.clone()
    }
}

/// One entry in the activity rail: a pane the user can reach with one
/// click, named, keyed and carrying whatever it currently holds.
///
/// The rail exists because the `g`-prefixed chords were the *only* door
/// into the subsystems: pairing, in particular, had no clickable entry
/// point anywhere in any shell, so P2P was undiscoverable to anyone who
/// had not read the keymap. Derived data rather than render-time
/// assembly, so every shell offers the same destinations in the same
/// order with the same chords (I4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// Stable identifier (`"peers"`, `"sniffer"`, …) — what tests and
    /// shells match on, never the label.
    pub id: &'static str,
    /// One glyph, for a collapsed rail. Deliberately not a Nerd Font
    /// codepoint: the TUI and the web tier render this too.
    pub icon: &'static str,
    /// What it is, in words. A glyph alone is a guessing game.
    pub label: &'static str,
    /// Registry command a click runs (I8).
    pub command: &'static str,
    /// Chord bound to that command in the active mode — `None` only if
    /// the mode binds none, which `rail.rs` forbids.
    pub chord: Option<&'static str>,
    /// Surface the command opens, so the rail can mark the current one.
    pub surface: ModalSurface,
    /// Live count, when there is something to count. `None` renders no
    /// badge at all rather than a `0`.
    pub badge: Option<String>,
    /// Whether that count is work waiting on the user (unresolved
    /// conflicts, blocked flows) rather than mere activity.
    pub urgent: bool,
    /// Whether this is the surface currently open.
    pub active: bool,
}

/// The rail's fixed running order, as
/// `(id, icon, label, command, surface)`.
///
/// A table rather than a builder: the order is what the user's muscle
/// memory learns, so it is one thing to read and one thing to change.
/// Icons are plain Unicode — the TUI and the web tier paint these too,
/// and a Nerd Font glyph would be a box in both.
const RAIL: &[(&str, &str, &str, &str, ModalSurface)] = &[
    ("outline", "⌂", "Outline", "browse", ModalSurface::Browse),
    ("agenda", "◷", "Agenda", "agenda", ModalSurface::Agenda),
    ("blocks", "⌗", "Blocks", "block-list", ModalSurface::Blocks),
    ("graph", "⁂", "Graph", "graph", ModalSurface::Graph),
    (
        "backlinks",
        "⟵",
        "Backlinks",
        "backlinks",
        ModalSurface::Backlinks,
    ),
    ("journal", "≡", "Journal", "journal", ModalSurface::Journal),
    ("cron", "⏱", "Jobs", "cron", ModalSurface::Cron),
    ("peers", "⇄", "Peers", "sync", ModalSurface::Sync),
    ("sniffer", "⇅", "Network", "sniffer", ModalSurface::Sniffer),
    ("assistant", "✦", "Assistant", "llm", ModalSurface::Llm),
    (
        "conflicts",
        "⚠",
        "Conflicts",
        "conflicts",
        ModalSurface::Conflicts,
    ),
    ("palette", "⌘", "Commands", "palette", ModalSurface::Palette),
];

/// How loudly a status-bar indicator should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorLevel {
    /// Nothing happening; muted.
    Idle,
    /// Doing something, or a permission is granted; accented.
    Active,
    /// Something is waiting on the user; warning colour.
    Warn,
}

/// One item in the bottom-right status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indicator {
    /// Stable identifier (`"sniffer"`, `"llm"`, …) — what tests and
    /// shells match on, never the label.
    pub id: &'static str,
    /// Short text shown in the bar, usually a glyph plus a count.
    pub label: String,
    /// Longer explanation for a tooltip or the status line.
    pub tooltip: String,
    /// How loudly it should read.
    pub level: IndicatorLevel,
    /// Command a click runs, when there is one.
    pub command: Option<&'static str>,
    /// Chord bound to that command in the active mode.
    pub chord: Option<&'static str>,
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
            let mut body = closure_org::escape_body(&self.body_buf);
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
                let folded = headline_is_folded(h);
                if self.query.is_empty() {
                    if let Some(limit) = hide_below {
                        if h.level() > limit {
                            continue;
                        }
                        hide_below = None;
                    }
                    if folded {
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
                            folded,
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
            body: closure_org::unescape_body(h.body_text()),
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
                        Ok(_) => self.status = format!("captured: {}", self.capture_buf),
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
    /// The vim-style `:` command line.
    Ex,
    /// org-edit-special: one source block, on its own, in its own
    /// language. `C-Enter` writes it back, `Esc` discards.
    EditBlock,
    /// Pairing and collaboration: our ticket, the peers we have, and
    /// what the last round with each of them did.
    Sync,
    /// The link graph: hubs, orphans, dead links.
    Graph,
    /// The recorded command journal.
    Journal,
    /// Scheduled jobs declared in the vault.
    Cron,
    /// The assistant: a transcript, a question field, and what the
    /// vault's `config.org` says is behind it.
    Llm,
    /// The whole file, as one buffer — the editor view. `C-Enter` (or
    /// `:w`) writes it back, `Esc` abandons it.
    EditFile,
}

/// Which of the two shapes of the shell you are working in.
///
/// The GUI grew Notion-shaped: rows you click, a detail pane, a rail.
/// That is the right shape for a mouse and the wrong one for someone who
/// lives in Doom, where the *file* is the interface and the whole frame
/// is a buffer. Both are the same vault and the same commands; what
/// changes is what fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// The outline, the detail pane and the rail: rows you click.
    Clickable,
    /// The file itself, in one full-window buffer.
    Editor,
}

impl ViewMode {
    /// The view an input mode starts in.
    ///
    /// A modal mode is a statement about how you work: Vim, Doom and
    /// Helix users edit files. Notion and Emacs mode start on the
    /// outline — Emacs included, because its bindings here are the
    /// launcher's `C-c` chords rather than evil's.
    #[must_use]
    pub const fn for_input(mode: InputMode) -> Self {
        match mode {
            InputMode::Vim | InputMode::Doom | InputMode::Helix => Self::Editor,
            InputMode::Notion | InputMode::Emacs => Self::Clickable,
        }
    }
}

impl ModalSurface {
    /// Whether this surface is a text buffer rather than a pane.
    ///
    /// A buffer takes the whole window: the outline, the rail and the
    /// detail pane get out of its way, the way `org-edit-special` gets
    /// its own frame in Emacs. Editing a body in a third of the window,
    /// beside a list of the other headlines, is a preview — not a
    /// place to write in.
    #[must_use]
    pub const fn is_editor(self) -> bool {
        matches!(self, Self::EditBody | Self::EditBlock | Self::EditFile)
    }
}

/// One turn of the assistant transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTurn {
    /// Whether the user said it (rather than the model).
    pub from_user: bool,
    /// What was said.
    pub text: String,
}

/// What the vault's `config.org` says about the assistant, and whether
/// it can actually be used right now.
///
/// "Configured" and "usable" are different questions: the key lives in
/// an environment variable, so a perfectly good config still fails at
/// 2am when the variable is not exported. Both are reported, and the
/// key's *value* never is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmStatus {
    /// Whether a question can be sent right now.
    pub ready: bool,
    /// Provider name from the config, if any.
    pub provider: Option<String>,
    /// Model identifier from the config, if any.
    pub model: Option<String>,
    /// Endpoint URL, when the config pins one.
    pub endpoint: Option<String>,
    /// A sentence for the pane: what is set, or what to add.
    pub detail: String,
}

/// Where an org-edit-special session came from, and therefore how it
/// writes back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpecialOrigin {
    /// A block reached from the Blocks list: it belongs to a file, so
    /// the writeback goes through the kernel's span-preserving
    /// `set_block_content` and the rest of the file stays untouched
    /// bytes (I1).
    File {
        /// File the block lives in.
        path: std::path::PathBuf,
        /// Index of the block within that file.
        index: usize,
    },
    /// A block reached from the body editor: it lives inside the
    /// buffer being edited, so it is spliced back into that buffer and
    /// the ordinary body commit carries it to disk. Writing through
    /// the file here would mean persisting the body twice, from two
    /// different states.
    Body {
        /// Byte range of the block's content within the body buffer.
        range: std::ops::Range<usize>,
        /// The body buffer as it was when the session opened.
        buffer: String,
        /// Cursor to restore into that buffer afterwards.
        cursor: usize,
    },
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

/// Vim's three character classes. `w`/`b`/`e` and the `iw` object stop
/// at every class boundary; the `W`/`B`/`E`/`iW` variants fold `Punct`
/// into `Word` so only blanks delimit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Blank,
    Word,
    Punct,
}

/// Classify `c` for a small (`big == false`) or big word motion.
fn char_class(c: char, big: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// How a motion's target combines with the cursor into an operator
/// range — vim's exclusive/inclusive/linewise distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionKind {
    /// `w`, `b`, `0`, `$`: the target byte itself stays.
    Exclusive,
    /// `e`, `f`, `t`: the char under the target is taken too.
    Inclusive,
    /// `j`, `k`, `G`, `gg`, `ip`: whole lines, newline included.
    Linewise,
}

/// What the editor is waiting for mid-chord. `d` alone is not an edit;
/// it is a promise that the next stroke names a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Nothing outstanding — the next stroke is a whole command.
    None,
    /// An operator (`d`/`c`/`y`/`>`/`<`) awaiting a motion, a text
    /// object, or its own doubled key (`dd`).
    Op(char),
    /// `g` typed, alone (`gg`) or under an operator (`dgg`).
    G(Option<char>),
    /// `i`/`a` typed; the next stroke names the object (`iw`, `a"`).
    /// `op == None` is Visual mode, where the object sets the selection.
    Obj { op: Option<char>, around: bool },
    /// `f`/`F`/`t`/`T` typed; the next stroke is the target char.
    Find { op: Option<char>, kind: char },
    /// `r` typed; the next stroke is the replacement char.
    Replace,
    /// `"` typed; the next stroke names the register the following
    /// yank/delete/paste uses.
    Register,
    /// `m` typed; the next stroke names the mark to set.
    Mark,
    /// `` ` `` or `'` typed; the next stroke names the mark to jump to.
    /// `linewise` is the `'` form, which lands on the line's first
    /// non-blank and makes an operator take whole lines.
    JumpMark { op: Option<char>, linewise: bool },
    /// `q` typed with nothing recording; the next stroke names the
    /// register to record into.
    RecordMacro,
    /// `@` typed; the next stroke names the macro to replay.
    RunMacro,
}

impl Pending {
    /// The stroke a shell should echo as "mid-chord", if any.
    const fn stroke(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::Op(c) => Some(c),
            Self::G(_) => Some('g'),
            Self::Obj { around: true, .. } => Some('a'),
            Self::Obj { around: false, .. } => Some('i'),
            Self::Find { kind, .. } => Some(kind),
            Self::Replace => Some('r'),
            Self::Register => Some('"'),
            Self::Mark => Some('m'),
            Self::JumpMark { linewise: true, .. } => Some('\''),
            Self::JumpMark {
                linewise: false, ..
            } => Some('`'),
            Self::RecordMacro => Some('q'),
            Self::RunMacro => Some('@'),
        }
    }
}

/// One step of a recorded macro: a modal stroke, or a run of text
/// typed while the macro was in INSERT.
///
/// Two kinds rather than one because a macro spans modes: `ciwfoo<Esc>`
/// is three strokes, then three characters that never reach
/// [`BodyEditor::modal_key`] at all.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MacroStep {
    /// A Normal/Visual stroke, replayed through the modal dispatch.
    Key(String),
    /// Text typed in INSERT, replayed verbatim.
    Text(String),
}

/// Lines a `PageUp`/`PageDown` stroke moves when the editor is asked
/// without a viewport. A shell that knows its own height should call
/// [`BodyEditor::page`] with it instead.
const PAGE_LINES: usize = 20;

/// The one-line vocabulary hint a shell paints beside the mode chip.
///
/// Shared so the terminal and the GUI advertise the same editor: a
/// chord named here works in both (I4). Keep it in step with
/// [`BodyEditor::modal_key`] — a hint that outlives its command is the
/// same lie as a dead chord.
#[must_use]
pub const fn editor_hint(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Insert => {
            "type · TAB tempo (<s…) · C-n complete · C-a/e/k/y readline · Esc → NORMAL"
        }
        EditorMode::Normal => {
            "w b e f t % move · diw caw dis dt, gUiw operate · . repeat · dd yy Y p · \
             \"a reg · ma `a mark · qa @a macro · /pat n N * # · C-a/C-x · C-d/C-u/C-f/C-b · \
             A I O R J r gv gi · v V · Esc"
        }
        EditorMode::Visual | EditorMode::VisualLine => {
            "motions + iw aw i( a\" extend · d c y > < operate · o swap ends · Esc → NORMAL"
        }
    }
}

/// A modal multi-line text editor with a real cursor — the state
/// behind the org-edit-special surface.
///
/// Pure and unicode-safe (the cursor is a byte offset kept on a `char`
/// boundary); a shell paints `text()` + `cursor_line_col()` and feeds
/// keys through [`ModalApp`].
// The flags are independent latches on the editing session (a count was
// typed, the register is linewise, an Insert burst is armed, a change is
// recording, a replay is running); grouping them would only add
// indirection to what is already one cohesive editor state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct BodyEditor {
    buf: String,
    cursor: usize,
    /// The column vertical motion is *trying* to be in — vim's
    /// `curswant`. A `j` through a two-character line clamps to it and
    /// keeps wanting the original, so the next `j` comes back out to
    /// where you were reading. Cleared by anything horizontal.
    wanted_col: Option<usize>,
    mode: EditorMode,
    /// Visual-mode selection anchor (byte offset).
    anchor: usize,
    /// The yank/kill register shared by vim (`y`/`d`/`p`) and the
    /// readline chords (`C-k`/`C-u`/`C-w`/`C-y`).
    register: String,
    /// Whether the register holds whole lines (`dd`/`yy` → `p` pastes
    /// below the current line).
    linewise: bool,
    /// What the in-progress chord is still waiting for (`d` of `diw`).
    pending: Pending,
    /// Pending vim count in Normal/Visual modes (0 = none).
    count: usize,
    /// Count typed *before* the operator (the `2` of `2d3w`), so both
    /// halves multiply the way vim multiplies them.
    op_count: usize,
    /// Whether the chord being resolved carried an explicit count —
    /// `G` and `gg` mean "last"/"first" line only without one.
    count_given: bool,
    /// Last `f`/`F`/`t`/`T` as `(kind, target)`, for `;` and `,`.
    last_find: Option<(char, char)>,
    /// Strokes of the command being typed, kept until it is known to be
    /// a change worth remembering for `.`.
    scratch: Vec<String>,
    /// Text typed during the Insert session of the change in progress.
    insert_text: String,
    /// Whether the live Insert session belongs to a recorded change.
    recording_insert: bool,
    /// The last change as `(strokes, inserted text)` — what `.` replays.
    last_change: Option<(Vec<String>, String)>,
    /// True while `.` is replaying, so the replay records nothing.
    replaying: bool,
    /// Bumped by every [`Self::checkpoint`]: the signal that a command
    /// actually changed the buffer, and so is a change `.` can repeat.
    edit_seq: u64,
    /// Editor-local undo snapshots (buffer, cursor), newest last.
    undo_stack: Vec<(String, usize)>,
    /// Redo snapshots cleared by any fresh edit.
    redo_stack: Vec<(String, usize)>,
    /// Armed on every INSERT entry: the first buffer-changing INSERT
    /// edit takes one checkpoint, so the whole burst undoes as a unit
    /// (vim rule, G4).
    insert_armed: bool,
    /// Named registers `a`–`z` with their own linewise flags. The
    /// unnamed register stays [`Self::register`], so every command that
    /// does not say otherwise behaves exactly as it always did.
    registers: std::collections::BTreeMap<char, (String, bool)>,
    /// The register named by a `"x` prefix, consumed by the next
    /// yank/delete/paste. Uppercase means "append".
    target_register: Option<char>,
    /// Marks `a`–`z` as byte offsets. Edits do not move them, so a
    /// stale mark is clamped to the buffer on use rather than trusted.
    marks: std::collections::BTreeMap<char, usize>,
    /// Recorded macros `a`–`z`.
    macros: std::collections::BTreeMap<char, Vec<MacroStep>>,
    /// The register being recorded into and the steps taken so far.
    recording: Option<(char, Vec<MacroStep>)>,
    /// The macro `@@` repeats.
    last_macro: Option<char>,
    /// True while a macro replays, so the replay records nothing.
    running_macro: bool,
    /// The last Visual range as `(anchor, cursor, mode)` — what `gv`
    /// puts back.
    last_visual: Option<(usize, usize, EditorMode)>,
    /// Where INSERT was last left, which is where `gi` resumes.
    last_insert: Option<usize>,
    /// REPLACE mode (`R`): typing overwrites the char under the cursor
    /// instead of pushing it right.
    replacing: bool,
    /// The open search line as `(forward, pattern so far)`.
    search_input: Option<(bool, String)>,
    /// The operator armed when the search line opened (`d/foo`).
    search_op: Option<char>,
    /// The last search as `(pattern, forward)` — what `n`/`N` repeat.
    last_search: Option<(String, bool)>,
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
            wanted_col: None,
            mode: EditorMode::Insert,
            anchor: 0,
            register: String::new(),
            linewise: false,
            pending: Pending::None,
            count: 0,
            op_count: 0,
            count_given: false,
            last_find: None,
            scratch: Vec::new(),
            insert_text: String::new(),
            recording_insert: false,
            last_change: None,
            replaying: false,
            edit_seq: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            insert_armed: false,
            registers: std::collections::BTreeMap::new(),
            target_register: None,
            marks: std::collections::BTreeMap::new(),
            macros: std::collections::BTreeMap::new(),
            recording: None,
            last_macro: None,
            running_macro: false,
            last_visual: None,
            last_insert: None,
            replacing: false,
            search_input: None,
            search_op: None,
            last_search: None,
        }
    }

    /// Load `text`, cursor at the end, Insert mode (the edit-body flow).
    pub fn load(&mut self, text: String) {
        self.load_in(text, EditorMode::Insert);
    }

    /// Load `text` and land in `mode`.
    ///
    /// Where the cursor goes follows from the mode rather than being a
    /// second decision: INSERT opens at the end, the way appending to a
    /// note does; NORMAL opens at the top, the way opening a buffer
    /// does. A VISUAL entry mode makes no sense with no selection yet,
    /// so it starts at the top too and collapses the anchor onto the
    /// cursor.
    pub fn load_in(&mut self, text: String, mode: EditorMode) {
        self.cursor = if mode == EditorMode::Insert {
            text.len()
        } else {
            0
        };
        self.buf = text;
        self.mode = mode;
        self.anchor = self.cursor;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.insert_armed = true;
        self.reset_buffer_state();
    }

    /// Drop everything that describes *this* buffer's positions.
    ///
    /// Marks, the last selection and the last insert point are byte
    /// offsets into the text that was here; carried into the next
    /// headline they would point at unrelated words. Registers, macros
    /// and the last search pattern are deliberately kept — those are
    /// global in vim, and the whole point of yanking in one note is
    /// pasting it into another.
    fn reset_buffer_state(&mut self) {
        self.marks.clear();
        self.last_visual = None;
        self.last_insert = None;
        self.replacing = false;
        self.search_input = None;
        self.search_op = None;
        self.recording = None;
        self.target_register = None;
        self.pending = Pending::None;
        self.count = 0;
        self.op_count = 0;
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
        self.reset_buffer_state();
    }

    /// Switch to Normal (from Insert `Esc`).
    pub fn to_normal(&mut self) {
        // A macro that entered INSERT has to leave it again on replay,
        // and `Esc` never reaches `modal_key` — the shells answer it
        // themselves. So the exit is recorded here.
        if self.mode == EditorMode::Insert {
            self.last_insert = Some(self.cursor);
            self.push_macro_step(MacroStep::Key("escape".to_owned()));
        }
        self.replacing = false;
        // Leaving Insert closes a recorded change: the strokes that
        // opened it plus everything typed inside become one `.` unit,
        // exactly as vim scopes a change.
        if self.recording_insert {
            self.recording_insert = false;
            if self.scratch.is_empty() {
                // An Insert session nobody opened with a command (the
                // editor loads in Insert) is not a repeatable change.
                self.insert_text.clear();
            } else {
                self.last_change = Some((
                    std::mem::take(&mut self.scratch),
                    std::mem::take(&mut self.insert_text),
                ));
            }
        }
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

    /// Insert `c` at the cursor — or, in REPLACE mode, overwrite the
    /// char under it. A newline is never overwritten: `R` past the line
    /// end appends, it does not weld two lines together.
    pub fn insert_char(&mut self, c: char) {
        self.insert_guard();
        self.record_typed(c.encode_utf8(&mut [0u8; 4]));
        if self.replacing
            && let Some(under) = self.char_at(self.cursor).filter(|u| *u != '\n')
        {
            self.buf
                .replace_range(self.cursor..self.cursor + under.len_utf8(), "");
        }
        self.buf.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert `s` at the cursor, cursor after it.
    pub fn insert_str(&mut self, s: &str) {
        self.insert_guard();
        self.record_typed(s);
        self.buf.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Note text typed inside a change, so `.` can retype it.
    fn record_typed(&mut self, s: &str) {
        if self.recording_insert && !self.replaying {
            self.insert_text.push_str(s);
        }
        self.push_macro_step(MacroStep::Text(s.to_owned()));
    }

    /// Delete the char before the cursor (Insert Backspace).
    pub fn backspace(&mut self) {
        if let Some((i, _)) = self.buf[..self.cursor].char_indices().next_back() {
            self.insert_guard();
            if self.recording_insert && !self.replaying {
                self.insert_text.pop();
            }
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

    /// Move `lines` whole lines up or down — the page motion, sized by
    /// the caller because only the shell knows its viewport height.
    pub fn page(&mut self, down: bool, lines: usize) {
        let line = if down {
            self.cursor_line() + lines
        } else {
            self.cursor_line().saturating_sub(lines)
        };
        self.cursor = self.line_col_offset(line, 0);
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

    /// Replace a byte range of the buffer with `text`, parking the
    /// cursor after it.
    ///
    /// The seam an input method needs: a composed character arrives as
    /// a replacement over a range, not as a keypress. Offsets are
    /// clamped to the buffer and snapped down to char boundaries, so a
    /// bad range from the platform cannot panic (I5).
    pub fn replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        let lo = self.snap_boundary(range.start.min(self.buf.len()));
        let hi = self.snap_boundary(range.end.clamp(lo, self.buf.len()));
        if lo == hi && text.is_empty() {
            return;
        }
        self.checkpoint();
        self.buf.replace_range(lo..hi, text);
        self.cursor = lo + text.len();
        self.last_insert = Some(self.cursor);
    }

    /// Round `at` down to the nearest char boundary.
    fn snap_boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.buf.len());
        while at > 0 && !self.buf.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// Redo the last editor-local undo (Normal C-r).
    pub fn redo_local(&mut self) {
        if let Some((buf, cur)) = self.redo_stack.pop() {
            self.undo_stack.push((self.buf.clone(), self.cursor));
            self.buf = buf;
            self.cursor = cur;
        }
    }

    /// One Normal/Visual-mode key.
    ///
    /// The vocabulary is vim's real grammar rather than a handful of
    /// hard-coded pairs: `[count]operator[count]{motion|text-object}`,
    /// so `diw`, `2d3w`, `ca"`, `dt,`, `yG` and `>>` all compose from
    /// the same three tables ([`Self::motion`], [`Self::text_object`],
    /// [`Self::apply_operator`]).
    ///
    /// `Esc` leaves Visual or clears an in-progress chord; the *caller*
    /// cancels the edit on `Esc` when [`Self::pending_stroke`] is clear
    /// and the mode is Normal.
    pub fn modal_key(&mut self, key: &str) {
        // `.` is not itself a change and must never be recorded, or it
        // would repeat itself.
        if key == "." && self.pending == Pending::None && self.search_input.is_none() {
            return self.repeat_change();
        }
        // The strokes that drive the recorder are not part of what it
        // records: `qa` … `q` would otherwise replay as "stop, start".
        let recorder_stroke = self.pending == Pending::RecordMacro
            || (key == "q" && self.pending == Pending::None && self.recording.is_some());
        if !recorder_stroke {
            self.push_macro_step(MacroStep::Key(key.to_owned()));
        }
        if !self.replaying {
            self.scratch.push(key.to_owned());
        }
        let before = self.edit_seq;
        self.dispatch_modal_key(key);
        if !self.replaying {
            self.record_step(before);
        }
    }

    /// Add one step to the macro being recorded, if one is.
    fn push_macro_step(&mut self, step: MacroStep) {
        if self.running_macro || self.replaying {
            return;
        }
        let Some((_, steps)) = self.recording.as_mut() else {
            return;
        };
        // Consecutive typed characters coalesce into one run, so a
        // replay inserts them as a single edit.
        if let (MacroStep::Text(add), Some(MacroStep::Text(tail))) = (&step, steps.last_mut()) {
            tail.push_str(add);
            return;
        }
        steps.push(step);
    }

    /// Decide what the stroke just dispatched means for `.`.
    ///
    /// A command that changed the buffer is remembered; one that only
    /// moved the cursor is forgotten; a chord still mid-flight is kept
    /// so `d` + `i` + `w` commits as the single change `diw`.
    fn record_step(&mut self, before: u64) {
        if self.mode == EditorMode::Insert {
            // The change continues into the typing; `to_normal` commits.
            self.recording_insert = true;
            self.insert_text.clear();
        } else if self.pending == Pending::None && self.count == 0 && self.op_count == 0 {
            // A bare count is still mid-command — `3` of `3x` must stay
            // in the scratch or `.` would repeat a countless `x`.
            if self.edit_seq == before {
                self.scratch.clear();
            } else {
                self.last_change = Some((std::mem::take(&mut self.scratch), String::new()));
            }
        }
    }

    /// `.`: replay the last change at the cursor.
    fn repeat_change(&mut self) {
        let Some((keys, text)) = self.last_change.clone() else {
            return;
        };
        self.replaying = true;
        for k in &keys {
            self.dispatch_modal_key(k);
        }
        if !text.is_empty() {
            self.insert_str(&text);
        }
        if self.mode == EditorMode::Insert {
            self.to_normal();
        }
        self.replaying = false;
    }

    /// The modal vocabulary proper, with no `.`-recording around it.
    fn dispatch_modal_key(&mut self, key: &str) {
        // The search line owns every key while it is open — a pattern
        // is text, not a chord.
        if self.search_input.is_some() {
            return self.search_key(key);
        }
        let was_visual = matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine)
            .then_some((self.anchor, self.cursor, self.mode));
        // A chord waiting on a literal char argument consumes any key,
        // digits included — `r5` and `df3` are not counts.
        match self.pending {
            Pending::Replace => self.finish_replace(key),
            Pending::Find { op, kind } => self.finish_find(op, kind, key),
            Pending::Obj { op, around } => self.finish_object(op, around, key),
            Pending::G(op) => self.finish_g(op, key),
            Pending::Op(op) => self.after_operator(op, key),
            Pending::Register => self.finish_register(key),
            Pending::Mark => self.finish_mark(key),
            Pending::JumpMark { op, linewise } => self.finish_mark_jump(op, linewise, key),
            Pending::RecordMacro => self.finish_record(key),
            Pending::RunMacro => self.finish_run_macro(key),
            Pending::None => {
                if !self.take_digit(key) {
                    self.normal_key(key);
                }
            }
        }
        // Whatever ended a Visual selection — `Esc`, an operator, a
        // mode switch — is what `gv` puts back.
        if let Some(last) = was_visual
            && !matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine)
        {
            self.last_visual = Some(last);
        }
    }

    /// Accumulate a count digit; `true` when the key was one. A leading
    /// `0` is the line-start motion, not a count.
    fn take_digit(&mut self, key: &str) -> bool {
        let Some(d) = key.chars().next().filter(|_| key.len() == 1) else {
            return false;
        };
        let Some(d) = d.to_digit(10).and_then(|d| usize::try_from(d).ok()) else {
            return false;
        };
        if d == 0 && self.count == 0 {
            return false;
        }
        self.count = self.count * 10 + d;
        true
    }

    /// A whole command with no chord outstanding.
    // One arm per vim key: splitting the vocabulary would hide the
    // mode dispatch (same precedent as `run_command`).
    #[allow(clippy::too_many_lines)]
    fn normal_key(&mut self, key: &str) {
        let visual = matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine);
        match key {
            // --- Operators. In Visual they act on the selection now;
            // in Normal they arm a pending chord.
            "d" | "x" if visual => self.visual_operator('d'),
            "c" | "s" if visual => self.visual_operator('c'),
            "y" if visual => self.visual_operator('y'),
            ">" if visual => self.visual_operator('>'),
            "<" if visual => self.visual_operator('<'),
            // In Visual these are case changes, not undo — the `u` of
            // `viwu`. Normal-mode `u` stays undo, further down.
            "u" | "U" | "~" if visual => {
                self.visual_operator(key.chars().next().unwrap_or('u'));
            }
            "D" | "X" if visual => self.visual_linewise_operator('d'),
            "C" | "S" if visual => self.visual_linewise_operator('c'),
            "Y" if visual => self.visual_linewise_operator('y'),
            "p" | "P" if visual => self.visual_paste(),
            "d" | "c" | "y" | ">" | "<" => {
                self.op_count = self.count;
                self.count = 0;
                self.pending = Pending::Op(key.chars().next().unwrap_or('d'));
            }
            // --- Text objects and find-char open a chord in Visual too.
            "i" | "a" if visual => {
                self.pending = Pending::Obj {
                    op: None,
                    around: key == "a",
                };
            }
            "f" | "F" | "t" | "T" => {
                self.pending = Pending::Find {
                    op: None,
                    kind: key.chars().next().unwrap_or('f'),
                };
            }
            "g" => self.pending = Pending::G(None),
            "r" => self.pending = Pending::Replace,
            // --- The stateful prefixes: register, mark, macro.
            "\"" => self.pending = Pending::Register,
            "m" => self.pending = Pending::Mark,
            "`" | "'" => {
                self.pending = Pending::JumpMark {
                    op: None,
                    linewise: key == "'",
                };
            }
            "q" => {
                // A second `q` closes the recording rather than opening
                // another one.
                if let Some((reg, steps)) = self.recording.take() {
                    self.macros.insert(reg, steps);
                } else {
                    self.pending = Pending::RecordMacro;
                }
            }
            "@" => self.pending = Pending::RunMacro,
            // --- Search.
            "/" | "?" => self.open_search(None, key == "/"),
            "n" | "N" => {
                let n = self.take_count();
                self.repeat_search(None, key == "n", n);
            }
            "*" | "#" => {
                let n = self.take_count();
                self.search_word_under_cursor(key == "*", n);
            }
            // --- Motions.
            "h" | "left" | "l" | "right" | "j" | "down" | "k" | "up" | "w" | "W" | "b" | "B"
            | "e" | "E" | "0" | "home" | "^" | "$" | "end" | "G" | "{" | "}" | ";" | ","
            | "pageup" | "pagedown" | "_" | "+" | "-" | "enter" | "|" | "%" | "C-f" | "C-b"
            | "C-d" | "C-u" => {
                let n = self.take_count();
                // A vertical motion keeps aiming at the column it was
                // aiming at; everything else re-aims at where it lands.
                let vertical = matches!(key, "j" | "down" | "k" | "up");
                let wanted = vertical.then(|| self.wanted_col());
                if let Some((target, _)) = self.motion(key, n, false) {
                    self.cursor = target;
                    self.set_wanted_col(wanted);
                    // `$` as an operator target is the line end; as a
                    // cursor move it lands *on* the last char, because
                    // a Normal cursor never sits past one (vim).
                    if matches!(key, "$" | "end") {
                        self.cursor = self.offset_left(self.cursor);
                    }
                }
            }
            // --- Mode switches.
            "i" => {
                self.count = 0;
                self.to_insert();
            }
            "a" => {
                self.count = 0;
                self.right();
                self.to_insert();
            }
            "I" => {
                self.count = 0;
                self.cursor = self.first_non_blank(self.cursor);
                self.to_insert();
            }
            "A" => {
                self.count = 0;
                self.line_end_motion();
                self.to_insert();
            }
            "o" if visual => {
                std::mem::swap(&mut self.anchor, &mut self.cursor);
            }
            "o" => {
                self.count = 0;
                self.checkpoint();
                self.open_below();
            }
            "O" => {
                self.count = 0;
                self.checkpoint();
                self.open_above();
            }
            "v" => {
                self.count = 0;
                if self.mode == EditorMode::Visual {
                    self.mode = EditorMode::Normal;
                } else {
                    if self.mode == EditorMode::Normal {
                        self.anchor = self.cursor;
                    }
                    self.mode = EditorMode::Visual;
                }
            }
            "V" => {
                self.count = 0;
                if self.mode == EditorMode::VisualLine {
                    self.mode = EditorMode::Normal;
                } else {
                    if self.mode == EditorMode::Normal {
                        self.anchor = self.cursor;
                    }
                    self.mode = EditorMode::VisualLine;
                }
            }
            "escape" => {
                self.count = 0;
                self.op_count = 0;
                self.mode = EditorMode::Normal;
            }
            // --- Whole-line and single-char edits.
            "D" => self.operate_to_line_end('d'),
            "C" => self.operate_to_line_end('c'),
            "S" => {
                let n = self.take_count();
                self.apply_linewise_operator('c', n);
            }
            // `Y` is `yy`, not `y$` — vim's own inconsistency, and the
            // one every muscle memory has.
            "Y" => {
                let n = self.take_count();
                self.apply_linewise_operator('y', n);
            }
            "s" => {
                let n = self.take_count();
                self.checkpoint();
                self.delete_chars_forward(n);
                self.mode = EditorMode::Insert;
                self.insert_armed = false;
            }
            "x" | "delete" => {
                let n = self.take_count();
                self.checkpoint();
                self.delete_chars_forward(n);
            }
            "X" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    if let Some((i, c)) = self.buf[..self.cursor].char_indices().next_back()
                        && c != '\n'
                    {
                        self.buf.remove(i);
                        self.cursor = i;
                    }
                }
            }
            "~" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    self.toggle_case_at_cursor();
                }
            }
            "J" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    self.join_line();
                }
            }
            "p" | "P" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    if key == "p" {
                        self.paste();
                    } else {
                        self.paste_before();
                    }
                }
            }
            "u" => {
                self.count = 0;
                self.undo_local();
            }
            "R" => {
                self.count = 0;
                self.to_insert();
                self.replacing = true;
            }
            "C-a" | "C-x" => {
                let n = self.take_count();
                let delta = i64::try_from(n).unwrap_or(1);
                self.change_number(if key == "C-a" { delta } else { -delta });
            }
            _ => self.count = 0,
        }
    }

    // ---- Chord continuations -------------------------------------

    /// The stroke after an operator: its own key doubles it (`dd`), a
    /// text object or find opens a further chord, anything else must
    /// resolve as a motion or the operator is abandoned.
    fn after_operator(&mut self, op: char, key: &str) {
        if self.take_digit(key) {
            return;
        }
        self.pending = Pending::None;
        match key {
            "escape" => {
                self.count = 0;
                self.op_count = 0;
            }
            // `dd`, `yy`, `cc`, `>>`, `<<` — linewise over count lines.
            // `S` doubles for `cc` the way vim lets it.
            k if k.starts_with(op) => {
                let n = self.take_count_for_op();
                self.apply_linewise_operator(op, n);
            }
            "i" | "a" => {
                self.pending = Pending::Obj {
                    op: Some(op),
                    around: key == "a",
                };
            }
            "f" | "F" | "t" | "T" => {
                self.pending = Pending::Find {
                    op: Some(op),
                    kind: key.chars().next().unwrap_or('f'),
                };
            }
            "`" | "'" => {
                self.pending = Pending::JumpMark {
                    op: Some(op),
                    linewise: key == "'",
                };
            }
            "/" | "?" => self.open_search(Some(op), key == "/"),
            "n" | "N" => {
                let n = self.take_count_for_op();
                self.repeat_search(Some(op), key == "n", n);
            }
            "g" => self.pending = Pending::G(Some(op)),
            _ => {
                // Vim's oldest wart: on a non-blank, `cw` changes the
                // word without its trailing space, i.e. it is `ce`.
                let key = match key {
                    "w" | "W"
                        if op == 'c'
                            && self
                                .char_at(self.cursor)
                                .is_some_and(|c| !c.is_whitespace()) =>
                    {
                        if key == "w" {
                            "e"
                        } else {
                            "E"
                        }
                    }
                    k => k,
                };
                let n = self.take_count_for_op();
                if let Some((target, kind)) = self.motion(key, n, true) {
                    self.apply_operator(op, self.cursor, target, kind);
                } else {
                    self.count = 0;
                    self.op_count = 0;
                }
            }
        }
    }

    /// `g`-prefixed chords: `gg` (and `dgg`), `ge`, `g_`.
    fn finish_g(&mut self, op: Option<char>, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        // `gu`, `gU`, `g~` are operators in their own right: they arm a
        // pending chord exactly as `d` does, and then take a motion or
        // a text object (`guiw`) or double (`guu`).
        if op.is_none() && matches!(key, "u" | "U" | "~") {
            self.op_count = n;
            self.count = 0;
            self.pending = Pending::Op(key.chars().next().unwrap_or('u'));
            return;
        }
        // `gJ` joins with no separator at all — the one thing plain `J`
        // will not do. Not a motion, so it is answered here.
        if op.is_none() && key == "J" {
            self.checkpoint();
            for _ in 0..n {
                self.join_line_raw();
            }
            return;
        }
        // `gv` puts the last selection back; `gi` resumes typing where
        // INSERT was last left. Neither is a range, so neither can be
        // an operator target.
        if op.is_none() && key == "v" {
            if let Some((anchor, cursor, mode)) = self.last_visual {
                self.anchor = anchor.min(self.buf.len());
                self.set_cursor_byte(cursor);
                self.mode = mode;
            }
            return;
        }
        if op.is_none() && key == "i" {
            if let Some(at) = self.last_insert {
                self.set_cursor_byte(at);
            }
            self.to_insert();
            return;
        }
        let target = match key {
            "g" => Some(self.line_col_offset(if self.count_given { n - 1 } else { 0 }, 0)),
            "e" | "E" => Some(self.word_end_back(self.cursor, key == "E", n)),
            "_" => Some(self.first_non_blank_backwards_from_line_end()),
            // The display-line motions. Nothing soft-wraps here, so a
            // display line is a real line and these are their plain
            // counterparts — but they must exist, or `gj` would eat the
            // `g` and leave the cursor where it was.
            "0" | "home" => Some(self.line_start(self.cursor)),
            "^" => Some(self.first_non_blank(self.cursor)),
            "$" | "end" => Some(self.line_end(self.cursor)),
            "j" | "down" => Some(self.line_col_offset(self.cursor_line() + n, 0)),
            "k" | "up" => Some(self.line_col_offset(self.cursor_line().saturating_sub(n), 0)),
            _ => None,
        };
        let Some(target) = target else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let kind = match key {
            "g" | "j" | "down" | "k" | "up" => MotionKind::Linewise,
            "0" | "home" | "^" | "$" | "end" => MotionKind::Exclusive,
            _ => MotionKind::Inclusive,
        };
        if let Some(op) = op {
            self.apply_operator(op, self.cursor, target, kind);
        } else {
            self.cursor = target;
            // As with `$`, a Normal cursor never sits past the last
            // char of a line.
            if matches!(key, "$" | "end") {
                self.cursor = self.offset_left(self.cursor);
            }
        }
    }

    /// The target char of an `f`/`F`/`t`/`T` chord.
    fn finish_find(&mut self, op: Option<char>, kind: char, key: &str) {
        self.pending = Pending::None;
        let Some(target_char) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        self.last_find = Some((kind, target_char));
        self.run_find(op, kind, target_char);
    }

    /// Execute a find, shared by `f`-chords and the `;`/`,` repeats.
    fn run_find(&mut self, op: Option<char>, kind: char, target_char: char) {
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let Some(target) = self.find_char(kind, target_char, n) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let motion_kind = if matches!(kind, 'f' | 't') {
            MotionKind::Inclusive
        } else {
            MotionKind::Exclusive
        };
        match op {
            Some(op) => self.apply_operator(op, self.cursor, target, motion_kind),
            None => self.cursor = target,
        }
    }

    /// The object char of an `i`/`a` chord (`iw`, `a"`, `ip`, …).
    fn finish_object(&mut self, op: Option<char>, around: bool, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let Some(obj) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let Some((lo, hi, kind)) = self.text_object(obj, around, n) else {
            return;
        };
        // An operator consumes the object range directly; in Visual the
        // object *becomes* the selection.
        if let Some(op) = op {
            self.apply_range_operator(op, lo, hi, kind);
        } else {
            self.mode = if kind == MotionKind::Linewise {
                EditorMode::VisualLine
            } else {
                EditorMode::Visual
            };
            self.anchor = lo;
            self.cursor = self.prev_offset(hi).max(lo);
        }
    }

    /// The register named by a `"x` prefix.
    fn finish_register(&mut self, key: &str) {
        self.pending = Pending::None;
        self.target_register = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic());
    }

    /// The mark named by an `m` chord.
    fn finish_mark(&mut self, key: &str) {
        self.pending = Pending::None;
        if let Some(c) = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        {
            self.marks.insert(c, self.cursor);
        }
    }

    /// The mark named by a `` ` `` / `'` chord, jumped to or operated on.
    ///
    /// An unset mark is not an error, it is nothing: vim beeps, and the
    /// buffer must be left exactly as it was.
    fn finish_mark_jump(&mut self, op: Option<char>, linewise: bool, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let _ = n;
        let Some(&raw) = key
            .chars()
            .next()
            .filter(|_| key.chars().count() == 1)
            .and_then(|c| self.marks.get(&c))
        else {
            return;
        };
        let mut target = raw.min(self.buf.len());
        while target > 0 && !self.buf.is_char_boundary(target) {
            target -= 1;
        }
        if linewise {
            target = self.first_non_blank(target);
        }
        let kind = if linewise {
            MotionKind::Linewise
        } else {
            MotionKind::Exclusive
        };
        match op {
            Some(op) => self.apply_operator(op, self.cursor, target, kind),
            None => self.cursor = target,
        }
    }

    /// The register a `q` chord starts recording into.
    fn finish_record(&mut self, key: &str) {
        self.pending = Pending::None;
        if let Some(c) = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        {
            self.recording = Some((c, Vec::new()));
        }
    }

    /// The macro an `@` chord replays; `@@` repeats the last one.
    fn finish_run_macro(&mut self, key: &str) {
        self.pending = Pending::None;
        let n = self.take_count();
        let reg = if key == "@" {
            self.last_macro
        } else {
            key.chars()
                .next()
                .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        };
        if let Some(reg) = reg {
            self.run_macro(reg, n);
        }
    }

    /// Replay macro `reg` `n` times.
    ///
    /// The replay records nothing (a macro that recorded itself would
    /// grow without bound) and leaves INSERT behind it, so a macro that
    /// ends mid-typing is repeatable.
    fn run_macro(&mut self, reg: char, n: usize) {
        let Some(steps) = self.macros.get(&reg).cloned() else {
            return;
        };
        self.last_macro = Some(reg);
        let outer = self.running_macro;
        self.running_macro = true;
        for _ in 0..n {
            for step in &steps {
                match step {
                    MacroStep::Key(k) if k == "escape" && self.mode == EditorMode::Insert => {
                        self.to_normal();
                    }
                    MacroStep::Key(k) => self.dispatch_modal_key(k),
                    MacroStep::Text(t) => {
                        let t = t.clone();
                        self.insert_str(&t);
                    }
                }
            }
            if self.mode == EditorMode::Insert {
                self.to_normal();
            }
        }
        self.running_macro = outer;
    }

    /// The register being recorded into, for a shell's `recording @q`
    /// indicator. `None` when nothing is.
    #[must_use]
    pub const fn recording_register(&self) -> Option<char> {
        match self.recording {
            Some((reg, _)) => Some(reg),
            None => None,
        }
    }

    /// Whether typing overwrites rather than inserts (`R`).
    #[must_use]
    pub const fn replacing(&self) -> bool {
        self.replacing
    }

    /// The open search line as the user sees it (`/beta`), or `None`.
    #[must_use]
    pub fn search_prompt(&self) -> Option<String> {
        self.search_input
            .as_ref()
            .map(|(fwd, pat)| format!("{}{pat}", if *fwd { '/' } else { '?' }))
    }

    /// The last search pattern, for a shell that highlights matches.
    #[must_use]
    pub fn search_pattern(&self) -> Option<&str> {
        self.last_search.as_ref().map(|(p, _)| p.as_str())
    }

    /// Open the `/` (or `?`) line, carrying any armed operator with it
    /// so `d/foo` deletes up to the match.
    fn open_search(&mut self, op: Option<char>, forward: bool) {
        self.search_input = Some((forward, String::new()));
        self.search_op = op;
    }

    /// One key of the open search line.
    fn search_key(&mut self, key: &str) {
        let Some((forward, pattern)) = self.search_input.as_mut() else {
            return;
        };
        match key {
            "escape" => {
                self.search_input = None;
                self.search_op = None;
                self.count = 0;
                self.op_count = 0;
            }
            "enter" => {
                let forward = *forward;
                let pattern = std::mem::take(pattern);
                self.search_input = None;
                let op = self.search_op.take();
                if pattern.is_empty() {
                    return;
                }
                self.last_search = Some((pattern, forward));
                let n = if op.is_some() {
                    self.take_count_for_op()
                } else {
                    self.take_count()
                };
                self.repeat_search(op, true, n);
            }
            // Backspacing past the `/` closes the line, the way it
            // closes vim's — the pattern and its prompt are one thing.
            "backspace" => {
                if pattern.pop().is_none() {
                    self.search_input = None;
                    self.search_op = None;
                }
            }
            k => {
                if let Some(c) = k.chars().next().filter(|_| k.chars().count() == 1) {
                    pattern.push(c);
                }
            }
        }
    }

    /// Run the last search `n` times, `same` direction or reversed
    /// (`N`), optionally as an operator target.
    fn repeat_search(&mut self, op: Option<char>, same: bool, n: usize) {
        let Some((pattern, forward)) = self.last_search.clone() else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let forward = forward == same;
        let mut at = self.cursor;
        for _ in 0..n {
            let Some(next) = self.find_from(&pattern, at, forward) else {
                self.count = 0;
                self.op_count = 0;
                return;
            };
            at = next;
        }
        match op {
            Some(op) => self.apply_operator(op, self.cursor, at, MotionKind::Exclusive),
            None => self.cursor = at,
        }
    }

    /// `*`/`#`: search for the word under the cursor.
    fn search_word_under_cursor(&mut self, forward: bool, n: usize) {
        let Some((lo, hi, _)) = self.word_object(false, false, 1) else {
            return;
        };
        let word = self.buf[lo..hi].to_owned();
        if word.trim().is_empty() {
            return;
        }
        self.last_search = Some((word, forward));
        self.repeat_search(None, true, n);
    }

    /// The next occurrence of `pattern` from `at`, wrapping around the
    /// buffer exactly once. `None` when the pattern is nowhere.
    fn find_from(&self, pattern: &str, at: usize, forward: bool) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        if forward {
            let from = self.next_offset(at);
            self.buf
                .get(from..)
                .and_then(|s| s.find(pattern))
                .map(|i| from + i)
                .or_else(|| self.buf.find(pattern))
        } else {
            self.buf
                .get(..at)
                .and_then(|s| s.rfind(pattern))
                .or_else(|| self.buf.rfind(pattern))
        }
    }

    /// `C-a`/`C-x`: add `delta` to the first number at or right of the
    /// cursor on its line, leaving the cursor on the number's last
    /// digit (vim's rule).
    fn change_number(&mut self, delta: i64) {
        let line_end = self.line_end(self.cursor);
        let Some(digit) = self.buf[self.cursor..line_end]
            .char_indices()
            .find(|&(_, c)| c.is_ascii_digit())
            .map(|(i, _)| self.cursor + i)
        else {
            return;
        };
        let line_start = self.line_start(self.cursor);
        let mut lo = digit;
        while lo > line_start && self.buf.as_bytes()[lo - 1].is_ascii_digit() {
            lo -= 1;
        }
        // A `-` immediately before the digits is part of the number.
        if lo > line_start && self.buf.as_bytes()[lo - 1] == b'-' {
            lo -= 1;
        }
        let mut hi = digit;
        while hi < line_end && self.buf.as_bytes()[hi].is_ascii_digit() {
            hi += 1;
        }
        let Ok(value) = self.buf[lo..hi].parse::<i64>() else {
            return;
        };
        self.checkpoint();
        let replacement = (value.saturating_add(delta)).to_string();
        self.buf.replace_range(lo..hi, &replacement);
        self.cursor = lo + replacement.len() - 1;
    }

    /// The replacement char of an `r` chord.
    fn finish_replace(&mut self, key: &str) {
        self.pending = Pending::None;
        let n = self.take_count();
        let Some(c) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            return;
        };
        if key == "escape" {
            return;
        }
        // Vim refuses `r` when fewer than `count` chars remain on the line.
        let mut end = self.cursor;
        for _ in 0..n {
            let next = self.next_offset(end);
            if next == end || self.buf[end..].starts_with('\n') {
                return;
            }
            end = next;
        }
        self.checkpoint();
        let replacement: String = std::iter::repeat_n(c, n).collect();
        self.buf.replace_range(self.cursor..end, &replacement);
        self.cursor = self.prev_offset(end.min(self.buf.len())).max(self.cursor);
    }

    // ---- Motions -------------------------------------------------

    /// Resolve `key` to a target offset and how it combines into a
    /// range. `for_op` applies vim's rule that `dw` never joins lines.
    /// The column a vertical motion should aim for: the one it has been
    /// aiming for since the last horizontal move, else where the cursor
    /// is now.
    fn wanted_col(&self) -> usize {
        self.wanted_col.unwrap_or_else(|| self.cursor_line_col().1)
    }

    /// Remember the column for the vertical motions, or forget it.
    ///
    /// Everything horizontal — a motion, an edit, a click — decides a
    /// new column; only `j`/`k` and the arrows inherit one.
    const fn set_wanted_col(&mut self, col: Option<usize>) {
        self.wanted_col = col;
    }

    fn motion(&self, key: &str, n: usize, for_op: bool) -> Option<(usize, MotionKind)> {
        use MotionKind::{Exclusive, Inclusive, Linewise};
        let (target, kind) = match key {
            "h" | "left" => (self.repeat(n, self.cursor, Self::offset_left), Exclusive),
            "l" | "right" => (self.repeat(n, self.cursor, Self::offset_right), Exclusive),
            // Vertical motion keeps the column, and keeps *wanting* the
            // column it started in: vim's `curswant`, so passing through
            // a short line on the way down does not cost you the place
            // you were reading. It used to be a flat column zero, so
            // every `j` jumped to the start of the next line.
            "j" | "down" => (
                self.line_col_offset(self.cursor_line() + n, self.wanted_col()),
                Linewise,
            ),
            "k" | "up" => (
                self.line_col_offset(self.cursor_line().saturating_sub(n), self.wanted_col()),
                Linewise,
            ),
            "w" | "W" => {
                let big = key == "W";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_forward_from(at, big);
                }
                // `dw` on the last word of a line stops at the line end
                // instead of dragging the next line up.
                if for_op && self.line_end(self.cursor) < at {
                    (self.line_end(self.cursor), Exclusive)
                } else {
                    (at, Exclusive)
                }
            }
            "b" | "B" => {
                let big = key == "B";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_backward_from(at, big);
                }
                (at, Exclusive)
            }
            "e" | "E" => {
                let big = key == "E";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_end_from(at, big);
                }
                (at, Inclusive)
            }
            "0" | "home" => (self.line_start(self.cursor), Exclusive),
            "^" => (self.first_non_blank(self.cursor), Exclusive),
            "pagedown" => (
                self.line_col_offset(self.cursor_line() + PAGE_LINES * n, 0),
                Linewise,
            ),
            "pageup" => (
                self.line_col_offset(self.cursor_line().saturating_sub(PAGE_LINES * n), 0),
                Linewise,
            ),
            "$" | "end" => {
                let line = self.cursor_line() + n - 1;
                (self.line_end(self.line_col_offset(line, 0)), Exclusive)
            }
            "G" => {
                let last = self.line_count().saturating_sub(1);
                let line = if self.count_given { n - 1 } else { last };
                (self.line_col_offset(line.min(last), 0), Linewise)
            }
            "}" => (self.paragraph_forward(n), Exclusive),
            "{" => (self.paragraph_backward(n), Exclusive),
            // The first-non-blank line motions. `_` counts the current
            // line, `+`/`-` count from it — vim's off-by-one, kept.
            "_" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line() + n - 1, 0)),
                Linewise,
            ),
            "+" | "enter" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line() + n, 0)),
                Linewise,
            ),
            "-" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line().saturating_sub(n), 0)),
                Linewise,
            ),
            // `|` is 1-based: `1|` is the line start.
            "|" => (self.line_col_offset(self.cursor_line(), n - 1), Exclusive),
            "%" => (self.matching_bracket()?, Inclusive),
            "C-f" | "C-b" | "C-d" | "C-u" => (self.scroll_motion(key, n), Linewise),
            ";" | "," => {
                let (kind, ch) = self.last_find?;
                let kind = if key == ";" {
                    kind
                } else {
                    match kind {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    }
                };
                let target = self.find_char(kind, ch, n)?;
                let k = if matches!(kind, 'f' | 't') {
                    Inclusive
                } else {
                    Exclusive
                };
                (target, k)
            }
            _ => return None,
        };
        Some((target, kind))
    }

    /// `C-f`/`C-b` (a page) and `C-d`/`C-u` (half of one).
    ///
    /// The editor owns no viewport — the shells derive their scroll
    /// from the cursor — so moving the cursor *is* the scroll, and both
    /// pairs are line motions rather than viewport commands.
    fn scroll_motion(&self, key: &str, n: usize) -> usize {
        let step = if matches!(key, "C-f" | "C-b") {
            PAGE_LINES
        } else {
            PAGE_LINES / 2
        };
        let line = self.cursor_line();
        let target = if matches!(key, "C-f" | "C-d") {
            line + step * n
        } else {
            line.saturating_sub(step * n)
        };
        self.line_col_offset(target, 0)
    }

    /// `%`: the bracket matching the first one at or right of the
    /// cursor on its line.
    ///
    /// Vim scans forward for a bracket before matching, so `%` works
    /// from anywhere on a line that has one; the match itself nests and
    /// crosses lines. `None` when the line holds no bracket, or when
    /// the one it holds is unbalanced.
    fn matching_bracket(&self) -> Option<usize> {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
        let line_end = self.line_end(self.cursor);
        let (at, here) = self.buf[self.cursor..line_end]
            .char_indices()
            .find(|&(_, c)| PAIRS.iter().any(|&(o, cl)| c == o || c == cl))
            .map(|(i, c)| (self.cursor + i, c))?;
        let forward = PAIRS.iter().find(|&&(o, _)| o == here);
        let mut depth = 0usize;
        if let Some(&(open, close)) = forward {
            for (i, c) in self.buf[self.next_offset(at)..].char_indices() {
                if c == open {
                    depth += 1;
                } else if c == close {
                    if depth == 0 {
                        return Some(self.next_offset(at) + i);
                    }
                    depth -= 1;
                }
            }
            return None;
        }
        let &(open, close) = PAIRS.iter().find(|&&(_, cl)| cl == here)?;
        for (i, c) in self.buf[..at].char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
        }
        None
    }

    /// Apply `f`/`F`/`t`/`T` `n` times within the cursor's line.
    fn find_char(&self, kind: char, target: char, n: usize) -> Option<usize> {
        let line_start = self.line_start(self.cursor);
        let line_end = self.line_end(self.cursor);
        let mut at = self.cursor;
        for _ in 0..n {
            at = match kind {
                'f' | 't' => {
                    // `t` starts one char further so `;` makes progress.
                    let from = self.next_offset(if kind == 't' {
                        self.next_offset(at)
                    } else {
                        at
                    });
                    let rel = self.buf[from.min(line_end)..line_end].find(target)?;
                    from + rel
                }
                _ => {
                    let to = if kind == 'T' {
                        self.prev_offset(at)
                    } else {
                        at
                    };
                    let rel = self.buf[line_start..to.max(line_start)].rfind(target)?;
                    line_start + rel
                }
            };
        }
        Some(match kind {
            't' => self.prev_offset(at),
            'T' => self.next_offset(at),
            _ => at,
        })
    }

    // ---- Text objects --------------------------------------------

    /// Resolve `iw`/`aw`, `i(`/`a(` …, `i"`/`a"`, `ip`/`ap` to a byte
    /// range plus its kind. `None` when the cursor is not inside one.
    fn text_object(&self, obj: char, around: bool, n: usize) -> Option<(usize, usize, MotionKind)> {
        match obj {
            'w' | 'W' => self.word_object(obj == 'W', around, n),
            's' => self.sentence_object(around),
            'p' => self.paragraph_object(around),
            '"' | '\'' | '`' => self.quote_object(obj, around),
            '(' | ')' | 'b' => self.bracket_object('(', ')', around),
            '[' | ']' => self.bracket_object('[', ']', around),
            '{' | '}' | 'B' => self.bracket_object('{', '}', around),
            '<' | '>' => self.bracket_object('<', '>', around),
            _ => None,
        }
    }

    /// `iw`/`aw`: the run of same-class chars under the cursor, plus
    /// (for `aw`) the blanks after it — or before it at a line end.
    fn word_object(&self, big: bool, around: bool, n: usize) -> Option<(usize, usize, MotionKind)> {
        let c = self.char_at(self.cursor)?;
        let class = char_class(c, big);
        let mut lo = self.cursor;
        while let Some((i, prev)) = self.buf[..lo].char_indices().next_back() {
            if prev != '\n' && char_class(prev, big) == class {
                lo = i;
            } else {
                break;
            }
        }
        let mut hi = self.cursor;
        while let Some(next) = self.char_at(hi) {
            if next != '\n' && char_class(next, big) == class {
                hi += next.len_utf8();
            } else {
                break;
            }
        }
        // `2iw` spans the following runs too (blanks count as a run).
        for _ in 1..n {
            let Some(next) = self.char_at(hi) else { break };
            if next == '\n' {
                break;
            }
            let next_class = char_class(next, big);
            while let Some(c) = self.char_at(hi) {
                if c != '\n' && char_class(c, big) == next_class {
                    hi += c.len_utf8();
                } else {
                    break;
                }
            }
        }
        if around {
            let trailing_start = hi;
            while let Some(next) = self.char_at(hi) {
                if next != '\n' && next.is_whitespace() {
                    hi += next.len_utf8();
                } else {
                    break;
                }
            }
            // No trailing blanks (last word on the line): take the
            // leading ones instead, the way vim does.
            if hi == trailing_start && class != CharClass::Blank {
                while let Some((i, prev)) = self.buf[..lo].char_indices().next_back() {
                    if prev != '\n' && prev.is_whitespace() {
                        lo = i;
                    } else {
                        break;
                    }
                }
            }
        }
        Some((lo, hi, MotionKind::Exclusive))
    }

    /// `is`/`as`: the sentence around the cursor.
    ///
    /// A sentence ends at `.`, `!` or `?` (with any closing bracket or
    /// quote after it) and runs to the start of the next one. `as`
    /// takes the blanks that separate it from its successor, so
    /// `das` leaves the surrounding prose spaced as it was.
    ///
    /// Always resolves — every cursor sits in some sentence — but it
    /// returns the shape of its sibling objects so the caller can treat
    /// all of them alike.
    #[allow(clippy::unnecessary_wraps)]
    fn sentence_object(&self, around: bool) -> Option<(usize, usize, MotionKind)> {
        const CLOSERS: [char; 4] = [')', ']', '"', '\''];
        let para_start = self.line_start(self.cursor);
        let para_end = self.line_end(self.cursor);
        let text = &self.buf[para_start..para_end];
        // Every sentence end within the line, as an offset one past the
        // terminator and its closers.
        let mut ends = Vec::new();
        let bytes: Vec<(usize, char)> = text.char_indices().collect();
        for (i, (off, c)) in bytes.iter().enumerate() {
            if !matches!(c, '.' | '!' | '?') {
                continue;
            }
            let mut end = off + c.len_utf8();
            let mut j = i + 1;
            while let Some((o, c)) = bytes.get(j).filter(|(_, c)| CLOSERS.contains(c)) {
                end = o + c.len_utf8();
                j += 1;
            }
            // A terminator mid-word (`3.5`) does not end a sentence.
            if bytes.get(j).is_none_or(|(_, c)| c.is_whitespace()) {
                ends.push(end);
            }
        }
        let here = self.cursor - para_start;
        let lo = ends.iter().copied().rfind(|&e| e <= here).map_or(0, |e| {
            e + text[e..]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(text.len() - e)
        });
        let mut hi = ends
            .iter()
            .copied()
            .find(|&e| e > here)
            .unwrap_or(text.len());
        if around {
            hi += text[hi..]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(text.len() - hi);
        }
        Some((para_start + lo, para_start + hi, MotionKind::Exclusive))
    }

    /// `ip`/`ap`: the run of blank or non-blank lines around the cursor.
    /// Always resolves — every cursor sits in some paragraph — but it
    /// returns the same shape as its sibling objects so the caller can
    /// treat all of them alike.
    #[allow(clippy::unnecessary_wraps)]
    fn paragraph_object(&self, around: bool) -> Option<(usize, usize, MotionKind)> {
        let blank = |line: usize, ed: &Self| -> bool {
            let s = ed.line_col_offset(line, 0);
            ed.buf[s..ed.line_end(s)].trim().is_empty()
        };
        let cur = self.cursor_line();
        let last = self.line_count().saturating_sub(1);
        let want = blank(cur, self);
        let mut first = cur;
        while first > 0 && blank(first - 1, self) == want {
            first -= 1;
        }
        let mut end = cur;
        while end < last && blank(end + 1, self) == want {
            end += 1;
        }
        if around {
            let tail = end;
            while end < last && blank(end + 1, self) != want {
                end += 1;
            }
            if end == tail {
                while first > 0 && blank(first - 1, self) != want {
                    first -= 1;
                }
            }
        }
        let lo = self.line_col_offset(first, 0);
        let hi = self.line_end(self.line_col_offset(end, 0));
        Some((lo, hi, MotionKind::Linewise))
    }

    /// `i"`/`a"`: the quoted run on the cursor's line. Quotes pair up
    /// left to right, so the cursor picks the first pair it is not
    /// already past.
    fn quote_object(&self, q: char, around: bool) -> Option<(usize, usize, MotionKind)> {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        let positions: Vec<usize> = self.buf[start..end]
            .char_indices()
            .filter(|&(_, c)| c == q)
            .map(|(i, _)| start + i)
            .collect();
        let (open, close) = positions
            .chunks_exact(2)
            .map(|p| (p[0], p[1]))
            .find(|&(_, close)| self.cursor <= close)?;
        Some(if around {
            (open, self.next_offset(close), MotionKind::Exclusive)
        } else {
            (self.next_offset(open), close, MotionKind::Exclusive)
        })
    }

    /// `i(`/`a(` and friends: the innermost balanced pair containing
    /// the cursor, brackets included when `around`.
    fn bracket_object(
        &self,
        open_c: char,
        close_c: char,
        around: bool,
    ) -> Option<(usize, usize, MotionKind)> {
        let here = self.char_at(self.cursor);
        // Sitting on a bracket picks that pair rather than its parent.
        let open = if here == Some(open_c) {
            self.cursor
        } else {
            let mut depth = 0usize;
            let mut found = None;
            let search_end = if here == Some(close_c) {
                self.cursor
            } else {
                self.next_offset(self.cursor)
            };
            for (i, c) in self.buf[..search_end].char_indices().rev() {
                if c == close_c {
                    depth += 1;
                } else if c == open_c {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth -= 1;
                }
            }
            found?
        };
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in self.buf[self.next_offset(open)..].char_indices() {
            if c == open_c {
                depth += 1;
            } else if c == close_c {
                if depth == 0 {
                    close = Some(self.next_offset(open) + i);
                    break;
                }
                depth -= 1;
            }
        }
        let close = close?;
        Some(if around {
            (open, self.next_offset(close), MotionKind::Exclusive)
        } else {
            (self.next_offset(open), close, MotionKind::Exclusive)
        })
    }

    // ---- Operators -----------------------------------------------

    /// Combine cursor and motion target into a range, then run the
    /// operator over it.
    fn apply_operator(&mut self, op: char, from: usize, target: usize, kind: MotionKind) {
        let (lo, hi) = match kind {
            // Linewise ranges are widened to whole lines downstream, so
            // the raw endpoints are all `run_linewise` needs.
            MotionKind::Exclusive | MotionKind::Linewise => (from.min(target), from.max(target)),
            MotionKind::Inclusive => {
                let hi = from.max(target);
                (from.min(target), self.next_offset(hi))
            }
        };
        self.apply_range_operator(op, lo, hi, kind);
    }

    /// Run `op` over an already-resolved range.
    fn apply_range_operator(&mut self, op: char, lo: usize, hi: usize, kind: MotionKind) {
        self.count = 0;
        self.op_count = 0;
        if kind == MotionKind::Linewise {
            let first = self.line_start(lo);
            let last_end = self.line_end(hi);
            return self.run_linewise(op, first, last_end);
        }
        if hi <= lo {
            return;
        }
        match op {
            'y' => {
                self.set_register(self.buf[lo..hi].to_owned(), false);
                self.cursor = lo;
            }
            'd' | 'c' => {
                self.checkpoint();
                self.set_register(self.buf[lo..hi].to_owned(), false);
                self.buf.replace_range(lo..hi, "");
                self.cursor = lo;
                if op == 'c' {
                    self.mode = EditorMode::Insert;
                    self.insert_armed = false;
                } else if self.mode != EditorMode::Normal {
                    self.mode = EditorMode::Normal;
                }
            }
            '>' | '<' => self.run_linewise(op, self.line_start(lo), self.line_end(hi)),
            'u' | 'U' | '~' => {
                self.checkpoint();
                self.recase(lo, hi, op);
                self.cursor = lo;
                if self.mode != EditorMode::Normal {
                    self.mode = EditorMode::Normal;
                }
            }
            _ => {}
        }
    }

    /// Rewrite `lo..hi` to lower (`u`), upper (`U`) or flipped (`~`).
    fn recase(&mut self, lo: usize, hi: usize, op: char) {
        let mut recased = String::with_capacity(hi - lo);
        for c in self.buf[lo..hi].chars() {
            let to_upper = match op {
                'u' => false,
                'U' => true,
                _ => !c.is_uppercase(),
            };
            if to_upper {
                recased.extend(c.to_uppercase());
            } else {
                recased.extend(c.to_lowercase());
            }
        }
        self.buf.replace_range(lo..hi, &recased);
    }

    /// `dd`/`yy`/`cc`/`>>`: `n` whole lines from the cursor's.
    fn apply_linewise_operator(&mut self, op: char, n: usize) {
        self.count = 0;
        self.op_count = 0;
        let first = self.line_start(self.cursor);
        let mut last_end = self.line_end(first);
        for _ in 1..n {
            if last_end >= self.buf.len() {
                break;
            }
            last_end = self.line_end(last_end + 1);
        }
        self.run_linewise(op, first, last_end);
    }

    /// The linewise core: `first` is a line start, `last_end` the end
    /// (before the newline) of the last line in the range.
    fn run_linewise(&mut self, op: char, first: usize, last_end: usize) {
        let with_newline = if last_end < self.buf.len() {
            last_end + 1
        } else {
            last_end
        };
        match op {
            'y' => {
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                self.cursor = first;
            }
            'd' => {
                self.checkpoint();
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                // Deleting through the last (newline-less) line eats the
                // preceding newline so no dangling terminator remains.
                let cut_from = if with_newline >= self.buf.len() && first > 0 {
                    first - 1
                } else {
                    first
                };
                self.buf.replace_range(cut_from..with_newline, "");
                // Vim parks the cursor on the line that moved up into
                // the deleted one, clamped at the last line.
                self.cursor = self.line_start(cut_from.min(self.buf.len()));
            }
            'c' => {
                self.checkpoint();
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                // The line itself survives, emptied — that is what makes
                // `cc` different from `dd` followed by `O`.
                self.buf.replace_range(first..last_end, "");
                self.cursor = first;
                self.mode = EditorMode::Insert;
                self.insert_armed = false;
            }
            '>' | '<' => {
                self.checkpoint();
                self.shift_lines(first, last_end, op == '>');
            }
            'u' | 'U' | '~' => {
                self.checkpoint();
                self.recase(first, last_end, op);
                self.cursor = first;
            }
            _ => {}
        }
        if self.mode != EditorMode::Insert {
            self.mode = EditorMode::Normal;
        }
    }

    /// Indent (`>`) or dedent (`<`) every line in the range by two
    /// spaces — org bodies are space-indented, never tabbed.
    fn shift_lines(&mut self, first: usize, last_end: usize, indent: bool) {
        let mut starts = vec![first];
        let mut at = first;
        while at < last_end {
            let end = self.line_end(at);
            if end >= last_end {
                break;
            }
            at = end + 1;
            starts.push(at);
        }
        // Back to front so earlier edits do not shift later offsets.
        for &start in starts.iter().rev() {
            if indent {
                self.buf.insert_str(start, "  ");
            } else {
                let end = self.line_end(start);
                let drop = self.buf[start..end]
                    .chars()
                    .take(2)
                    .take_while(|c| *c == ' ')
                    .count();
                self.buf.replace_range(start..start + drop, "");
            }
        }
        self.cursor = self.first_non_blank(self.line_start(first.min(self.buf.len())));
    }

    /// `D`/`C`: the rest of the line, `n - 1` further lines included.
    fn operate_to_line_end(&mut self, op: char) {
        let n = self.take_count();
        let mut end = self.line_end(self.cursor);
        for _ in 1..n {
            if end >= self.buf.len() {
                break;
            }
            end = self.line_end(end + 1);
        }
        self.apply_range_operator(op, self.cursor, end, MotionKind::Exclusive);
        if op == 'c' {
            self.mode = EditorMode::Insert;
            self.insert_armed = false;
        }
    }

    /// A Visual-mode operator over the charwise selection.
    /// Insert text from outside the editor at the cursor, as one
    /// undoable edit, replacing a VISUAL selection if there is one.
    ///
    /// Separate from [`Self::insert_str`] because that one is *typing*:
    /// it feeds the dot-register so `.` retypes it. A clipboard paste
    /// is not something the user typed, and `.` repeating it would be
    /// a surprise with somebody else's text in it.
    pub fn paste_external(&mut self, text: &str) {
        self.checkpoint();
        if matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) {
            let (lo, hi) = if self.mode == EditorMode::VisualLine {
                (
                    self.line_start(self.anchor.min(self.cursor)),
                    self.line_end(self.anchor.max(self.cursor)),
                )
            } else {
                let (lo, hi) = self.selection();
                (lo, hi)
            };
            self.buf.replace_range(lo..hi, "");
            self.cursor = lo;
            self.mode = EditorMode::Normal;
        }
        self.buf.insert_str(self.cursor, text);
        self.cursor += text.len();
        // A Normal cursor never sits past the last character of the
        // line it is on.
        if self.mode == EditorMode::Normal {
            self.cursor = self
                .prev_offset(self.cursor)
                .max(self.line_start(self.cursor));
        }
    }

    fn visual_operator(&mut self, op: char) {
        if self.mode == EditorMode::VisualLine {
            return self.visual_linewise_operator(op);
        }
        let (lo, hi) = self.selection();
        self.apply_range_operator(op, lo, hi, MotionKind::Exclusive);
        if self.mode != EditorMode::Insert {
            self.mode = EditorMode::Normal;
        }
    }

    /// A Visual-mode operator forced linewise (`V`-mode, or `D`/`C`).
    fn visual_linewise_operator(&mut self, op: char) {
        let first = self.line_start(self.anchor.min(self.cursor));
        let last_end = self.line_end(self.anchor.max(self.cursor));
        self.count = 0;
        self.op_count = 0;
        self.run_linewise(op, first, last_end);
    }

    /// Visual `p`: the selection is replaced by the register, and the
    /// replaced text becomes the new register (vim's swap).
    fn visual_paste(&mut self) {
        let (lo, hi) = if self.mode == EditorMode::VisualLine {
            let first = self.line_start(self.anchor.min(self.cursor));
            let end = self.line_end(self.anchor.max(self.cursor));
            (first, end)
        } else {
            self.selection()
        };
        self.checkpoint();
        let (text, linewise) = self.take_register();
        let replaced = self.buf[lo..hi].to_owned();
        let insert = if linewise {
            text.trim_end_matches('\n').to_owned()
        } else {
            text
        };
        self.buf.replace_range(lo..hi, &insert);
        self.register = replaced;
        self.linewise = false;
        self.cursor = lo;
        self.mode = EditorMode::Normal;
    }

    /// `x`/`s`: delete up to `n` chars forward, never past the line end.
    fn delete_chars_forward(&mut self, n: usize) {
        let start = self.cursor;
        let line_end = self.line_end(start);
        let mut end = start;
        for _ in 0..n {
            let next = self.next_offset(end);
            if next == end || next > line_end {
                break;
            }
            end = next;
        }
        if end > start {
            self.set_register(self.buf[start..end].to_owned(), false);
            self.buf.replace_range(start..end, "");
        }
    }

    /// `~`: flip the case under the cursor and step right.
    fn toggle_case_at_cursor(&mut self) {
        let Some(c) = self.char_at(self.cursor).filter(|c| *c != '\n') else {
            return;
        };
        let flipped: String = if c.is_uppercase() {
            c.to_lowercase().collect()
        } else {
            c.to_uppercase().collect()
        };
        let end = self.cursor + c.len_utf8();
        self.buf.replace_range(self.cursor..end, &flipped);
        self.cursor = (self.cursor + flipped.len()).min(self.buf.len());
    }

    /// `J`: pull the next line up, separated by exactly one space.
    fn join_line(&mut self) {
        let end = self.line_end(self.cursor);
        if end >= self.buf.len() {
            return;
        }
        let next_start = end + 1;
        let next_end = self.line_end(next_start);
        let tail = self.buf[next_start..next_end].trim_start().to_owned();
        let needs_space =
            !tail.is_empty() && !self.buf[self.line_start(self.cursor)..end].ends_with(' ');
        let joiner = if needs_space { " " } else { "" };
        self.buf
            .replace_range(end..next_end, &format!("{joiner}{tail}"));
        self.cursor = end;
    }

    /// `gJ`: pull the next line up verbatim — no separator inserted and
    /// the next line's own indent kept, which is the whole difference
    /// from [`Self::join_line`].
    fn join_line_raw(&mut self) {
        let end = self.line_end(self.cursor);
        if end >= self.buf.len() {
            return;
        }
        self.buf.replace_range(end..=end, "");
        self.cursor = end;
    }

    /// `O`: open a line above the current one and enter Insert.
    pub fn open_above(&mut self) {
        let start = self.line_start(self.cursor);
        self.buf.insert(start, '\n');
        self.cursor = start;
        self.mode = EditorMode::Insert;
        self.insert_armed = false;
    }

    /// `P`: paste linewise above the current line, charwise at the
    /// cursor (as opposed to [`Self::paste`], which goes after).
    pub fn paste_before(&mut self) {
        let (text, linewise) = self.take_register();
        if text.is_empty() {
            return;
        }
        if linewise {
            let start = self.line_start(self.cursor);
            let text = format!("{}\n", text.trim_end_matches('\n'));
            self.buf.insert_str(start, &text);
            self.cursor = start;
        } else {
            self.buf.insert_str(self.cursor, &text);
        }
    }

    // ---- Offsets and lines ---------------------------------------

    /// The char starting at `pos`, if any.
    fn char_at(&self, pos: usize) -> Option<char> {
        self.buf.get(pos..).and_then(|s| s.chars().next())
    }

    /// The offset after the char at `pos` (`pos` itself at the end).
    fn next_offset(&self, pos: usize) -> usize {
        self.char_at(pos).map_or(pos, |c| pos + c.len_utf8())
    }

    /// The offset of the char before `pos` (`0` at the start).
    fn prev_offset(&self, pos: usize) -> usize {
        self.buf
            .get(..pos)
            .and_then(|s| s.char_indices().next_back())
            .map_or(0, |(i, _)| i)
    }

    /// One char left, stopping at the line start (the `h` motion).
    fn offset_left(&self, pos: usize) -> usize {
        match self.buf[..pos].char_indices().next_back() {
            Some((i, c)) if c != '\n' => i,
            _ => pos,
        }
    }

    /// One char right, stopping at the line end (the `l` motion).
    fn offset_right(&self, pos: usize) -> usize {
        match self.char_at(pos) {
            Some(c) if c != '\n' => pos + c.len_utf8(),
            _ => pos,
        }
    }

    /// Apply an offset step `n` times.
    fn repeat(&self, n: usize, from: usize, step: fn(&Self, usize) -> usize) -> usize {
        let mut at = from;
        for _ in 0..n {
            at = step(self, at);
        }
        at
    }

    /// The 0-based line the cursor is on.
    fn cursor_line(&self) -> usize {
        self.buf[..self.cursor].matches('\n').count()
    }

    /// Total line count (a trailing newline does not open a new line).
    fn line_count(&self) -> usize {
        self.buf.matches('\n').count() + 1
    }

    /// Byte offset of `line`/`col`, both clamped.
    fn line_col_offset(&self, line: usize, col: usize) -> usize {
        let last = self.line_count().saturating_sub(1);
        let line = line.min(last);
        let mut start = 0usize;
        for _ in 0..line {
            start = self.line_end(start);
            if start < self.buf.len() {
                start += 1;
            }
        }
        let end = self.line_end(start);
        let mut pos = start;
        for c in self.buf[start..end].chars().take(col) {
            pos += c.len_utf8();
        }
        pos
    }

    /// The first non-blank char of the line containing `pos` (`^`).
    fn first_non_blank(&self, pos: usize) -> usize {
        let start = self.line_start(pos);
        let end = self.line_end(pos);
        let indent = self.buf[start..end]
            .char_indices()
            .find(|&(_, c)| !c.is_whitespace())
            .map_or(0, |(i, _)| i);
        start + indent
    }

    /// `g_`: the last non-blank char of the line.
    fn first_non_blank_backwards_from_line_end(&self) -> usize {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        self.buf[start..end]
            .char_indices()
            .rfind(|&(_, c)| !c.is_whitespace())
            .map_or(start, |(i, _)| start + i)
    }

    /// `w`/`W` from `pos`: past the current run, then past the blanks.
    fn word_forward_from(&self, pos: usize, big: bool) -> usize {
        let mut at = pos;
        if let Some(c) = self.char_at(at) {
            let class = char_class(c, big);
            if class != CharClass::Blank {
                while let Some(c) = self.char_at(at) {
                    if char_class(c, big) == class {
                        at += c.len_utf8();
                    } else {
                        break;
                    }
                }
            }
        }
        while let Some(c) = self.char_at(at) {
            if char_class(c, big) == CharClass::Blank {
                at += c.len_utf8();
            } else {
                break;
            }
        }
        at
    }

    /// `b`/`B` from `pos`: back over blanks, then to the run's start.
    fn word_backward_from(&self, pos: usize, big: bool) -> usize {
        let mut at = self.prev_offset(pos);
        if at == pos {
            return 0;
        }
        while at > 0 {
            match self.char_at(at) {
                Some(c) if char_class(c, big) == CharClass::Blank => at = self.prev_offset(at),
                _ => break,
            }
        }
        let Some(class) = self
            .char_at(at)
            .map(|c| char_class(c, big))
            .filter(|c| *c != CharClass::Blank)
        else {
            return at;
        };
        while let Some((i, c)) = self.buf[..at].char_indices().next_back() {
            if char_class(c, big) == class {
                at = i;
            } else {
                break;
            }
        }
        at
    }

    /// `e`/`E` from `pos`: forward to the end of the next run.
    fn word_end_from(&self, pos: usize, big: bool) -> usize {
        let mut at = self.next_offset(pos);
        while let Some(c) = self.char_at(at) {
            if char_class(c, big) == CharClass::Blank {
                at = self.next_offset(at);
            } else {
                break;
            }
        }
        let Some(class) = self.char_at(at).map(|c| char_class(c, big)) else {
            return self.prev_offset(self.buf.len());
        };
        while let Some(c) = self.char_at(self.next_offset(at)) {
            if char_class(c, big) == class && self.next_offset(at) != at {
                at = self.next_offset(at);
            } else {
                break;
            }
        }
        at
    }

    /// `ge`/`gE`: backwards to the previous run's end.
    fn word_end_back(&self, pos: usize, big: bool, n: usize) -> usize {
        let mut at = pos;
        for _ in 0..n {
            at = self.prev_offset(at);
            while at > 0 {
                match self.char_at(at) {
                    Some(c) if char_class(c, big) == CharClass::Blank => at = self.prev_offset(at),
                    _ => break,
                }
            }
        }
        at
    }

    /// `}`: the next blank line, or the last line.
    fn paragraph_forward(&self, n: usize) -> usize {
        let last = self.line_count().saturating_sub(1);
        let mut line = self.cursor_line();
        for _ in 0..n {
            let mut next = line + 1;
            while next <= last && !self.line_is_blank(next) {
                next += 1;
            }
            line = next.min(last);
        }
        self.line_col_offset(line, 0)
    }

    /// `{`: the previous blank line, or line 0.
    fn paragraph_backward(&self, n: usize) -> usize {
        let mut line = self.cursor_line();
        for _ in 0..n {
            let mut prev = line;
            loop {
                if prev == 0 {
                    break;
                }
                prev -= 1;
                if self.line_is_blank(prev) {
                    break;
                }
            }
            line = prev;
        }
        self.line_col_offset(line, 0)
    }

    /// True when `line` holds nothing but whitespace.
    fn line_is_blank(&self, line: usize) -> bool {
        let start = self.line_col_offset(line, 0);
        self.buf[start..self.line_end(start)].trim().is_empty()
    }

    /// Store yanked or deleted text, honouring a `"x` prefix.
    ///
    /// The unnamed register is always written — vim's rule, and what
    /// keeps a bare `p` working after `"ayy`. An uppercase name appends
    /// to the lowercase register instead of replacing it.
    fn set_register(&mut self, text: String, linewise: bool) {
        if let Some(name) = self.target_register.take() {
            let lower = name.to_ascii_lowercase();
            if name.is_uppercase() {
                let entry = self
                    .registers
                    .entry(lower)
                    .or_insert_with(|| (String::new(), linewise));
                entry.0.push_str(&text);
                entry.1 = linewise;
            } else {
                self.registers.insert(lower, (text.clone(), linewise));
            }
        }
        self.register = text;
        self.linewise = linewise;
    }

    /// The text a paste takes, honouring a `"x` prefix.
    fn take_register(&mut self) -> (String, bool) {
        match self.target_register.take() {
            Some(name) => self
                .registers
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
            None => (self.register.clone(), self.linewise),
        }
    }

    /// Normalise a linewise register to always end in a newline.
    fn as_lines(text: &str) -> String {
        let mut out = text.to_owned();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// The stroke of the chord in progress, if any.
    #[must_use]
    pub const fn pending_stroke(&self) -> Option<char> {
        // An open search line is mid-chord too: without this the shells
        // would read its `Esc` as "cancel the whole edit".
        match self.search_input {
            Some((true, _)) => Some('/'),
            Some((false, _)) => Some('?'),
            None => self.pending.stroke(),
        }
    }

    /// The whole chord in progress as the user typed it (`2d3i`) —
    /// what a shell echoes in its status line. Empty when idle.
    ///
    /// The editor renders this itself rather than handing shells the
    /// pieces, so the terminal and the GUI show the same thing (I4).
    #[must_use]
    pub fn pending_chord(&self) -> String {
        if let Some(prompt) = self.search_prompt() {
            return prompt;
        }
        let mut out = String::new();
        if self.op_count > 0 {
            out.push_str(&self.op_count.to_string());
        }
        if let Some(op) = self.pending_op() {
            out.push(op);
        }
        if self.count > 0 {
            out.push_str(&self.count.to_string());
        }
        match self.pending {
            Pending::G(_) => out.push('g'),
            Pending::Obj { around, .. } => out.push(if around { 'a' } else { 'i' }),
            Pending::Find { kind, .. } => out.push(kind),
            Pending::Replace => out.push('r'),
            Pending::Register => out.push('"'),
            Pending::Mark => out.push('m'),
            Pending::JumpMark { linewise, .. } => out.push(if linewise { '\'' } else { '`' }),
            Pending::RecordMacro => out.push('q'),
            Pending::RunMacro => out.push('@'),
            Pending::Op(_) | Pending::None => {}
        }
        out
    }

    /// The operator armed by the chord in progress, if any.
    const fn pending_op(&self) -> Option<char> {
        match self.pending {
            Pending::Op(c) => Some(c),
            Pending::G(op)
            | Pending::Obj { op, .. }
            | Pending::Find { op, .. }
            | Pending::JumpMark { op, .. } => op,
            Pending::Replace
            | Pending::Register
            | Pending::Mark
            | Pending::RecordMacro
            | Pending::RunMacro
            | Pending::None => None,
        }
    }

    /// The pending vim count (0 = none) - the caller's cancel guard.
    #[must_use]
    pub const fn pending_count(&self) -> usize {
        self.count
    }

    /// Consume the count typed for a plain motion or edit.
    fn take_count(&mut self) -> usize {
        self.count_given = self.count > 0;
        let n = self.count.max(1);
        self.count = 0;
        n
    }

    /// Consume both halves of `[count]op[count]motion` — vim multiplies
    /// them, so `2d3w` deletes six words.
    fn take_count_for_op(&mut self) -> usize {
        self.count_given = self.count > 0 || self.op_count > 0;
        let n = self.count.max(1) * self.op_count.max(1);
        self.count = 0;
        self.op_count = 0;
        n
    }

    /// Record the current state before a mutating edit (bounded at 50).
    fn checkpoint(&mut self) {
        // Every buffer change funnels through here, which makes this the
        // one honest signal that a command was a *change* (what `.`
        // repeats) rather than a motion.
        self.edit_seq = self.edit_seq.wrapping_add(1);
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
        let (text, linewise) = self.take_register();
        if text.is_empty() {
            return;
        }
        if linewise {
            let end = self.line_end(self.cursor);
            let text = format!("\n{}", text.trim_end_matches('\n'));
            self.buf.insert_str(end, &text);
            self.cursor = end + 1;
        } else {
            let pos = self.buf[self.cursor..]
                .chars()
                .next()
                .filter(|c| *c != '\n')
                .map_or(self.cursor, |c| self.cursor + c.len_utf8());
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
    let sources: Vec<String> = vault.iter().map(|(_p, doc)| doc.source()).collect();
    body_completions_from(prefix, sources.iter().map(String::as_str))
}

/// [`body_completions`] over raw document sources, for a shell that
/// holds text rather than a [`closure_store::Vault`] — the terminal
/// shell keeps the vault in its driver, not in its app state.
#[must_use]
pub fn body_completions_from<'a>(
    prefix: &str,
    sources: impl Iterator<Item = &'a str>,
) -> Vec<String> {
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
    for source in sources {
        for word in source.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
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
// Four independent facts about the session — quitting, whether a row is
// selected, whether an answer is in flight, whether to re-render it.
// They are orthogonal and each is genuinely two-valued; an enum over
// their sixteen combinations would describe the same thing worse.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct ModalApp {
    mode: InputMode,
    surface: ModalSurface,
    selected: usize,
    query: String,
    /// The capture overlay's one-line field (text + cursor).
    capture_buf: LineInput,
    body: BodyEditor,
    completion: Option<CompletionSession>,
    edit_target: Option<String>,
    /// Path of the file open in the editor view, when one is.
    file_target: Option<std::path::PathBuf>,
    /// Which shape of the shell we are in ([`ViewMode`]).
    view: ViewMode,
    /// The body as it was when the editor opened it, so
    /// [`Self::body_dirty`] can answer by comparison rather than with
    /// a "was touched" bit — a buffer put back the way it was found
    /// has nothing to save.
    body_baseline: String,
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
    /// How many body lines the shell last said it can paint. The
    /// kernel decides *where* the viewport sits and the shell knows how
    /// big it is, so the shell reports it ([`Self::set_body_viewport`])
    /// and the framing chords read it back.
    body_viewport: usize,
    /// The first visible line as last resolved by [`Self::body_scroll_follow`],
    /// which is what "scroll by the minimum" is measured from.
    body_anchor: Option<usize>,
    /// `C-l`'s place in the centre → top → bottom cycle, with the
    /// framing it produced — a press that finds the viewport somewhere
    /// else is a first press, not the next one.
    recenter: Option<(u8, usize, usize)>,
    /// A body-editor prefix key waiting for the rest of its chord.
    pending_body: Option<BodyPrefix>,
    /// Where the cursor was left in each body, by block id, so opening
    /// a note again resumes rather than restarting at byte zero.
    body_cursors: std::collections::HashMap<String, usize>,
    /// The outline row the search overlay was opened from, so Esc can
    /// put the cursor back rather than leaving it on a result index.
    search_return: Option<usize>,
    /// Buffer text scale, in [`ZOOM_STEP`] powers.
    zoom_steps: i8,
    /// Whether a row is *actually* selected, as opposed to the cursor
    /// merely sitting somewhere. Escape in the outline clears it, which
    /// is how a capture is told "file this at the top level, not under
    /// whatever I happened to be looking at"; any motion selects again.
    selection_active: bool,
    /// Body-editor wheel viewport `(start, cursor_line_when_set)`; the
    /// override self-clears when the cursor line changes (G5).
    body_scroll: Option<(usize, usize)>,
    /// Cursor row inside the `UndoHistory` pane (Q2-U3).
    hist_cursor: usize,
    /// The Notion "/" block menu's query while it is open, and its
    /// cursor row. `None` when closed.
    slash: Option<(String, usize)>,
    /// The `:` command line's buffer while it is open.
    ex_buf: String,
    /// Surface the `:` line was opened from, so Escape returns there.
    ex_return: Option<ModalSurface>,
    /// The live org-edit-special session: where it came from and the
    /// language it is editing.
    special: Option<(SpecialOrigin, String)>,
    /// Surface the edit-special session was opened from.
    special_return: Option<ModalSurface>,
    /// Output of the last source block run from the Blocks surface.
    /// Cleared whenever the cursor moves or the pane closes, because
    /// output shown beside a block that did not produce it is a lie.
    block_out: Option<String>,
    /// Collaboration state, created on first use so a shell that never
    /// pairs never generates a keypair.
    sync: Option<SyncApp>,
    /// Where pairing binds and what it advertises, from `config.org`.
    /// Held here rather than in [`SyncApp`] because the shell reads the
    /// config long before anything pairs, and creating the state early
    /// would generate a keypair for a session that never asked to.
    sync_bind: std::net::SocketAddr,
    sync_advertise: Option<std::net::IpAddr>,
    /// The ticket-entry field on the Sync surface.
    sync_buf: String,
    /// The assistant transcript, oldest first.
    chat: Vec<ChatTurn>,
    /// The question field on the assistant surface.
    chat_buf: String,
    /// Whether a question is in flight, so the pane can say so rather
    /// than looking asleep.
    chat_busy: bool,
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
    /// Memoised detail for the selected row — see [`ModalApp::detail`].
    detail_memo: std::cell::RefCell<Option<DetailMemo>>,
    /// Derivations paid for; the render budget's second number.
    detail_recomputes: std::cell::Cell<u64>,
    /// Memoised palette entries — see [`ModalApp::palette_entries`].
    palette_memo: std::cell::RefCell<Option<PaletteMemo>>,
    /// Derivations paid for; the render budget's third number.
    palette_recomputes: std::cell::Cell<u64>,
    /// Memoised source-block list — see [`ModalApp::block_rows`].
    block_memo: std::cell::RefCell<Option<(u64, std::sync::Arc<Vec<BlockRow>>)>>,
    /// Derivations paid for; the render budget's fourth number.
    block_recomputes: std::cell::Cell<u64>,
}

/// A filled detail memo with the key it is valid under.
#[derive(Debug, Clone)]
struct DetailMemo {
    /// Vault revision the detail was read at.
    revision: u64,
    /// Block id it describes (`None` when there is no selection).
    id: Option<String>,
    /// The derived detail, shared so a repaint costs a refcount bump
    /// rather than cloning a whole headline body.
    detail: Option<std::sync::Arc<Detail>>,
}

/// A filled palette memo with the key it is valid under.
#[derive(Debug, Clone)]
struct PaletteMemo {
    /// The filter the entries were scored against.
    query: String,
    /// The mode whose chords they carry.
    mode: InputMode,
    /// The derived entries.
    entries: std::sync::Arc<Vec<PaletteEntry>>,
}

/// One listed source block: `(file, language, first line)`.
pub type BlockRow = (String, String, String);

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
            ex_buf: String::new(),
            ex_return: None,
            special: None,
            special_return: None,
            block_out: None,
            sync: None,
            sync_bind: DEFAULT_SYNC_BIND,
            sync_advertise: None,
            sync_buf: String::new(),
            chat: Vec::new(),
            chat_buf: String::new(),
            chat_busy: false,
            sniffer: SnifferApp::new(),
            conflicts: ConflictApp::new(Vec::new(), mode),
            llm_render: false,
            mode,
            surface: ModalSurface::Browse,
            selected: 0,
            query: String::new(),
            capture_buf: LineInput::default(),
            body: BodyEditor::new(),
            body_baseline: String::new(),
            completion: None,
            edit_target: None,
            file_target: None,
            // Every window opens on the outline: it is where the rail
            // and every affordance live, and a shell that opened
            // straight into a raw file buffer would hide the app from
            // anyone who had not asked for that. `set_view` is how a
            // vault config (`view = editor`) asks for it, and
            // `toggle-view` is how a keyboard does.
            view: ViewMode::Clickable,
            link_target: None,
            field_target: None,
            field_buf: String::new(),
            palette_cursor: 0,
            pending: Vec::new(),
            status: String::new(),
            quit: false,
            scroll_override: None,
            body_viewport: BODY_VIEWPORT_DEFAULT,
            body_anchor: None,
            recenter: None,
            pending_body: None,
            selection_active: true,
            zoom_steps: 0,
            search_return: None,
            body_cursors: std::collections::HashMap::new(),
            body_scroll: None,
            hist_cursor: 0,
            row_memo: std::cell::RefCell::new(None),
            row_recomputes: std::cell::Cell::new(0),
            detail_memo: std::cell::RefCell::new(None),
            detail_recomputes: std::cell::Cell::new(0),
            palette_memo: std::cell::RefCell::new(None),
            palette_recomputes: std::cell::Cell::new(0),
            block_memo: std::cell::RefCell::new(None),
            block_recomputes: std::cell::Cell::new(0),
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
        self.palette_shared().as_ref().clone()
    }

    /// The same entries, shared rather than cloned — the render path's
    /// entry point.
    ///
    /// Building them scores every command against the query and walks
    /// the keymap once per entry to find its chord, allocating a
    /// handful of `String`s each time. The palette pane asked for that
    /// on every frame *and* twice per keystroke, which is why it was
    /// the slowest surface to scroll. Memoised against
    /// `(query, mode)` — the mode matters because every entry carries
    /// the chord for the *active* mode.
    #[must_use]
    pub fn palette_shared(&self) -> std::sync::Arc<Vec<PaletteEntry>> {
        {
            let memo = self.palette_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.query == self.field_buf
                && m.mode == self.mode
            {
                return std::sync::Arc::clone(&m.entries);
            }
        }
        let entries = std::sync::Arc::new(self.palette_entries_uncached());
        self.palette_recomputes
            .set(self.palette_recomputes.get() + 1);
        *self.palette_memo.borrow_mut() = Some(PaletteMemo {
            query: self.field_buf.clone(),
            mode: self.mode,
            entries: std::sync::Arc::clone(&entries),
        });
        entries
    }

    /// Ground truth: build the palette without consulting or filling
    /// the memo. What [`Self::palette_shared`] must always agree with.
    #[must_use]
    pub fn palette_entries_uncached(&self) -> Vec<PaletteEntry> {
        command_palette(&self.field_buf, self.mode)
            .into_iter()
            .flat_map(|s| s.items)
            .collect()
    }

    /// How many times the palette has actually been rebuilt. The
    /// render budget is one per query or mode change.
    #[must_use]
    pub const fn palette_recomputes(&self) -> u64 {
        self.palette_recomputes.get()
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
                self.go_home();
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
        self.go_home();
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
        self.capture_buf.text()
    }
    /// Byte offset of the cursor in the capture field, so a shell can
    /// draw the caret where the next character will actually go.
    #[must_use]
    pub const fn capture_cursor(&self) -> usize {
        self.capture_buf.cursor()
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
                // The fold state is needed twice: to hide descendants
                // here, and by the outline to draw the arrow. Compute
                // it once and carry it on the row.
                let folded = headline_is_folded(h);
                if filter.is_empty() {
                    if let Some(limit) = hide_below {
                        if h.level() > limit {
                            continue;
                        }
                        hide_below = None;
                    }
                    if folded {
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
                            folded,
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
        // Clicking a row is the least ambiguous way there is of saying
        // "this one".
        self.selection_active = true;
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
        self.detail_shared(shell).map(|d| d.as_ref().clone())
    }

    /// The same detail, shared rather than cloned — the render path's
    /// entry point.
    ///
    /// Deriving it copies the whole headline: body text, tags,
    /// properties. Scrolling the outline repaints the detail pane
    /// without changing the selection, so it is memoised against
    /// `(vault revision, selected id)` — the two things it reads.
    #[must_use]
    pub fn detail_shared(&self, shell: &Shell) -> Option<std::sync::Arc<Detail>> {
        let revision = shell.vault.revision();
        let id = self
            .rows_shared(shell)
            .get(self.selected)
            .map(|r| r.id.clone());
        {
            let memo = self.detail_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.revision == revision
                && m.id == id
            {
                return m.detail.clone();
            }
        }
        let detail = Self::derive_detail(shell, id.as_deref()).map(std::sync::Arc::new);
        self.detail_recomputes.set(self.detail_recomputes.get() + 1);
        *self.detail_memo.borrow_mut() = Some(DetailMemo {
            revision,
            id,
            detail: detail.clone(),
        });
        detail
    }

    /// How many times the detail has actually been read out of the
    /// vault. The render budget is one per selection change.
    #[must_use]
    pub const fn detail_recomputes(&self) -> u64 {
        self.detail_recomputes.get()
    }

    /// The uncached derivation behind [`Self::detail_shared`].
    fn derive_detail(shell: &Shell, id: Option<&str>) -> Option<Detail> {
        let bid = closure_core::BlockId::from_existing(id?);
        let (h, path) = shell.vault.find_by_id(&bid)?;
        Some(Detail {
            title: h.title().to_owned(),
            todo: h.todo().map(ToOwned::to_owned),
            priority: h.priority(),
            tags: h.tags().to_vec(),
            scheduled: h.scheduled().map(ToOwned::to_owned),
            deadline: h.deadline().map(ToOwned::to_owned),
            properties: h.properties().to_vec(),
            // The body is shown and edited as the author wrote it; the
            // comma escape that keeps a `* line` out of the outline is
            // an on-disk spelling ([`closure_org::escape_body`]).
            body: closure_org::unescape_body(h.body_text()),
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
            ModalSurface::Search => self.on_search_key(shell, key, ctrl, alt, text),
            ModalSurface::Capture => self.on_capture_key(shell, key, ctrl, alt, text),
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
            ModalSurface::Ex => self.on_ex_key(shell, key, text),
            ModalSurface::Sync => self.on_sync_key(key, text),
            ModalSurface::Llm => self.on_llm_key(key, text),
            ModalSurface::Graph => {
                let len = self.hub_rows(shell).len() + self.orphan_rows(shell).len();
                self.on_pane_key(key, len);
            }
            ModalSurface::Journal => {
                let len = self.journal_rows(shell).len();
                self.on_pane_key(key, len);
            }
            ModalSurface::Cron => {
                let len = self.cron_rows(shell).len();
                self.on_pane_key(key, len);
            }
            ModalSurface::EditBlock => self.on_editblock_key(shell, key, ctrl, alt, text),
            ModalSurface::EditFile => self.on_editfile_key(shell, key, ctrl, alt, text),
            ModalSurface::Browse => self.on_browse_key(shell, key, ctrl, alt, text),
        }
    }

    /// The `:` command line's buffer while it is open.
    #[must_use]
    pub fn ex_buffer(&self) -> &str {
        &self.ex_buf
    }

    /// Keys for the org-edit-special session.
    ///
    /// The editing vocabulary is the body editor's, verbatim — same
    /// modes, same motions — because it *is* the body editor with a
    /// different buffer in it. Only the two exits differ: `C-Enter`
    /// writes the block back, and a quiet `Esc` in NORMAL discards the
    /// session rather than cancelling a body edit.
    fn on_editblock_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if key == "enter" && ctrl {
            self.commit_edit_special(shell);
            return;
        }
        if key == "escape"
            && self.body.mode() == EditorMode::Normal
            && self.body.pending_stroke().is_none()
            && self.body.pending_count() == 0
        {
            self.cancel_edit_special();
            return;
        }
        self.edit_body_key(shell, key, ctrl, alt, text);
    }

    /// Whether this keystroke belongs to the `SPC` leader rather than
    /// to the buffer, and route it if so.
    ///
    /// Doom's leader is the thing a Doom user's hands know, and it has
    /// to work where they spend the session — inside the buffer. In
    /// NORMAL that costs nothing: `SPC` is evil's forward-char, which
    /// is exactly the binding Doom takes away from it. In INSERT it is
    /// a space, because there it is prose. Only the Doom keymap has a
    /// leader, so no other mode loses a keystroke to this.
    fn leader_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if self.mode != InputMode::Doom || self.body.mode() == EditorMode::Insert {
            return false;
        }
        if self.pending.is_empty() {
            // A `SPC` mid-chord belongs to the chord: `d` then `SPC` is
            // vim's "delete the next character", not a leader.
            let editor_busy = self.body.pending_stroke().is_some() || self.body.pending_count() > 0;
            if key != "space" || editor_busy {
                return false;
            }
        }
        // Opening a chord, or continuing one: the keymap dispatcher owns
        // the strokes until the chord resolves or dies.
        self.on_browse_key(shell, key, ctrl, alt, text);
        true
    }

    /// The file buffer's keys: the same editor, with `:w` semantics.
    ///
    /// `C-Enter` writes and *stays* — a file you are editing is a file
    /// you keep editing, unlike a body, where the commit is the end of
    /// the errand. `Esc` out of NORMAL leaves without writing.
    fn on_editfile_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if key == "enter" && ctrl {
            self.commit_file_buffer(shell);
            return;
        }
        if key == "escape"
            && self.body.mode() == EditorMode::Normal
            && self.body.pending_stroke().is_none()
            && self.body.pending_count() == 0
        {
            self.view = ViewMode::Clickable;
            self.close_file_buffer();
            return;
        }
        self.edit_body_key(shell, key, ctrl, alt, text);
    }

    /// Headlines ranked by how many links point at them, most first,
    /// as `(id, title, count)`.
    ///
    /// The hubs of the vault: what everything else refers back to. A
    /// title comes along because an id alone is not a thing anyone can
    /// read.
    #[must_use]
    pub fn hub_rows(&self, shell: &Shell) -> Vec<(String, String, usize)> {
        let mut counts: std::collections::HashMap<closure_core::BlockId, usize> =
            std::collections::HashMap::new();
        for targets in shell.vault.link_graph().values() {
            for t in targets {
                *counts.entry(t.clone()).or_insert(0) += 1;
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        // Count descending, then id, so equal counts do not shuffle
        // between frames.
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
        });
        ranked
            .into_iter()
            .map(|(id, n)| {
                let title = shell
                    .vault
                    .find_by_id(&id)
                    .map_or_else(|| "?".to_owned(), |(h, _)| h.title().to_owned());
                (id.to_string(), title, n)
            })
            .collect()
    }

    /// Headlines nothing links to, as `(id, title)` — the far end of
    /// the same graph, and usually the interesting one.
    #[must_use]
    pub fn orphan_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        let mut targeted: std::collections::HashSet<closure_core::BlockId> =
            std::collections::HashSet::new();
        for targets in shell.vault.link_graph().values() {
            for t in targets {
                targeted.insert(t.clone());
            }
        }
        shell
            .vault
            .iter()
            .flat_map(|(_, doc)| doc.all_headlines())
            .filter(|h| !targeted.contains(h.id()))
            .map(|h| (h.id().to_string(), h.title().to_owned()))
            .collect()
    }

    /// Link targets that resolve to nothing — typos and things that
    /// were deleted out from under a reference.
    #[must_use]
    pub fn dead_link_rows(&self, shell: &Shell) -> Vec<String> {
        let mut out = Vec::new();
        for (path, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                for raw in h.link_targets() {
                    let Some(stripped) = raw.strip_prefix("id:") else {
                        continue;
                    };
                    if !shell
                        .vault
                        .has_id(&closure_core::BlockId::from_existing(stripped))
                    {
                        // The source matters as much as the target: a
                        // dead link is only fixable where it is
                        // written.
                        out.push(format!("{raw}  ← {} in {}", h.title(), path.display()));
                    }
                }
            }
        }
        out
    }

    /// The recorded command journal, newest last.
    #[must_use]
    pub fn journal_rows(&self, shell: &Shell) -> Vec<String> {
        closure_record::Journal::new(shell.vault.root(), true)
            .entries()
            .unwrap_or_default()
    }

    /// Scheduled jobs declared anywhere in the vault, as
    /// `(spec, command)`.
    ///
    /// A malformed block is skipped rather than fatal: a typo in one
    /// job must not make the pane unopenable.
    #[must_use]
    pub fn cron_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        shell
            .vault
            .iter()
            .filter_map(|(_, doc)| closure_cron::parse_jobs(&doc.source()).ok())
            .flatten()
            .map(|job| (format!("{:?}", job.spec), job.command))
            .collect()
    }

    /// Collaboration state, created on first use.
    ///
    /// Creating it generates a keypair, so a session that never pairs
    /// never generates one. It binds [`DEFAULT_SYNC_BIND`] unless
    /// [`Self::configure_sync`] has said otherwise.
    pub fn sync_mut(&mut self) -> &mut SyncApp {
        let (bind, advertise) = (self.sync_bind, self.sync_advertise);
        self.sync
            .get_or_insert_with(|| SyncApp::with_bind("local", bind, advertise))
    }

    /// Point pairing at a socket, from the vault's `config.org`.
    ///
    /// Called before the user pairs, and safe afterwards: an identity
    /// that already exists is moved rather than replaced, so a ticket
    /// handed out earlier keeps its key (see [`SyncApp::rebind`]).
    pub fn configure_sync(
        &mut self,
        bind: std::net::SocketAddr,
        advertise: Option<std::net::IpAddr>,
    ) {
        self.sync_bind = bind;
        self.sync_advertise = advertise;
        if let Some(sync) = self.sync.as_mut() {
            sync.rebind(bind, advertise);
        }
    }

    /// Collaboration state, or `None` until something has asked for it
    /// — merely *reading* must not be what generates a keypair.
    #[must_use]
    pub const fn sync(&self) -> Option<&SyncApp> {
        self.sync.as_ref()
    }

    /// The ticket-entry field on the Sync surface.
    #[must_use]
    pub fn sync_buffer(&self) -> &str {
        &self.sync_buf
    }

    /// The assistant transcript, oldest first.
    #[must_use]
    pub fn chat_turns(&self) -> &[ChatTurn] {
        &self.chat
    }

    /// The question field on the assistant surface.
    #[must_use]
    pub fn chat_buffer(&self) -> &str {
        &self.chat_buf
    }

    /// Whether a question is waiting on the provider.
    #[must_use]
    pub const fn chat_busy(&self) -> bool {
        self.chat_busy
    }

    /// Record a question and mark the conversation as waiting.
    pub fn chat_ask(&mut self, text: String) {
        self.chat.push(ChatTurn {
            from_user: true,
            text,
        });
        self.chat_busy = true;
    }

    /// Record an answer (or a failure) and stop waiting.
    pub fn chat_answer(&mut self, text: String) {
        self.chat.push(ChatTurn {
            from_user: false,
            text,
        });
        self.chat_busy = false;
    }

    /// Keys for the assistant: typing edits the question, Enter sends
    /// it, Escape leaves.
    fn on_llm_key(&mut self, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.go_home();
            }
            "backspace" => {
                self.chat_buf.pop();
            }
            "enter" => {
                let question = std::mem::take(&mut self.chat_buf);
                let question = question.trim();
                if !question.is_empty() {
                    self.chat_ask(question.to_owned());
                }
            }
            _ => {
                if let Some(c) = text {
                    self.chat_buf.push(c);
                }
            }
        }
    }

    /// What the vault's `config.org` says about the assistant, and
    /// whether it can be used right now.
    ///
    /// A chat box that silently does nothing because no provider is
    /// configured is the worst version of this feature, so the pane
    /// gets a sentence naming the keys to add. When a provider *is*
    /// set, the key's environment variable is checked too — configured
    /// and usable are different questions — and only ever named, never
    /// printed.
    #[must_use]
    pub fn llm_config_status(&self, shell: &Shell) -> LlmStatus {
        let cfg = closure_config::Config::from_path(&shell.vault.root().join("config.org")).ok();
        let provider = cfg.as_ref().and_then(|c| c.llm_provider.clone());
        let model = cfg.as_ref().and_then(|c| c.llm_model.clone());
        let endpoint = cfg.as_ref().and_then(|c| c.llm_endpoint.clone());
        let key_env = cfg.as_ref().and_then(|c| c.llm_key_env.clone());
        let Some(provider_name) = provider.clone() else {
            return LlmStatus {
                ready: false,
                provider,
                model,
                endpoint,
                detail: "no assistant configured — add `llm_provider` (and `llm_model`, \
                         `llm_key_env`) to the closure-config block in config.org"
                    .to_owned(),
            };
        };
        let model_name = model
            .clone()
            .unwrap_or_else(|| "(default model)".to_owned());
        if let Some(var) = key_env {
            if closure_llm::resolve_key(&var).is_none() {
                return LlmStatus {
                    ready: false,
                    provider,
                    model,
                    endpoint,
                    detail: format!(
                        "{provider_name} / {model_name} — ${var} is not set in this environment"
                    ),
                };
            }
            return LlmStatus {
                ready: true,
                provider,
                model,
                endpoint,
                detail: format!("{provider_name} / {model_name} — key from ${var}"),
            };
        }
        LlmStatus {
            ready: true,
            provider,
            model,
            endpoint: endpoint.clone(),
            detail: endpoint.map_or_else(
                || format!("{provider_name} / {model_name} — no key required"),
                |url| format!("{provider_name} / {model_name} — {url}"),
            ),
        }
    }

    /// Run one assistant tool line against the vault.
    ///
    /// The gate is the live one: `view-render` answers with the
    /// *current* view only while render access is granted, and starts
    /// refusing the moment it is revoked. A model that could still read
    /// the screen after the user said no would make the toggle a lie.
    /// Takes `&mut Shell` because some of these tools write — capture,
    /// rename, set-property — and all of them go through the same
    /// registry surface the chords do (I8).
    pub fn llm_tool(&self, shell: &mut Shell, line: &str) -> String {
        if line.trim() == closure_llm::RENDER_TOOL {
            if !self.llm_render {
                return format!(
                    "error: '{}' not allowed — render access is revoked (toggle-llm-render)",
                    closure_llm::RENDER_TOOL
                );
            }
            return serialize_view(&browse_view(&shell.vault));
        }
        shell.vault.run_tool(line)
    }

    /// Keys for the Sync surface: typing edits the ticket field, Enter
    /// adds the peer, Escape leaves.
    fn on_sync_key(&mut self, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.sync_buf.clear();
                self.go_home();
            }
            "backspace" => {
                self.sync_buf.pop();
            }
            "enter" => {
                let ticket = std::mem::take(&mut self.sync_buf);
                match self.sync_mut().add_peer(ticket.trim()) {
                    Ok(()) => {
                        let n = self.sync_mut().peers().len();
                        self.status = format!("peer added — {n} peer(s)");
                    }
                    Err(e) => {
                        // Keep the text so it can be corrected rather
                        // than retyped.
                        self.sync_buf = ticket;
                        self.status = format!("bad ticket: {e}");
                    }
                }
            }
            _ => {
                if let Some(c) = text {
                    self.sync_buf.push(c);
                }
            }
        }
    }

    /// Replace the status line — for a shell reporting something the
    /// core did not produce, such as what the pointer is hovering.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }

    /// The activity rail: every pane of the shell as a clickable
    /// destination, in a fixed order, each with its chord and its live
    /// badge.
    ///
    /// The status bar ([`Self::indicators`]) reports *state*; this
    /// reports *where you can go*, which is not the same list — the
    /// sniffer appears in both, pairing appeared in neither until this
    /// existed, and the outline needs a way home that Esc alone gave
    /// only the keyboard.
    #[must_use]
    pub fn destinations(&self, shell: &Shell) -> Vec<Destination> {
        RAIL.iter()
            .map(|&(id, icon, label, command, surface)| Destination {
                id,
                icon,
                label,
                command,
                chord: self.chord_for(command),
                surface,
                badge: self.rail_badge(shell, id).map(|n| n.to_string()),
                urgent: self.rail_urgent(id),
                active: self.surface == surface,
            })
            .collect()
    }

    /// What the rail button `id` counts, when a count is worth showing.
    /// `None` — and a zero — render no badge at all rather than a `0`.
    fn rail_badge(&self, shell: &Shell, id: &str) -> Option<usize> {
        let n = match id {
            "backlinks" => self.backlink_rows(shell).len(),
            "peers" => self.sync.as_ref().map_or(0, |s| s.peers().len()),
            "sniffer" => self.sniffer.events().len(),
            "conflicts" => self.conflicts.conflicts().len(),
            _ => 0,
        };
        (n > 0).then_some(n)
    }

    /// Whether the rail button `id` is counting work that waits on the
    /// user rather than mere activity.
    fn rail_urgent(&self, id: &str) -> bool {
        match id {
            "conflicts" => !self.conflicts.conflicts().is_empty(),
            "sniffer" => self
                .sniffer
                .events()
                .iter()
                .any(|e| e.action == Some(FlowAction::Block)),
            _ => false,
        }
    }

    /// The bottom-right status bar: one item per subsystem, each
    /// reporting its live state and carrying the chord that opens it.
    ///
    /// Derived here rather than assembled in a render function, so
    /// every shell shows the same set — and so "is the LLM allowed to
    /// read my screen?" has an answer you can see rather than one
    /// buried in a config file.
    #[must_use]
    pub fn indicators(&self, shell: &Shell) -> Vec<Indicator> {
        let item =
            |id, label: String, tooltip: String, level, command: Option<&'static str>| Indicator {
                id,
                label,
                tooltip,
                level,
                command,
                chord: command.and_then(|c| self.chord_for(c)),
            };
        let headlines = self.rows_shared(shell).len();
        let files = shell.vault.iter().count();
        let flows = self.sniffer.events().len();
        let blocked = self
            .sniffer
            .events()
            .iter()
            .filter(|e| e.action == Some(FlowAction::Block))
            .count();
        let conflicts = self.conflicts.conflicts().len();
        let blocks = self.block_rows(shell).len();
        vec![
            item(
                "vault",
                format!("⌂ {headlines}"),
                format!("{headlines} headline(s) across {files} file(s)"),
                IndicatorLevel::Idle,
                Some("headline-list"),
            ),
            item(
                "blocks",
                format!("⌗ {blocks}"),
                format!("{blocks} source block(s) — run one with eval-block"),
                IndicatorLevel::Idle,
                Some("block-list"),
            ),
            item(
                "sniffer",
                format!("⇅ {flows}"),
                if flows == 0 {
                    "network sniffer: no flows captured".to_owned()
                } else {
                    format!("network sniffer: {flows} flow(s), {blocked} blocked")
                },
                if blocked > 0 {
                    IndicatorLevel::Warn
                } else if flows > 0 {
                    IndicatorLevel::Active
                } else {
                    IndicatorLevel::Idle
                },
                Some("sniffer"),
            ),
            item(
                "llm",
                if self.llm_render {
                    "◉ llm".to_owned()
                } else {
                    "○ llm".to_owned()
                },
                if self.llm_render {
                    "LLM render access GRANTED — a model may read the rendered view".to_owned()
                } else {
                    "LLM render access revoked — a model cannot read the rendered view".to_owned()
                },
                if self.llm_render {
                    IndicatorLevel::Active
                } else {
                    IndicatorLevel::Idle
                },
                Some("toggle-llm-render"),
            ),
            item(
                "sync",
                format!("⇄ {conflicts}"),
                if conflicts == 0 {
                    "sync: every field converged".to_owned()
                } else {
                    format!("sync: {conflicts} conflict(s) awaiting a decision")
                },
                if conflicts > 0 {
                    IndicatorLevel::Warn
                } else {
                    IndicatorLevel::Idle
                },
                Some("conflicts"),
            ),
        ]
    }

    /// Language of the live org-edit-special session (empty when the
    /// block declared none, or when no session is open).
    #[must_use]
    pub fn special_language(&self) -> &str {
        self.special.as_ref().map_or("", |(_, lang)| lang.as_str())
    }

    /// Open the source block under the cursor on its own
    /// (org-edit-special).
    ///
    /// From the body editor the block is the one containing the
    /// cursor; from the Blocks list it is the row under it. Anywhere
    /// else there is no block to edit, and saying so beats opening an
    /// empty editor.
    fn begin_edit_special(&mut self, shell: &Shell) {
        match self.surface {
            ModalSurface::EditBody => self.begin_special_from_body(),
            ModalSurface::Blocks => self.begin_special_from_list(shell),
            _ => "edit-special: open a source block first (g e lists them)"
                .clone_into(&mut self.status),
        }
    }

    /// org-edit-special from the body editor: the block is inside the
    /// buffer, so the session remembers the buffer and the range to
    /// splice back into.
    fn begin_special_from_body(&mut self) {
        let buffer = self.body.text().to_owned();
        let cursor = self.body.cursor_byte();
        let Some((range, lang)) = enclosing_src_block(&buffer, cursor) else {
            "edit-special: the cursor is not inside a source block".clone_into(&mut self.status);
            return;
        };
        let content = buffer[range.clone()].to_owned();
        // Both entry points report the same language name: the Blocks
        // list already shows the normalised one, and a session that
        // called it `sh` from one door and `shell` from the other
        // would be reporting the door, not the block.
        self.special = Some((
            SpecialOrigin::Body {
                range,
                buffer,
                cursor,
            },
            normalise_lang(&lang),
        ));
        self.open_special(content);
    }

    /// org-edit-special from the Blocks list: the block belongs to a
    /// file, so the session remembers which one.
    fn begin_special_from_list(&mut self, shell: &Shell) {
        let rows = self.block_rows(shell);
        let Some((path, lang, _)) = rows.get(self.selected).cloned() else {
            "edit-special: no source blocks in this vault".clone_into(&mut self.status);
            return;
        };
        let index = rows[..self.selected]
            .iter()
            .filter(|(p, _, _)| *p == path)
            .count();
        let path = std::path::PathBuf::from(&path);
        let Some(content) = shell
            .vault
            .document(&path)
            .and_then(|doc| doc.org().code_blocks().get(index).copied())
            .and_then(|n| n.as_code_block().map(|cb| cb.content.to_owned()))
        else {
            "edit-special: could not read that block".clone_into(&mut self.status);
            return;
        };
        self.special = Some((SpecialOrigin::File { path, index }, lang));
        self.open_special(content);
    }

    /// Which shape of the shell is showing ([`ViewMode`]).
    #[must_use]
    pub const fn view_mode(&self) -> ViewMode {
        self.view
    }

    /// Put the shell in `view`, opening or closing the file buffer to
    /// match.
    ///
    /// [`ModalApp::new`] has no vault, so a config that asks to start in
    /// the editor view cannot be honoured at construction; the shells
    /// call this once they have one.
    pub fn set_view(&mut self, view: ViewMode, shell: &Shell) {
        self.view = view;
        match (view, self.surface) {
            (ViewMode::Editor, ModalSurface::Browse) => self.open_file_buffer(shell),
            (ViewMode::Clickable, ModalSurface::EditFile) => self.close_file_buffer(),
            _ => {}
        }
    }

    /// Open the selected row's file as one full-window buffer.
    ///
    /// The editor view *is* this: no rows, no detail pane — the file,
    /// the way `find-file` gives it to you. An empty vault has no file
    /// to open, so it stays where it is and says so.
    fn open_file_buffer(&mut self, shell: &Shell) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            "no file to open — the vault is empty".clone_into(&mut self.status);
            self.view = ViewMode::Clickable;
            return;
        };
        let id = closure_core::BlockId::from_existing(&row.id);
        let Some((_, path)) = shell.vault.find_by_id(&id) else {
            "that headline has no file on disk".clone_into(&mut self.status);
            self.view = ViewMode::Clickable;
            return;
        };
        let path = path.to_path_buf();
        let source = shell
            .vault
            .iter()
            .find(|(p, _)| *p == path)
            .map_or_else(String::new, |(_, doc)| doc.source());
        self.body_baseline.clone_from(&source);
        self.load_body(source);
        self.file_target = Some(path);
        self.surface = ModalSurface::EditFile;
        self.status = if self.modal_editing() {
            "file — NORMAL, i to insert, C-Enter save, Esc back".to_owned()
        } else {
            "file — C-Enter save, Esc back".to_owned()
        };
    }

    /// Leave the file buffer without writing it.
    fn close_file_buffer(&mut self) {
        self.file_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.surface = ModalSurface::Browse;
    }

    /// Write the file buffer back to its file, staying in it — `:w`,
    /// not `:wq`.
    fn commit_file_buffer(&mut self, shell: &mut Shell) {
        let Some(path) = self.file_target.clone() else {
            return;
        };
        let source = self.body.text().to_owned();
        match shell.vault.set_source(&path, &source) {
            Ok(()) => {
                self.body_baseline = source;
                self.invalidate_rows();
                self.selected = self
                    .selected
                    .min(self.rows_shared(shell).len().saturating_sub(1));
                self.status = format!("wrote {}", path.display());
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    /// What the open buffer is, for the shells to put where a modeline
    /// puts a buffer name. `None` when no buffer is open.
    ///
    /// A full-window buffer with no name is a wall of text you cannot
    /// place: which headline is this, and which file did it come from?
    /// Derived here so every shell names it identically.
    #[must_use]
    pub fn buffer_name(&self, shell: &Shell) -> Option<String> {
        if !self.surface.is_editor() {
            return None;
        }
        if let Some(path) = &self.file_target {
            return Some(path.display().to_string());
        }
        let detail = self.detail(shell)?;
        Some(if self.surface == ModalSurface::EditBlock {
            let lang = self.special_language();
            let lang = if lang.is_empty() { "src" } else { lang };
            format!("{lang} block — {} · {}", detail.title, detail.path)
        } else {
            format!("{} · {}", detail.title, detail.path)
        })
    }

    /// Whether the active input mode edits modally — whether there is a
    /// NORMAL for a buffer to open in at all.
    ///
    /// Notion and Emacs have no modal state, so a body they open is a
    /// text field and typing into it types. Vim, Doom and Helix do, and
    /// in those the first thing typed into a fresh buffer is a command.
    #[must_use]
    pub const fn modal_editing(&self) -> bool {
        matches!(
            self.mode,
            InputMode::Vim | InputMode::Doom | InputMode::Helix
        )
    }

    /// Which editor mode a freshly opened buffer lands in.
    const fn entry_mode(&self) -> EditorMode {
        if self.modal_editing() {
            EditorMode::Normal
        } else {
            EditorMode::Insert
        }
    }

    /// Load `text` into the body editor the way *this* input mode opens
    /// a buffer ([`Self::entry_mode`]).
    fn load_body(&mut self, text: String) {
        self.body.load_in(text, self.entry_mode());
    }

    /// Load `content` into the editor as an edit-special session.
    fn open_special(&mut self, content: String) {
        self.special_return = Some(self.surface);
        self.load_body(content);
        self.surface = ModalSurface::EditBlock;
        self.status = format!(
            "edit-special [{}] — C-Enter write back, Esc discard",
            self.special_language()
        );
    }

    /// Write the edited block back the way its origin requires.
    fn commit_edit_special(&mut self, shell: &mut Shell) {
        let Some((origin, _)) = self.special.take() else {
            return;
        };
        let edited = self.body.text().to_owned();
        match origin {
            SpecialOrigin::File { path, index } => {
                match shell.vault.set_block_content(&path, index, &edited) {
                    Ok(()) => "block written".clone_into(&mut self.status),
                    Err(e) => self.status = format!("edit-special failed: {e}"),
                }
                self.body.clear();
            }
            SpecialOrigin::Body {
                range,
                mut buffer,
                cursor,
            } => {
                // Splice into the body buffer; the ordinary body commit
                // is what carries it to disk.
                buffer.replace_range(range, &edited);
                self.load_body(buffer);
                self.body.set_cursor_byte(cursor);
                "block spliced — C-Enter again to save the body".clone_into(&mut self.status);
            }
        }
        self.surface = self.special_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Abandon the edit-special session, restoring what it replaced.
    fn cancel_edit_special(&mut self) {
        let origin = self.special.take().map(|(o, _)| o);
        if let Some(SpecialOrigin::Body { buffer, cursor, .. }) = origin {
            self.load_body(buffer);
            self.body.set_cursor_byte(cursor);
        } else {
            self.body.clear();
        }
        "edit-special discarded".clone_into(&mut self.status);
        self.surface = self.special_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Open the `:` command line.
    fn begin_ex(&mut self) {
        self.ex_buf.clear();
        self.ex_return = Some(self.surface);
        self.surface = ModalSurface::Ex;
        ":".clone_into(&mut self.status);
    }

    /// Keys for the `:` line: typing edits it, Enter runs it, Escape
    /// abandons it, and backspacing past the start closes it (the same
    /// rule as the `/` menu — deleting the trigger dismisses it).
    fn on_ex_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => self.close_ex(),
            "backspace" => {
                if self.ex_buf.pop().is_none() {
                    self.close_ex();
                }
            }
            "enter" => {
                let line = std::mem::take(&mut self.ex_buf);
                self.run_ex(shell, line.trim());
            }
            _ => {
                if let Some(c) = text {
                    self.ex_buf.push(c);
                }
            }
        }
    }

    /// Abandon the `:` line and return to where it was opened from.
    fn close_ex(&mut self) {
        self.ex_buf.clear();
        self.surface = self.ex_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Execute an ex command.
    ///
    /// The vim set first, then a fall-through to the registry, so `:`
    /// is a superset of the palette it replaced rather than a
    /// replacement for it.
    fn run_ex(&mut self, shell: &mut Shell, line: &str) {
        let editing = self.ex_return == Some(ModalSurface::EditBody);
        self.ex_buf.clear();
        self.ex_return = None;
        self.surface = ModalSurface::Browse;
        match line {
            "" => {}
            // The bang is the whole point of the bang: `:q` will not
            // take an unfinished paragraph with it, `:q!` will.
            "q" | "quit" => {
                if self.refuse_quit_when_dirty() {
                    self.surface = ModalSurface::EditBody;
                } else {
                    self.quit = true;
                }
            }
            "q!" | "quit!" => self.quit = true,
            "w" | "write" | "wq" | "x" | "wq!" | "x!" => {
                if editing {
                    // `:w` in every vi ever written means "write and
                    // carry on"; only the `q` half leaves. The ex line
                    // returns to Browse before running its command, so
                    // a plain write used to close the buffer it had
                    // just saved.
                    if line == "w" || line == "write" {
                        self.write_body(shell);
                        self.surface = ModalSurface::EditBody;
                    } else {
                        self.commit_edit_body(shell);
                    }
                } else {
                    // And here it does not, and saying "written" would
                    // be a lie about a write that never happened —
                    // every edit goes through the kernel to disk (I8).
                    "the vault is written on every edit — nothing to save"
                        .clone_into(&mut self.status);
                }
                if line.starts_with("wq") || line.starts_with('x') {
                    self.quit = true;
                }
            }
            other => {
                // Anything else is a command name. Resolve it against
                // the registry the palette and the chords share (I4).
                let known = closure_input::mode_keymap(self.mode)
                    .iter()
                    .any(|(_, cmd)| *cmd == other)
                    || command_palette("", self.mode)
                        .iter()
                        .flat_map(|s| &s.items)
                        .any(|e| e.action.command() == other);
                if known {
                    if editing {
                        self.commit_edit_body(shell);
                    }
                    self.run_command(shell, other);
                } else {
                    self.status = format!("not an editor command: {other}");
                }
            }
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
                self.go_home();
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
                self.go_home();
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
            "escape" | "q" => self.go_home(),
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
            "escape" | "q" => self.go_home(),
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
                self.go_home();
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
                self.go_home();
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
                // Block output belongs to the pane that produced it.
                self.block_out = None;
                self.go_home();
            }
            "down" | "j" => {
                self.block_out = None;
                self.selected = (self.selected + 1).min(len.saturating_sub(1));
            }
            "up" | "k" => {
                self.block_out = None;
                self.selected = self.selected.saturating_sub(1);
            }
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

    /// Report how many body lines the pane can paint.
    ///
    /// The framing chords (`C-l`, `zz`/`zt`/`zb`) have to answer "where
    /// is the middle of the screen", and only the shell knows how big
    /// the screen is. Called once per frame by the painter; a shell
    /// that never calls it gets [`BODY_VIEWPORT_DEFAULT`].
    pub const fn set_body_viewport(&mut self, lines: usize) {
        if lines > 0 {
            self.body_viewport = lines;
        }
    }

    /// The viewport height the shell last reported.
    #[must_use]
    pub const fn body_viewport(&self) -> usize {
        self.body_viewport
    }

    /// Put the body cursor at the start of `line`, clamped to the
    /// buffer — what a jump (a search hit, `G`, a followed link) does
    /// before the viewport is asked to follow it.
    pub fn body_goto_line(&mut self, line: usize) {
        let text = self.body.text();
        let mut at = 0usize;
        for (n, l) in text.split('\n').enumerate() {
            if n == line {
                break;
            }
            at += l.len() + 1;
        }
        self.body.set_cursor_byte(at.min(text.len()));
        self.completion = None;
    }

    /// Resolve the first visible body line for this frame, moving the
    /// viewport the way Doom's settings say to.
    ///
    /// `lisp/doom-emacs.el` sets `scroll-margin 0` and
    /// `scroll-conservatively 10`: an ordinary move gets no forced
    /// context and scrolls by the minimum, and a *jump* — further than
    /// ten lines off the edge — recentres instead, because a line
    /// pinned to the bottom edge of the pane has nothing under it to
    /// read. The old rule always parked the cursor on the last visible
    /// line, which made every search hit land at the very bottom.
    ///
    /// Stateful by necessity ("scroll by the minimum" is measured from
    /// where the viewport already was), so this is the painter's entry
    /// point; [`Self::body_scroll_start`] stays the pure reader.
    pub fn body_scroll_follow(&mut self, viewport: usize) -> usize {
        /// `scroll-conservatively`: further than this is a jump.
        const CONSERVATIVELY: usize = 10;
        let (cursor, _) = self.body.cursor_line_col();
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        if let Some((start, at)) = self.body_scroll
            && at == cursor
        {
            self.body_anchor = Some(start);
            return start;
        }
        let previous = self
            .body_anchor
            .unwrap_or_else(|| self.body_scroll_start(viewport));
        let start = if (previous..previous + viewport).contains(&cursor) {
            // Already on screen: Emacs does not move the window, and
            // neither does anything else worth using.
            previous
        } else {
            let distance = if cursor < previous {
                previous - cursor
            } else {
                cursor + 1 - (previous + viewport)
            };
            if distance <= CONSERVATIVELY {
                if cursor < previous {
                    cursor
                } else {
                    cursor + 1 - viewport
                }
            } else {
                cursor.saturating_sub(viewport / 2)
            }
        }
        .min(max);
        self.body_anchor = Some(start);
        self.body_scroll = Some((start, cursor));
        start
    }

    /// `C-l`: cycle the cursor line through centre, top and bottom.
    ///
    /// Emacs' `recenter-top-bottom`, which Doom keeps. A press that
    /// finds the viewport somewhere other than where the last press
    /// left it starts the cycle over — the cycle is about *this* line
    /// in *this* framing, and a motion invalidates both.
    pub fn body_recenter_cycle(&mut self) {
        let viewport = self.body_viewport;
        let (cursor, _) = self.body.cursor_line_col();
        let current = self.body_scroll_start(viewport);
        let step = match self.recenter {
            Some((step, start, at)) if start == current && at == cursor => (step + 1) % 3,
            _ => 0,
        };
        let framing = match step {
            0 => BodyFraming::Centre,
            1 => BodyFraming::Top,
            _ => BodyFraming::Bottom,
        };
        let start = self.frame_body(framing);
        self.recenter = Some((step, start, cursor));
    }

    /// `zz` / `zt` / `zb`: put the cursor line in the middle, at the
    /// top or at the bottom, saying which rather than cycling.
    pub fn body_frame(&mut self, framing: BodyFraming) {
        let start = self.frame_body(framing);
        let (cursor, _) = self.body.cursor_line_col();
        // A framing chord is also a fresh starting point for `C-l`.
        self.recenter = Some((0, start, cursor));
    }

    /// Park the viewport so the cursor line sits where `framing` says,
    /// returning the resolved first visible line.
    fn frame_body(&mut self, framing: BodyFraming) -> usize {
        let viewport = self.body_viewport;
        let (cursor, _) = self.body.cursor_line_col();
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let start = match framing {
            BodyFraming::Centre => cursor.saturating_sub(viewport / 2),
            BodyFraming::Top => cursor,
            BodyFraming::Bottom => (cursor + 1).saturating_sub(viewport),
        }
        .min(max);
        self.body_scroll = Some((start, cursor));
        self.body_anchor = Some(start);
        start
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
            self.selection_active = true;
            return true;
        }
        false
    }

    /// Move the outline selection to the first row titled `title`,
    /// reporting whether one was found.
    ///
    /// Org's fuzzy link — `[[Some Heading]]` — points at a title, not
    /// an id, and it is the spelling a person writes by hand. Matching
    /// is exact after trimming: a fuzzy match here would follow a link
    /// to the wrong note, which is worse than not following it.
    pub fn select_by_title(&mut self, shell: &Shell, title: &str) -> bool {
        let want = title.trim();
        if let Some(i) = self
            .rows_shared(shell)
            .iter()
            .position(|r| r.title.trim() == want)
        {
            self.selected = i;
            return true;
        }
        false
    }

    /// Move the outline selection into the file `path` names — to
    /// `title` within it, or to its first headline.
    ///
    /// `file:` links are written relative to wherever the link lives,
    /// so `./b.org`, `b.org` and `notes/b.org` can all mean the same
    /// file; the match is on the trailing path components, which is
    /// the most that can be resolved without knowing the linking
    /// file's own directory.
    pub fn select_in_file(&mut self, shell: &Shell, path: &str, title: Option<&str>) -> bool {
        let want = path.trim().trim_start_matches("./");
        let rows = self.rows_shared(shell);
        let found = rows.iter().position(|r| {
            let same_file = r.path == want
                || r.path.ends_with(&format!("/{want}"))
                || want.ends_with(&format!("/{}", r.path));
            same_file && title.is_none_or(|t| r.title.trim() == t.trim())
        });
        if let Some(i) = found {
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
    pub fn block_rows(&self, shell: &Shell) -> Vec<BlockRow> {
        self.block_rows_shared(shell).as_ref().clone()
    }

    /// The same source-block list, shared rather than cloned.
    ///
    /// Deriving it is expensive in a way that is easy to miss:
    /// `Document::source()` does not hand back a cached string, it
    /// *re-serialises the whole document*. Doing that for every file
    /// in the vault is fine once; doing it from the status bar on
    /// every frame meant re-printing the entire vault on every
    /// keystroke, which is precisely what typing lag feels like.
    /// Memoised on the vault revision — the only thing it reads.
    #[must_use]
    pub fn block_rows_shared(&self, shell: &Shell) -> std::sync::Arc<Vec<BlockRow>> {
        let revision = shell.vault.revision();
        {
            let memo = self.block_memo.borrow();
            if let Some((rev, rows)) = memo.as_ref()
                && *rev == revision
            {
                return std::sync::Arc::clone(rows);
            }
        }
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
        let rows = std::sync::Arc::new(out);
        self.block_recomputes.set(self.block_recomputes.get() + 1);
        *self.block_memo.borrow_mut() = Some((revision, std::sync::Arc::clone(&rows)));
        rows
    }

    /// How many times the source-block list has actually been
    /// re-serialised out of the vault. The render budget is one per
    /// change, and the reason the status bar is affordable.
    #[must_use]
    pub const fn block_recomputes(&self) -> u64 {
        self.block_recomputes.get()
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
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if key == "enter" && ctrl {
            self.commit_edit_body(shell);
            return;
        }
        // `:` outside INSERT is the ex line, the way vim's is. Inside
        // INSERT it is text — `:PROPERTIES:` and `12:30` are prose —
        // and inside an open `/` search it is part of the pattern.
        if text == Some(':')
            && self.body.mode() != EditorMode::Insert
            && self.body.search_prompt().is_none()
        {
            self.begin_ex();
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
                // The arrows moved the menu and the chords did not, so
                // in a modal mode the "/" menu was mouse-and-arrows
                // only — which is the one thing it must not be.
                "down" | "up" | "j" | "k" | "n" | "p" if key == "down" || key == "up" || ctrl => {
                    if let Some((query, cursor)) = self.slash.as_mut() {
                        let last = block_templates(query).len().saturating_sub(1);
                        *cursor = if matches!(key, "down" | "j" | "n") {
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

    /// TAB inside an org table: realign the whole table, then move to
    /// the next cell, wrapping to the row below at the end of a row.
    ///
    /// Returns whether the cursor was in a table at all — `false`
    /// leaves TAB to its other jobs.
    fn table_tab(&mut self) -> bool {
        let text = self.body.text().to_owned();
        let (line, col) = self.body.cursor_line_col();
        let Some(rows) = table_bounds(&text, line) else {
            return false;
        };
        // Realign first, so the offsets below describe the table the
        // user is about to land in rather than the one they left.
        let lines: Vec<&str> = text.lines().collect();
        let join = |slice: &[&str]| {
            slice.iter().fold(String::new(), |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            })
        };
        let aligned = align_table(&join(&lines[rows.clone()]));
        let rebuilt = format!(
            "{}{aligned}{}",
            join(&lines[..rows.start]),
            join(&lines[rows.end..])
        );
        let aligned_lines: Vec<&str> = rebuilt.lines().collect();

        // Where the cursor should land: the next cell on this row, or
        // the first cell of the row below.
        let row_text = aligned_lines.get(line).copied().unwrap_or("");
        let at = row_text
            .char_indices()
            .nth(col)
            .map_or(row_text.len(), |(b, _)| b);
        let (target_line, target_byte) = match next_table_cell(row_text, at) {
            Some(next) => (line, next),
            None if line + 1 < rows.end => {
                let below = aligned_lines.get(line + 1).copied().unwrap_or("");
                (line + 1, next_table_cell(below, 0).unwrap_or(0))
            }
            None => (line, at),
        };
        // Byte offset of the target within the whole buffer.
        let mut offset = 0usize;
        for l in aligned_lines.iter().take(target_line) {
            offset += l.len() + 1;
        }
        // A realign happens mid-edit, so it keeps the mode the user is
        // in: reloading into the *entry* mode would throw them out of
        // INSERT halfway through typing a cell.
        let mode = self.body.mode();
        self.body.load_in(rebuilt, mode);
        self.body.set_cursor_byte(offset + target_byte);
        true
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
    /// The chords that move the *viewport* rather than the text, taken
    /// before the mode split because `C-l` is bound globally in Emacs
    /// (Doom keeps it) and so recentres while typing as well.
    ///
    /// Returns whether the key was consumed.
    fn viewport_chord(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if ctrl && key == "l" {
            self.pending_body = None;
            self.body_recenter_cycle();
            return true;
        }
        // Doom's `text-scale`, on the same chords the rest of the
        // desktop uses. It scales the buffer, not the chrome — which is
        // what text-scale does too.
        if ctrl {
            match key {
                "+" | "=" => {
                    self.zoom_in();
                    self.status = format!("zoom {:.0}%", self.zoom() * 100.0);
                    return true;
                }
                "-" => {
                    self.zoom_out();
                    self.status = format!("zoom {:.0}%", self.zoom() * 100.0);
                    return true;
                }
                "0" => {
                    self.zoom_reset();
                    "zoom 100%".clone_into(&mut self.status);
                    return true;
                }
                _ => {}
            }
        }
        if self.pending_body == Some(BodyPrefix::Viewport) {
            self.pending_body = None;
            match key {
                "z" => self.body_frame(BodyFraming::Centre),
                "t" => self.body_frame(BodyFraming::Top),
                "b" => self.body_frame(BodyFraming::Bottom),
                // `z` followed by anything else is a chord nobody
                // bound; swallowing the second key too would eat an
                // edit, so it falls through as itself.
                _ => self.edit_body_key(shell, key, ctrl, alt, text),
            }
            return true;
        }
        // `z` is evil's viewport prefix, in the modal modes only — in
        // INSERT, and in the mouse-first modes, it is the letter z.
        if key == "z"
            && self.body.mode() != EditorMode::Insert
            && matches!(
                self.mode,
                InputMode::Vim | InputMode::Doom | InputMode::Helix
            )
        {
            self.pending_body = Some(BodyPrefix::Viewport);
            return true;
        }
        false
    }

    fn edit_body_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.viewport_chord(shell, key, ctrl, alt, text) {
            return;
        }
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
                // The named keys every other text field answers to.
                "home" => self.body.line_home(),
                "end" => self.body.line_end_motion(),
                "delete" => self.body.delete_at(),
                "pageup" => self.body.page(false, 20),
                "pagedown" => self.body.page(true, 20),
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
                        // Inside a table TAB does the org thing —
                        // realign, then step to the next cell — and
                        // everywhere else it keeps its old job.
                        None if self.table_tab() => {}
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
                // Esc on a quiet Normal surface leaves the editor —
                // but only when there is nothing to lose. It used to
                // clear the buffer and go, so a paragraph typed and
                // Esc'd was gone with no prompt and no undo; the reflex
                // second Esc after a chord that "did nothing" was the
                // most reliable way to lose work in the whole app.
                if key == "escape"
                    && self.body.mode() == EditorMode::Normal
                    && self.body.pending_stroke().is_none()
                    && self.body.pending_count() == 0
                {
                    if self.body_dirty() {
                        "unsaved edit — C-Enter or :w saves · :q! discards"
                            .clone_into(&mut self.status);
                    } else {
                        self.remember_body_cursor();
                        self.edit_target = None;
                        self.body.clear();
                        self.surface = ModalSurface::Browse;
                    }
                } else if ctrl {
                    // `C-d`, `C-f`, `C-a` … are chords in their own
                    // right; dropping the modifier turned them into the
                    // plain letters and silently deleted a char.
                    self.body.modal_key(&format!("C-{key}"));
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

    /// The body editor's chord in progress (`2d3i`), for the shell's
    /// mode chip. Empty when nothing is outstanding.
    #[must_use]
    pub fn body_pending_chord(&self) -> String {
        self.body.pending_chord()
    }

    /// Park the body editor's cursor at a byte offset (the mouse seam,
    /// and what a test needs to address a position directly).
    pub fn body_set_cursor(&mut self, byte: usize) {
        self.body.set_cursor_byte(byte);
    }

    /// The buffer's text scale — Doom's `text-scale`, which is a
    /// property of what you are reading rather than of the chrome
    /// around it.
    #[must_use]
    pub fn zoom(&self) -> f32 {
        ZOOM_STEP.powi(i32::from(self.zoom_steps))
    }

    /// One step larger (`C-+` / `C-=`).
    pub const fn zoom_in(&mut self) {
        if self.zoom_steps < ZOOM_MAX_STEPS {
            self.zoom_steps += 1;
        }
    }

    /// One step smaller (`C--`).
    pub const fn zoom_out(&mut self) {
        if self.zoom_steps > ZOOM_MIN_STEPS {
            self.zoom_steps -= 1;
        }
    }

    /// Back to unscaled (`C-0`).
    pub const fn zoom_reset(&mut self) {
        self.zoom_steps = 0;
    }

    /// What the window should call itself right now.
    ///
    /// The title was set once, at window creation, and never moved: a
    /// closure window said the same thing whether it was showing an
    /// outline, editing a body, or holding an unsaved paragraph. It is
    /// the one piece of the app visible in a task switcher, so it says
    /// which buffer and whether that buffer is saved — the convention
    /// every other editor shares.
    #[must_use]
    pub fn window_title(&self, shell: &Shell, vault: &str) -> String {
        let dirty = if self.body_dirty() { "● " } else { "" };
        self.buffer_name(shell).map_or_else(
            || format!("{dirty}closure — {vault}"),
            |buffer| format!("{dirty}{buffer} — closure"),
        )
    }

    /// Insert `text` from *outside* the editor — the system clipboard —
    /// at the cursor, as one undoable edit.
    ///
    /// The window's paste types its characters as keystrokes, which is
    /// what keeps the slash menu, completion and table alignment in
    /// charge of it — and is exactly why it must not happen outside
    /// INSERT, where a pasted URL would be read as a dozen commands.
    /// This is the other path: text goes in as text, whatever mode the
    /// editor is in, and a VISUAL selection is replaced by it the way
    /// `p` replaces one.
    pub fn body_paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.completion = None;
        self.body.paste_external(text);
    }

    /// The body editor's open `/` search line, for the shell to paint.
    #[must_use]
    pub fn body_search_prompt(&self) -> Option<String> {
        self.body.search_prompt()
    }

    /// The pattern the last `/` or `?` searched for, so a shell can
    /// mark every hit rather than only moving the cursor to one.
    #[must_use]
    pub fn body_search_pattern(&self) -> Option<String> {
        self.body.search_pattern().map(ToOwned::to_owned)
    }

    /// Whether the body editor is in REPLACE (`R`) rather than INSERT —
    /// the same mode chip, a different word on it.
    #[must_use]
    pub const fn body_replacing(&self) -> bool {
        self.body.replacing()
    }

    /// The macro register the body editor is recording into, if any.
    #[must_use]
    pub const fn body_recording(&self) -> Option<char> {
        self.body.recording_register()
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
        self.write_body(shell);
        self.remember_body_cursor();
        self.edit_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.surface = ModalSurface::Browse;
    }

    /// Note where the cursor was left in the body being closed, so the
    /// next visit resumes there.
    fn remember_body_cursor(&mut self) {
        if let Some(id) = &self.edit_target {
            self.body_cursors
                .insert(id.clone(), self.body.cursor_byte());
        }
    }

    /// Write the buffer into the vault, leaving the editor open and the
    /// buffer as it is.
    ///
    /// What `:w` needs and what committing is built out of: the write
    /// and the leaving are two different decisions, and a plain `:w`
    /// only ever meant the first one.
    fn write_body(&mut self, shell: &mut Shell) {
        let Some(id) = self.edit_target.clone() else {
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        // A body line starting with `*` *is* a headline once it is
        // back in the file: written verbatim it would split the
        // outline and reparent every following sibling.
        let mut body = closure_org::escape_body(self.body.text());
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        match shell.set_body(&bid, &body) {
            Ok(()) => {
                "body saved".clone_into(&mut self.status);
                // Saved *is* the new baseline: `body_dirty` compares
                // against what the vault holds, and after a write that
                // is what is in the buffer.
                self.body_baseline = self.body.text().to_owned();
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    /// Whether the body editor holds something the vault does not.
    ///
    /// A comparison against what was loaded, not a "was touched" bit:
    /// a buffer the user has put back the way they found it — by
    /// undoing, or by retyping the same word — has nothing to save,
    /// and warning about it would train them to ignore the warning.
    #[must_use]
    pub fn body_dirty(&self) -> bool {
        self.edit_target.is_some() && self.body.text() != self.body_baseline
    }

    /// Write out a body edit still in progress, if there is one.
    /// `true` when something was saved.
    ///
    /// What a window closing under an unfinished edit calls: the
    /// gesture that closed the window is recoverable, the paragraph
    /// that was in the buffer is not, so the text wins.
    pub fn save_pending_edit(&mut self, shell: &mut Shell) -> bool {
        if !self.body_dirty() {
            return false;
        }
        self.commit_edit_body(shell);
        true
    }

    /// Run one `:` line, for a shell (or a test) that has the text
    /// already rather than a keystroke at a time.
    pub fn run_ex_line(&mut self, shell: &mut Shell, line: &str) {
        self.ex_return = Some(self.surface);
        self.run_ex(shell, line);
    }

    /// Refuse to quit over an unsaved body, and say why.
    ///
    /// `true` when the caller should stop. The bare flag this replaced
    /// meant `:q` in the middle of an edit threw the buffer away
    /// without a word.
    fn refuse_quit_when_dirty(&mut self) -> bool {
        if !self.body_dirty() {
            return false;
        }
        "unsaved body — :w saves, :wq saves and quits, :q! discards it"
            .clone_into(&mut self.status);
        true
    }

    /// Replace a byte range of the body buffer with `text`, leaving the
    /// cursor after it.
    ///
    /// What an input method hands over: a compose sequence or a CJK IME
    /// builds a character over several keystrokes and delivers it as a
    /// replacement over a range, not as a keypress. Out-of-range or
    /// non-boundary offsets are clamped rather than panicking (I5), and
    /// the edit is one undo step like any other.
    pub fn body_replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        self.completion = None;
        self.body.replace_range(range, text);
    }

    /// Park the body-editor viewport so `line` is its first visible
    /// one, clamped to the buffer — what a scrollbar drag needs, and
    /// the absolute half of [`Self::body_scroll_by`].
    pub fn body_scroll_to(&mut self, line: usize, viewport: usize) {
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let (cl, _) = self.body.cursor_line_col();
        self.body_scroll = Some((line.min(max), cl));
    }

    fn on_search_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                // Never mind: the outline goes back to the row it was
                // on, not to whatever index the results left behind.
                self.query.clear();
                self.selected = self.search_return.take().unwrap_or(0);
                self.go_home();
            }
            "enter" => {
                // The row list is *filtered* while the overlay is open,
                // so the cursor is an index into the results; clearing
                // the query unfilters it and that index then points at
                // a different row entirely. The id is the only thing
                // both lists agree on.
                let hit = self.selected_row_id(shell);
                self.query.clear();
                self.search_return = None;
                if let Some(id) = hit {
                    self.select_by_id(shell, &id);
                }
                self.go_home();
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
            // The arrows moved the result cursor and the chords every
            // modal user reaches for did not, which is the one thing a
            // search overlay must not get wrong.
            "j" | "n" if ctrl => {
                let last = self.rows_shared(shell).len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
            "k" | "p" if ctrl => self.selected = self.selected.saturating_sub(1),
            "w" if ctrl => {
                let kept = self.query.trim_end();
                let cut = kept.trim_end_matches(|c: char| !c.is_whitespace());
                self.query.truncate(cut.len());
                self.selected = 0;
            }
            "u" if ctrl => {
                self.query.clear();
                self.selected = 0;
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl && !alt) {
                    self.query.push(c);
                    self.selected = 0;
                }
            }
        }
    }

    fn on_capture_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                self.go_home();
                self.capture_buf.clear();
            }
            "enter" => {
                if !self.capture_buf.is_empty() {
                    let title = self.capture_buf.take();
                    self.commit_capture(shell, &title);
                }
                self.go_home();
                self.capture_buf.clear();
            }
            // Everything else is the field's: the readline chords, the
            // arrows, and the characters themselves.
            _ => {
                self.capture_buf.key(key, ctrl, alt, text);
            }
        }
    }

    /// File `title` where the outline is pointing, and put the cursor
    /// on what was just made.
    ///
    /// Two halves of the same complaint: a capture used to land at the
    /// top of `inbox.org` whatever you were looking at, and the
    /// selection stayed where it was — so the next thing you did
    /// happened to the previous headline. Under the selection when
    /// there is one, top level when Escape has said there is not.
    fn commit_capture(&mut self, shell: &mut Shell, title: &str) {
        let parent = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let captured = match parent {
            Some(parent) => {
                let id = closure_core::BlockId::from_existing(&parent);
                shell.capture_under(&id, title)
            }
            None => shell.capture(title),
        };
        match captured {
            Ok(id) => {
                self.status = format!("captured: {title}");
                // The row list is rebuilt from the bumped revision, so
                // the new id is findable the moment we ask.
                self.select_by_id(shell, id.as_str());
                self.selection_active = true;
            }
            Err(e) => self.status = format!("capture failed: {e}"),
        }
    }

    /// The block id of the row under the cursor, if there is one.
    fn selected_row_id(&self, shell: &Shell) -> Option<String> {
        self.rows_shared(shell)
            .get(self.selected)
            .map(|r| r.id.clone())
            .filter(|id| !id.is_empty())
    }

    /// Where an overlay returns to when it closes.
    ///
    /// In the clickable view that is the outline; in the editor view it
    /// is the file buffer, which is the whole point of that view. They
    /// all returned to the outline, so opening the palette or a capture
    /// from a full-window buffer dropped you back into the row list —
    /// a different shape of the app than the one you were using.
    const fn home_surface(&self) -> ModalSurface {
        match self.view {
            ViewMode::Editor => ModalSurface::EditFile,
            ViewMode::Clickable => ModalSurface::Browse,
        }
    }

    /// Close the current overlay, returning to [`Self::home_surface`].
    const fn go_home(&mut self) {
        self.surface = self.home_surface();
    }

    /// Whether a row is selected, as opposed to the cursor merely
    /// resting on one. Escape clears it; a motion or a capture makes
    /// it true again.
    #[must_use]
    pub const fn selection_active(&self) -> bool {
        self.selection_active
    }

    /// Drop the selection without moving the cursor.
    pub const fn clear_selection(&mut self) {
        self.selection_active = false;
    }

    fn on_browse_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        // Escape in the outline drops the selection (and any half-typed
        // chord): the way to tell a capture "file this loose, not under
        // whatever the cursor happens to be resting on".
        if key == "escape" {
            self.pending.clear();
            self.clear_selection();
            return;
        }
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
        // Moving the cursor *is* selecting: Escape drops the selection
        // so a capture goes to the top level, and the next motion is
        // how you say you are looking at something again. Opening a
        // surface is not a motion — `Esc` then `c` must still capture
        // loose.
        if matches!(
            cmd,
            "next-file"
                | "prev-file"
                | "first-file"
                | "last-file"
                | "next-sibling"
                | "prev-sibling"
                | "parent"
                | "child"
        ) {
            self.selection_active = true;
        }
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
            "quit" => {
                if !self.refuse_quit_when_dirty() {
                    self.quit = true;
                }
            }
            "capture-start" => {
                self.surface = ModalSurface::Capture;
                self.capture_buf.clear();
            }
            "search-start" | "search-headline-start" => {
                self.surface = ModalSurface::Search;
                self.query.clear();
                // Remembered so Esc is a real "never mind".
                self.search_return = Some(self.selected);
                self.selected = 0;
            }
            // Enter *opens* the row. It used to report the row in the
            // status line, which is not what Enter means in any other
            // list in this app or any other: the selection is already
            // where the cursor is, so naming it says nothing new.
            "open-file" => {
                if self.rows_shared(shell).get(self.selected).is_some() {
                    self.selection_active = true;
                    self.run_command(shell, "edit-body");
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
                    // Say which way it went, in the same words
                    // [`OutlineApp::toggle_fold`] uses. This said
                    // nothing at all, which left the shells' toast
                    // rules for `folded:`/`unfolded:` matching a status
                    // no modal shell ever produced.
                    self.status = match toggle_visibility(shell, &bid) {
                        Some(true) => format!("folded: {}", row.title),
                        Some(false) => format!("unfolded: {}", row.title),
                        None => format!("fold failed: {}", row.title),
                    };
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
            // A refused level change (promoting a level-1 headline: no
            // level 0 exists) used to be dropped on the floor, so the
            // key did nothing and said nothing — which is exactly what
            // "the UI doesn't refresh" feels like from the outside.
            "promote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    self.status = match shell.promote(&bid) {
                        Ok(()) => format!("promoted: {}", row.title),
                        Err(_) if row.level <= 1 => {
                            "already at the top level — nothing to promote into".to_owned()
                        }
                        Err(e) => format!("promote failed: {e}"),
                    };
                }
            }
            "demote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    self.status = match shell.demote(&bid) {
                        Ok(()) => format!("demoted: {}", row.title),
                        Err(e) => format!("demote failed: {e}"),
                    };
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
                    let resume = self.body_cursors.get(&row.id).copied();
                    self.edit_target = Some(row.id);
                    let body = self.detail(shell).map(|d| d.body).unwrap_or_default();
                    self.body_baseline.clone_from(&body);
                    let len = body.len();
                    self.load_body(body);
                    // Opening a note you were just in used to start at
                    // byte zero, so any edit deeper in it meant
                    // navigating back down every time. A body can shrink
                    // between visits, so the remembered offset is
                    // clamped rather than trusted (I5).
                    if let Some(at) = resume {
                        self.body.set_cursor_byte(at.min(len));
                    }
                    self.surface = ModalSurface::EditBody;
                    self.status = if self.modal_editing() {
                        "edit body — NORMAL, i to insert, C-Enter save, Esc cancel".to_owned()
                    } else {
                        "edit body — C-Enter save, Esc cancel".to_owned()
                    };
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
                // How you type and what you are looking at are the same
                // decision: a mode with a NORMAL wants the file, a mode
                // without one wants the rows. Toggling the view back is
                // one chord away for anyone who disagrees.
                self.view = ViewMode::for_input(self.mode);
                match (self.view, self.surface) {
                    (ViewMode::Clickable, ModalSurface::EditFile) => self.close_file_buffer(),
                    (ViewMode::Editor, ModalSurface::Browse) => self.open_file_buffer(shell),
                    _ => {}
                }
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
            "ex-command" => self.begin_ex(),
            "llm" => {
                self.chat_buf.clear();
                self.surface = ModalSurface::Llm;
                "assistant — type a question, Enter sends, Esc back".clone_into(&mut self.status);
            }
            "graph" | "journal" | "cron" => {
                self.selected = 0;
                self.surface = match cmd {
                    "graph" => ModalSurface::Graph,
                    "journal" => ModalSurface::Journal,
                    _ => ModalSurface::Cron,
                };
                self.status = format!("{cmd} — Esc back");
            }
            "sync" => {
                self.sync_buf.clear();
                self.sync_mut();
                self.surface = ModalSurface::Sync;
                "sync — hand over your ticket, paste theirs, Esc back".clone_into(&mut self.status);
            }
            "edit-special" => self.begin_edit_special(shell),
            "eval-block" => self.eval_selected_block(shell),
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
            // The way home. Esc has always walked back out of a pane,
            // but Esc is a keyboard-only door: the rail's home button
            // needs a command of its own, and a `g h` for the users who
            // would rather not reach for Esc.
            "browse" => {
                self.slash = None;
                self.surface = ModalSurface::Browse;
                "outline".clone_into(&mut self.status);
            }
            // `SPC f s` / `:w`: write whatever buffer is open. Which
            // one that is decides what "write" means — a body commits
            // through the kernel command, a file writes its whole
            // source — and outside a buffer there is nothing to write,
            // because every other edit is already on disk.
            "save-buffer" => match self.surface {
                ModalSurface::EditFile => self.commit_file_buffer(shell),
                ModalSurface::EditBody => self.commit_edit_body(shell),
                ModalSurface::EditBlock => self.commit_edit_special(shell),
                _ => "no buffer open — every edit is already written".clone_into(&mut self.status),
            },
            // The switch between the two shapes of the shell: rows you
            // click, or the file itself in one buffer.
            "toggle-view" => {
                // Keyed off the surface rather than the stored view: a
                // modal mode *starts* in the editor view without a
                // buffer open yet (nothing has a shell to open one
                // with until the shell hands us one), and a toggle that
                // trusted the flag would close a buffer that was never
                // opened.
                if self.surface == ModalSurface::EditFile {
                    self.view = ViewMode::Clickable;
                    self.close_file_buffer();
                    "outline view".clone_into(&mut self.status);
                } else {
                    self.view = ViewMode::Editor;
                    self.open_file_buffer(shell);
                }
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

    /// The chord bound to `command` in the *active* mode, if any.
    ///
    /// The one lookup a shell calls while painting any affordance, so
    /// that every button and menu entry can carry its keybinding — and
    /// so that they all change together when the mode does. Sourced
    /// from [`closure_input::chord_for_command`] (I4).
    #[must_use]
    pub fn chord_for(&self, command: &str) -> Option<&'static str> {
        closure_input::chord_for_command(self.mode, command)
    }

    /// Output of the last source block run, while the Blocks surface
    /// still shows the block that produced it.
    #[must_use]
    pub fn block_output(&self) -> Option<&str> {
        self.block_out.as_deref()
    }

    /// Run the source block under the Blocks cursor through the kernel
    /// (org-babel), keeping its output for the pane to paint.
    ///
    /// The execution path — and in particular the `eval_trust`
    /// allowlist, the reason opening a file cannot run its code — is
    /// the kernel's [`closure_store::Vault::eval_block`]. A refusal is
    /// reported, never worked around.
    fn eval_selected_block(&mut self, shell: &mut Shell) {
        self.block_out = None;
        let rows = self.block_rows(shell);
        let Some((path, _, _)) = rows.get(self.selected).cloned() else {
            "no source blocks in this vault".clone_into(&mut self.status);
            return;
        };
        // `block_rows` is flat across files; `eval_block` counts within
        // one, so rebase the cursor onto the block's own file.
        let index = rows[..self.selected]
            .iter()
            .filter(|(p, _, _)| *p == path)
            .count();
        self.surface = ModalSurface::Blocks;
        match shell.vault.eval_block(std::path::Path::new(&path), index) {
            Ok(out) => {
                self.status = format!("ran block #{index} of {path}");
                self.block_out = Some(out);
            }
            Err(e) => self.status = format!("{e}"),
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
                // The comma escape is an on-disk spelling: showing it
                // in a hit means the line the user reads is not the
                // line they typed.
                let body = closure_org::unescape_body(h.body_text());
                for line in body
                    .lines()
                    .filter(|l| l.to_lowercase().contains(needle.as_str()))
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
