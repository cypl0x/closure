//! gpui shell for closure (Zed's native GPU UI framework) — the
//! reference GUI (Decision 2026-07-04).
//!
//! Native desktop window built on gpui, behind the opt-in `gpui`
//! cargo feature so the default workspace stays hermetic (I10). The
//! editor core is the dep-free, unit-tested [`ModalApp`] command
//! surface from closure-shell-core: Browse keys are commands resolved
//! against the active mode's keymap (vim/doom/helix/emacs/notion), a
//! search overlay owns type-to-filter, and every mouse affordance
//! (row select, fold arrow, which-key chips, palette rows, detail
//! fields) dispatches the SAME commands the chords do (I8). The window
//! is a thin translation of key/mouse events plus painting with the
//! shared [`Theme`] tokens (G2).

#![forbid(unsafe_code)]
#![recursion_limit = "512"]

use std::path::Path;

use closure_shell_core::Theme;
#[cfg(feature = "gpui")]
use closure_store::Vault;

// The state cores are shell-agnostic and live in closure-shell-core;
// the gpui crate re-exports them (GpuiApp/GpuiMode aliases preserve
// the historical names) and adds the gpui window.
pub use closure_shell_core::{
    App as GpuiApp, Detail, HeadlessAdapter, ModalApp, ModalSurface, Mode as GpuiMode, Row,
    Selection, Shell, ShellAdapter,
};

/// Marker for the capability matrix.
pub const GPUI_SHELL: &str = "gpui";

#[cfg(feature = "gpui-test")]
mod testing;
#[cfg(feature = "gpui-test")]
pub use testing::{ALL_SURFACES, opening_route, test_window, visual_window};

/// `text` cut to at most `max` characters, the last of them an
/// ellipsis standing in for what was dropped.
///
/// Counted in characters, not bytes: a German headline is the normal
/// case here, and slicing one at a byte offset panics.
#[must_use]
pub fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(1);
    text.chars()
        .take(keep)
        .chain(std::iter::once('…'))
        .collect()
}

/// Pack a theme [`closure_shell_core::Color`] into the `0xRRGGBB`
/// integer gpui's `rgb()` expects.
#[must_use]
pub fn color_u32(c: closure_shell_core::Color) -> u32 {
    let (r, g, b) = c.rgb();
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Blend two packed `0xRRGGBB` colours channel-wise; `t` is the weight
/// of `b` in 0..=255. Backs hover/inactive shades derived from the
/// theme palette instead of hardcoded hexes.
#[must_use]
pub fn mix_u32(a: u32, b: u32, t: u32) -> u32 {
    let ch = |shift: u32| {
        let ca = (a >> shift) & 0xff;
        let cb = (b >> shift) & 0xff;
        ((ca * (255 - t) + cb * t) / 255) & 0xff
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Resolve the shared [`Theme`] from the vault's `config.org`.
///
/// Reads `theme = light|high-contrast|dark|doom-vibrant`. The reference
/// shell's default — absent config or the config default `default` — is
/// `doom-vibrant` (the user's colorscheme); an explicit name wins.
/// Never an error (I9 validates at load; the window must still open on
/// a themeless vault).
#[must_use]
pub fn resolve_theme(vault_path: &Path) -> Theme {
    let name = closure_config::Config::from_path(&vault_path.join("config.org"))
        .map_or_else(|_| "default".to_owned(), |cfg| cfg.theme);
    match name.to_ascii_lowercase().as_str() {
        "light" => Theme::light(),
        "high-contrast" | "hc" => Theme::high_contrast(),
        "dark" => Theme::dark(),
        _ => Theme::doom_vibrant(),
    }
}

/// Resolve the startup input mode from the vault's `config.org`
/// (`input_mode = doom|vim|helix|emacs|notion`); defaults to Doom (the
/// config default) when absent.
#[must_use]
pub fn resolve_input_mode(vault_path: &Path) -> closure_config::InputMode {
    closure_config::Config::from_path(&vault_path.join("config.org"))
        .map_or(closure_config::InputMode::Doom, |cfg| cfg.input_mode)
}

/// Which shape the window opens in, from the vault's `config.org`.
///
/// Defaults to the clickable outline — that is where the rail and every
/// affordance are, and a window that opened straight into a raw file
/// buffer would hide the app from anyone who had not asked for it.
/// `view = editor` is how you ask; `g v` is how you change your mind.
#[must_use]
pub fn resolve_view(vault_path: &Path) -> closure_shell_core::ViewMode {
    let name = closure_config::Config::from_path(&vault_path.join("config.org"))
        .map_or_else(|_| "clickable".to_owned(), |cfg| cfg.view);
    if name == "editor" {
        closure_shell_core::ViewMode::Editor
    } else {
        closure_shell_core::ViewMode::Clickable
    }
}

/// What the GPU window can do on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    /// A display and a Vulkan driver are both there; open the window.
    Ready,
    /// A display, but no driver the machine owns — render on the
    /// software rasteriser at this ICD manifest instead. Slow, and it
    /// opens.
    Software(std::path::PathBuf),
    /// Nothing to open a window on, and why, in words.
    Refused(String),
}

/// Whether the GPU window can open here, and on what.
///
/// gpui picks its backend from `WAYLAND_DISPLAY`/`DISPLAY` and then
/// `unwrap`s the GPU context, so a machine without a Vulkan driver gets
/// a panic and a backtrace through `blade_graphics` —
/// `NoSupportedDeviceFound`, which names nothing you could install.
/// This runs first and says what is actually missing.
///
/// The Vulkan *loader* is a library (the dev shell has it); a Vulkan
/// *driver* is an ICD manifest, which on NixOS comes from
/// `hardware.graphics.enable` and lands in `/run/opengl-driver`, and
/// elsewhere from the distro's mesa package. A headless server has
/// neither — and that is what `software_icd` is for: the dev shell
/// ships lavapipe and points `CLOSURE_SOFTWARE_ICD` at its manifest, so
/// a box with no GPU still opens a window without the user installing
/// anything. It is a fallback only: a real driver always wins, because
/// software rendering costs an order of magnitude in frame time.
#[must_use]
pub fn gpui_preflight(
    wayland: Option<&str>,
    x11: Option<&str>,
    icd_dirs: &[std::path::PathBuf],
    icd_override: Option<&str>,
    software_icd: Option<&str>,
) -> Preflight {
    let has_display = wayland.is_some_and(|d| !d.is_empty()) || x11.is_some_and(|d| !d.is_empty());
    if !has_display {
        return Preflight::Refused(
            "no display: neither WAYLAND_DISPLAY nor DISPLAY is set, so there is no \
             compositor to open a window on. Over SSH, forward one (`ssh -X`) or run \
             closure on the machine with the screen; the TUI (`closure tui`) needs \
             neither."
                .to_owned(),
        );
    }
    // An explicit override names the driver outright, so the search
    // directories stop mattering.
    if icd_override.is_some_and(|v| !v.is_empty()) {
        return Preflight::Ready;
    }
    let has_driver = icd_dirs.iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|mut entries| {
            entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|ext| ext == "json")))
        })
    });
    if has_driver {
        return Preflight::Ready;
    }
    // The env var can outlive the store path that set it, and a
    // manifest that is not on disk fails inside the loader with the
    // very panic this function exists to prevent.
    if let Some(icd) = software_icd.map(std::path::PathBuf::from)
        && icd.is_file()
    {
        return Preflight::Software(icd);
    }
    Preflight::Refused(
        "no Vulkan driver: a display is available but no ICD manifest was found, so \
         gpui's GPU context has nothing to run on (`NoSupportedDeviceFound`).\n  \
         Normally there is nothing to do: `nix develop` ships the lavapipe software \
         rasteriser and points CLOSURE_SOFTWARE_ICD at it, so a machine with no GPU \
         still opens a window. Seeing this means that variable is unset or stale — \
         run through the dev shell (`nix develop -c just run-gpui-release VAULT`).\n  \
         On NixOS with a GPU: set `hardware.graphics.enable = true;` \
         (`hardware.opengl.enable` before 24.11) and rebuild — that populates \
         /run/opengl-driver.\n  \
         Any lavapipe manifest also works, named outright: \
         VK_ICD_FILENAMES=/path/to/lvp_icd.x86_64.json\n  \
         Either way `closure tui` needs no GPU."
            .to_owned(),
    )
}

/// The standard places a Vulkan ICD manifest is looked for.
///
/// The loader's own search path, minus the per-user and `XDG_DATA_DIRS`
/// entries: NixOS' `/run/opengl-driver` first, then where a distro
/// package, a local build and an administrator put one. Looking only
/// where NixOS puts it reports "no driver" on a machine that has one.
#[must_use]
pub fn vulkan_icd_dirs() -> Vec<std::path::PathBuf> {
    [
        "/run/opengl-driver/share/vulkan/icd.d",
        "/usr/share/vulkan/icd.d",
        "/usr/local/share/vulkan/icd.d",
        "/etc/vulkan/icd.d",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect()
}

/// Where pairing listens, and which address its ticket hands out.
///
/// `sync_bind` is the socket to open, `sync_advertise` the address a
/// peer should dial. Absent config binds
/// [`closure_shell_core::DEFAULT_SYNC_BIND`] — the closure port on
/// every interface — and lets the advertised address be detected, which
/// is right for a LAN and wrong exactly when the machine has a second
/// route (a mesh VPN); that is what the key is for. Never an error: I9
/// validates at load, and a vault with a bad address must still open.
#[must_use]
pub fn resolve_sync_addrs(vault_path: &Path) -> (std::net::SocketAddr, Option<std::net::IpAddr>) {
    closure_config::Config::from_path(&vault_path.join("config.org")).map_or(
        (closure_shell_core::DEFAULT_SYNC_BIND, None),
        |cfg| {
            (
                cfg.sync_bind
                    .unwrap_or(closure_shell_core::DEFAULT_SYNC_BIND),
                cfg.sync_advertise,
            )
        },
    )
}

/// Semantic classification of a body-editor span (per line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySpan {
    /// Ordinary prose.
    Plain,
    /// `#+…` keyword/meta lines (block delimiters, `#+TITLE:` …).
    Meta,
    /// `:PROPERTIES:` / `:KEY: value` / `:END:` drawer lines.
    Drawer,
    /// Language keyword inside a src block.
    Keyword,
    /// String/number literal inside a src block.
    Literal,
    /// Comment inside a src block.
    Comment,
    /// An org link, `[[target]]` or `[[target][label]]`.
    Link,
    /// A table row or rule, `| a | b |` / `|---+---|`.
    Table,
    /// `*bold*`.
    Bold,
    /// `/italic/`.
    Italic,
    /// `=code=` — inline code, distinct from a src block's contents.
    InlineCode,
    /// `~verbatim~`.
    Verbatim,
    /// `+strikethrough+`.
    Strike,
    /// `_underline_`.
    Underline,
    /// The content of a `#+BEGIN_QUOTE` / `VERSE` / `CENTER` block:
    /// prose, but somebody else's.
    Quote,
    /// The content of a `#+BEGIN_EXAMPLE` / `EXPORT` / `COMMENT`
    /// block: verbatim, and not to be read as org syntax.
    Example,
    /// A headline's stars and title, carrying its nesting level — org's
    /// outline faces cycle by level, so the level is the colour.
    Headline(u8),
    /// An unfinished TODO keyword on a headline.
    Todo,
    /// A finished one (`DONE`).
    Done,
    /// A `[#A]` priority cookie.
    Priority,
    /// The `:tag:tag:` run at the end of a headline.
    Tags,
}

/// How a span is drawn beyond its colour.
///
/// Emphasis is weight and slant before it is hue: `*bold*` rendered as
/// a differently-coloured word is not bold, it is a colour. Kept as
/// plain data so it can be pinned without a GPU — the window turns it
/// into a `gpui::HighlightStyle`.
// Four orthogonal typographic rules, and they combine: bold italic
// struck-through underlined text is one span, not four states. An
// enum would have to enumerate the combinations.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Decoration {
    /// Draw the glyphs bold.
    pub bold: bool,
    /// Draw them italic.
    pub italic: bool,
    /// Strike through them.
    pub strike: bool,
    /// Underline them.
    pub underline: bool,
}

/// The decoration a span kind carries.
#[must_use]
pub const fn span_decoration(kind: BodySpan) -> Decoration {
    let d = Decoration {
        bold: false,
        italic: false,
        strike: false,
        underline: false,
    };
    match kind {
        // A heading is heavier than its body, at every level — that is
        // what makes an outline skimmable rather than merely coloured.
        BodySpan::Bold | BodySpan::Headline(_) | BodySpan::Todo | BodySpan::Done => {
            Decoration { bold: true, ..d }
        }
        BodySpan::Italic | BodySpan::Quote => Decoration { italic: true, ..d },
        BodySpan::Strike => Decoration { strike: true, ..d },
        BodySpan::Underline | BodySpan::Link => Decoration {
            underline: true,
            ..d
        },
        _ => d,
    }
}

/// The span kind for the content of the block named `name`.
///
/// A quote, a verse and a centred passage are prose somebody else
/// wrote; an example, an export and a comment are verbatim text that
/// must not be read as org syntax. `src` has its own path — the
/// language highlighter — and never reaches here.
const fn block_content_span(name: &str) -> BodySpan {
    if name.eq_ignore_ascii_case("quote")
        || name.eq_ignore_ascii_case("verse")
        || name.eq_ignore_ascii_case("center")
    {
        BodySpan::Quote
    } else {
        BodySpan::Example
    }
}

/// One org link found on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLink {
    /// Byte range of the whole `[[…]]` construct within the line.
    pub range: std::ops::Range<usize>,
    /// What it points at (`id:01HQ…`, a URL, a headline title).
    pub target: String,
    /// What it reads as — the target itself when there is no label.
    pub label: String,
}

/// Every org link on one line, in order.
///
/// The target is carried out separately rather than discarded, because
/// a rendered label with no way back to the id it points at would be
/// prettier and useless — this is what lets a click follow the link.
/// Unclosed brackets are not links: half-typed syntax must not swallow
/// the rest of the line.
#[must_use]
pub fn line_links(line: &str) -> Vec<LineLink> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if &bytes[i..i + 2] != b"[[" {
            i += 1;
            continue;
        }
        let Some(close) = line[i..].find("]]").map(|off| i + off) else {
            break;
        };
        let inner = &line[i + 2..close];
        // `[[target][label]]` splits on the inner `][`; without it the
        // target labels itself.
        let (target, label) = inner.split_once("][").unwrap_or((inner, inner));
        out.push(LineLink {
            range: i..close + 2,
            target: target.to_owned(),
            label: label.to_owned(),
        });
        i = close + 2;
    }
    out
}

/// What following an org link should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// `id:<ULID>` or a bare ULID — select that headline.
    Block(String),
    /// `file:<path>` — a file in the vault.
    File(String),
    /// `file:<path>::<search>` — a file, and where in it.
    FileAt(String, String),
    /// A URL: `https:`, `mailto:`, anything with a scheme closure does
    /// not own. Opening one is the user's call, not closure's.
    External(String),
    /// Org's fuzzy link, written with no scheme — a bare id or a
    /// headline title. Which one it is, is the vault's to say: a
    /// length-and-alphabet guess at "is this a ULID" would be wrong
    /// for every id format but the current one.
    Fuzzy(String),
    /// Nothing to follow.
    None,
}

/// Classify a link target.
///
/// The window understood `id:` and a bare ULID and reported everything
/// else as "not a headline in this vault" — true, and useless: a
/// `file:` link into the same vault had no reason to be refused, and a
/// URL had no way out of the window at all.
#[must_use]
pub fn link_action(target: &str) -> LinkAction {
    let target = target.trim();
    if target.is_empty() {
        return LinkAction::None;
    }
    if let Some(path) = target.strip_prefix("file:") {
        return match path.split_once("::") {
            Some((file, at)) => LinkAction::FileAt(file.to_owned(), at.to_owned()),
            None => LinkAction::File(path.to_owned()),
        };
    }
    if let Some(id) = target.strip_prefix("id:") {
        return LinkAction::Block(id.to_owned());
    }
    // A named scheme, not any colon: org's fuzzy links contain them
    // freely, so `Meeting: Monday` is a heading and treating it as a
    // URL because it parses like one is how a link stops working.
    // This is org's own approach — `org-link-parameters` is a list.
    if EXTERNAL_SCHEMES.iter().any(|s| {
        target.len() > s.len() && target.as_bytes()[s.len()] == b':' && starts_ci(target, s)
    }) {
        return LinkAction::External(target.to_owned());
    }
    LinkAction::Fuzzy(target.to_owned())
}

/// The link schemes closure hands back to the desktop rather than
/// resolving itself — org's built-in set.
const EXTERNAL_SCHEMES: &[&str] = &[
    "http", "https", "mailto", "ftp", "ftps", "news", "irc", "ssh", "gopher", "doi", "magnet",
    "tel", "sms",
];

/// Case-insensitive `starts_with` for the ASCII scheme names above.
fn starts_ci(haystack: &str, prefix: &str) -> bool {
    haystack
        .get(..prefix.len())
        .is_some_and(|h| h.eq_ignore_ascii_case(prefix))
}

/// Whether `line` is an org table row or rule.
fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// Split a prose line into `Plain`, `Link` and emphasis spans,
/// verbatim.
///
/// Links are found first and win: `[[https://x/a_b_c]]` is one link,
/// not a link with an underline run chewed out of its middle. What is
/// left between them is scanned for org's inline markup
/// ([`closure_org::markup_spans`]) — which the shell rendered as flat
/// prose until now, in a tool whose text is the product.
fn prose_spans(line: &str) -> Vec<(BodySpan, String)> {
    let links = line_links(line);
    let mut out = Vec::with_capacity(links.len() * 2 + 1);
    let mut at = 0usize;
    for link in links {
        if at < link.range.start {
            push_markup_spans(&mut out, &line[at..link.range.start]);
        }
        out.push((BodySpan::Link, line[link.range.clone()].to_owned()));
        at = link.range.end;
    }
    if at < line.len() {
        push_markup_spans(&mut out, &line[at..]);
    }
    if out.is_empty() {
        out.push((BodySpan::Plain, line.to_owned()));
    }
    out
}

/// Append `text` to `out`, split into `Plain` and emphasis runs.
fn push_markup_spans(out: &mut Vec<(BodySpan, String)>, text: &str) {
    let mut at = 0usize;
    for (range, kind) in closure_org::markup_spans(text) {
        if at < range.start {
            out.push((BodySpan::Plain, text[at..range.start].to_owned()));
        }
        out.push((markup_span(kind), text[range.clone()].to_owned()));
        at = range.end;
    }
    if at < text.len() {
        out.push((BodySpan::Plain, text[at..].to_owned()));
    }
}

/// The span kind for one of org's inline markup kinds.
const fn markup_span(kind: closure_org::MarkupKind) -> BodySpan {
    use closure_org::MarkupKind as M;
    match kind {
        M::Bold => BodySpan::Bold,
        M::Italic => BodySpan::Italic,
        M::Code => BodySpan::InlineCode,
        M::Verbatim => BodySpan::Verbatim,
        M::Strikethrough => BodySpan::Strike,
        M::Underline => BodySpan::Underline,
    }
}

/// Syntax-highlight an org body for the editor pane: one entry per
/// line, each a list of `(kind, text)` spans that concatenate back to
/// the line verbatim.
///
/// `#+…` lines are Meta, drawer lines Drawer, and the content of
/// `#+BEGIN_SRC lang` blocks is classified through the shared
/// [`closure_tree_sitter::Highlighter`] contract — the dep-free
/// keyword tier by default, real tree-sitter grammars behind the
/// `tree-sitter` feature of that crate, no API change here.
#[must_use]
pub fn highlight_body(body: &str) -> Vec<Vec<(BodySpan, String)>> {
    use closure_org::BlockDelimiter as D;
    use closure_tree_sitter::{HighlightKind, Highlighter as _, KeywordHighlighter};
    /// What the reader is inside of, if anything.
    enum Open {
        /// A `#+BEGIN_SRC`, with the language's highlighter.
        Src(String, KeywordHighlighter),
        /// Any other block: its name, and the kind its content takes.
        Other(String, BodySpan),
    }
    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    for line in body.split('\n') {
        // A delimiter is syntax whatever it delimits, and only its own
        // `#+END_` closes the block it opened.
        match (closure_org::block_delimiter_of(line), &open) {
            (Some(D::End { name }), Some(Open::Src(open_name, _) | Open::Other(open_name, _)))
                if name.eq_ignore_ascii_case(open_name) =>
            {
                open = None;
                out.push(vec![(BodySpan::Meta, line.to_owned())]);
                continue;
            }
            (Some(D::Begin { name, args }), None) => {
                open = Some(if name.eq_ignore_ascii_case("src") {
                    Open::Src(
                        name.to_owned(),
                        KeywordHighlighter::for_language(args.unwrap_or_default().trim()),
                    )
                } else {
                    Open::Other(name.to_owned(), block_content_span(name))
                });
                out.push(vec![(BodySpan::Meta, line.to_owned())]);
                continue;
            }
            _ => {}
        }
        match &open {
            // Src content goes through the shared highlighter contract
            // — the dep-free keyword tier by default, real grammars
            // behind closure-tree-sitter's `tree-sitter` feature.
            Some(Open::Src(_, hl)) => {
                let spans = hl
                    .highlight(line)
                    .into_iter()
                    .map(|h| {
                        let kind = match h.kind {
                            HighlightKind::Keyword => BodySpan::Keyword,
                            HighlightKind::Literal => BodySpan::Literal,
                            HighlightKind::Comment => BodySpan::Comment,
                            _ => BodySpan::Plain,
                        };
                        (kind, line[h.start..h.end].to_owned())
                    })
                    .collect::<Vec<_>>();
                out.push(if spans.is_empty() {
                    vec![(BodySpan::Plain, line.to_owned())]
                } else {
                    spans
                });
            }
            // Everything else in a block is verbatim: `*x*` inside an
            // example block is two stars and an x.
            Some(Open::Other(_, kind)) => out.push(vec![(*kind, line.to_owned())]),
            None => out.push(free_line_spans(line)),
        }
    }
    out
}

/// Classify one body line that is not inside a block.
fn free_line_spans(line: &str) -> Vec<(BodySpan, String)> {
    if let Some(spans) = headline_spans(line) {
        return spans;
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("#+") {
        return vec![(BodySpan::Meta, line.to_owned())];
    }
    if trimmed.starts_with(':')
        && (trimmed.ends_with(':')
            || trimmed
                .split_once(' ')
                .is_some_and(|(k, _)| k.ends_with(':')))
    {
        return vec![(BodySpan::Drawer, line.to_owned())];
    }
    if is_table_line(line) {
        return vec![(BodySpan::Table, line.to_owned())];
    }
    prose_spans(line)
}

/// Classify a headline line, or `None` when the line is not one.
///
/// The editor view opens a whole org file, so most of what is on screen
/// is headlines — and every one of them used to render as prose. The
/// pieces org gives a face of its own are lifted out (the keyword, the
/// priority cookie, the tag run) and everything else takes the level's
/// colour, which is how org itself paints an outline.
///
/// What counts as a headline is the *kernel's* answer, not a second
/// opinion: the line is parsed with [`closure_org::parse`] and the
/// pieces come from the resulting [`closure_org::Headline`] (I7). That
/// is what keeps `*bold*` prose and `  * item` a list bullet — one
/// space and one column apart from an outline heading — and what makes
/// the set of TODO keywords the parser's rather than this file's.
fn headline_spans(line: &str) -> Option<Vec<(BodySpan, String)>> {
    // Cheap reject first: parsing every prose line of a large buffer to
    // learn it does not start with a star is work nobody asked for.
    if !line.starts_with('*') {
        return None;
    }
    let doc = closure_org::parse(line).ok()?;
    let headline = doc.roots().first()?;
    let level = headline.level();
    // `* ` with nothing after it is still a headline; the parser says
    // so, and typing one is how you begin.
    let mut out: Vec<(BodySpan, String)> = Vec::new();
    let mut at = 0usize;
    let push = |kind: BodySpan, text: &str, out: &mut Vec<(BodySpan, String)>| {
        if !text.is_empty() {
            out.push((kind, text.to_owned()));
        }
    };
    // Stars, plus the whitespace that separates them from the title.
    let stars = line.len() - line.trim_start_matches('*').len();
    let after_stars = stars + (line[stars..].len() - line[stars..].trim_start().len());
    push(BodySpan::Headline(level), &line[at..after_stars], &mut out);
    at = after_stars;

    if let Some(keyword) = headline.todo()
        && line[at..].starts_with(keyword)
    {
        let kind = if keyword == "DONE" {
            BodySpan::Done
        } else {
            BodySpan::Todo
        };
        push(kind, keyword, &mut out);
        at += keyword.len();
    }
    if let Some(letter) = headline.priority() {
        let cookie = format!("[#{letter}]");
        if let Some(start) = line[at..].find(&cookie) {
            push(BodySpan::Headline(level), &line[at..at + start], &mut out);
            push(BodySpan::Priority, &cookie, &mut out);
            at += start + cookie.len();
        }
    }
    // The tag run is anchored to the end of the line, which is the only
    // place org accepts one — searching forwards would find `:a:` in a
    // title and cut it out of the middle.
    let tags = headline.tags();
    let tail = if tags.is_empty() {
        line.len()
    } else {
        let run = format!(":{}:", tags.join(":"));
        line.trim_end()
            .strip_suffix(&run)
            .map_or(line.len(), str::len)
    };
    push(BodySpan::Headline(level), &line[at..tail], &mut out);
    push(BodySpan::Tags, &line[tail..], &mut out);
    // Neighbours of the same kind are one run: the pieces above are cut
    // where the *parser* has something to say, not where the painter
    // does, and two adjacent ranges in one colour are a redundant
    // highlight for gpui to lay out.
    out.dedup_by(|(kind, text), (prev_kind, prev_text)| {
        (*kind == *prev_kind)
            .then(|| prev_text.push_str(text))
            .is_some()
    });
    Some(out)
}

/// Flatten one line's highlight spans into `(byte range, kind)` pairs
/// over the reconstructed line text — the shape gpui's
/// `StyledText::with_highlights` consumes.
///
/// Ranges are contiguous, byte-indexed and end exactly at the line
/// length (gpui debug-asserts every bound is a char boundary). Empty
/// spans are dropped: a zero-width run styles nothing.
#[must_use]
pub fn span_ranges(spans: &[(BodySpan, String)]) -> Vec<(std::ops::Range<usize>, BodySpan)> {
    let mut out = Vec::with_capacity(spans.len());
    let mut at = 0usize;
    for (kind, text) in spans {
        let end = at + text.len();
        if at < end {
            out.push((at..end, *kind));
        }
        at = end;
    }
    out
}

/// What a marked run in the body editor *means*.
///
/// All three are a background range, and they must not look alike — a
/// cursor drawn in the selection tint disappears against a selected
/// row, which is exactly what it used to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// The VISUAL selection: tinted background, glyphs keep their
    /// syntax colour.
    Selection,
    /// The cursor: inverse video, so it is legible on any background.
    Cursor,
    /// A `/` search hit. The editor moved the cursor to one and marked
    /// none of them, so the pattern you were looking for was the one
    /// thing on screen you could not see.
    Search,
}

/// One painted run of a body line: the byte range it covers within the
/// line, the span kind that colours it, and what — if anything — is
/// drawn behind it.
pub type StyledRun = (std::ops::Range<usize>, BodySpan, Option<Emphasis>);

/// Merge a line's syntax spans with any number of background `marks`
/// into the single ordered run list gpui takes.
///
/// A line carries independent stylings: the per-span syntax colour, and
/// backgrounds for the VISUAL selection, the NORMAL-mode block caret
/// and every search hit on the line. `StyledText::with_highlights`
/// walks its input assuming ascending, non-overlapping, char-aligned
/// ranges, so the spans are split at every mark edge rather than
/// layered. Later marks win where two overlap, which is how the cursor
/// is drawn over a search hit it happens to sit on.
///
/// The runs are contiguous and cover the line exactly.
#[must_use]
pub fn styled_runs(
    spans: &[(BodySpan, String)],
    marks: &[(std::ops::Range<usize>, Emphasis)],
) -> Vec<StyledRun> {
    // A zero-width mark marks nothing, and must not cut a span either:
    // splitting `abc` at 2..2 would hand gpui two runs where one would
    // do, and make two identical calls compare unequal.
    let marks: Vec<&(std::ops::Range<usize>, Emphasis)> =
        marks.iter().filter(|(m, _)| m.start < m.end).collect();
    let mut out: Vec<StyledRun> = Vec::with_capacity(spans.len() + marks.len() * 2);
    for (range, kind) in span_ranges(spans) {
        // Cut this span at every mark edge that falls inside it, then
        // label each piece with the last mark covering it.
        let strictly_inside = |at: usize| at > range.start && at < range.end;
        let mut cuts = vec![range.start, range.end];
        for (m, _) in &marks {
            cuts.extend(
                [m.start, m.end]
                    .into_iter()
                    .filter(|at| strictly_inside(*at)),
            );
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let mark = marks
                .iter()
                .rfind(|(m, _)| m.start <= start && m.end >= end)
                .map(|(_, e)| *e);
            out.push((start..end, kind, mark));
        }
    }
    out
}

/// Every occurrence of `pattern` in `text`, as disjoint ascending byte
/// ranges.
///
/// What marks a search hit. Case-sensitive, because
/// [`closure_shell_core::BodyEditor`]'s own `/` is: a mark that lit up
/// a word the cursor would not jump to is worse than no mark. Disjoint
/// matters too — the ranges go to a renderer that assumes it, so `aa`
/// in `aaaa` is two runs and not three. An empty pattern matches
/// nothing: a search with nothing in it should not light up the buffer.
#[must_use]
pub fn line_matches(text: &str, pattern: &str) -> Vec<std::ops::Range<usize>> {
    if pattern.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(found) = text.get(at..).and_then(|rest| rest.find(pattern)) {
        let start = at + found;
        at = start + pattern.len();
        out.push(start..at);
    }
    out
}

/// The first visible row of a capped list, as a range over it.
///
/// The graph pane painted the first 20 hubs while the keyboard cursor
/// ran across all of them, so past the cap `j` moved a cursor that was
/// not drawn and nothing scrolled after it. The window follows the
/// cursor instead: the same rule the body editor's viewport uses.
#[must_use]
pub fn visible_window(cursor: usize, len: usize, cap: usize) -> std::ops::Range<usize> {
    if len == 0 || cap == 0 {
        return 0..0;
    }
    if len <= cap {
        return 0..len;
    }
    let cursor = cursor.min(len - 1);
    let start = if cursor < cap { 0 } else { cursor + 1 - cap };
    start..(start + cap).min(len)
}

/// First visible column of a body line, from where the cursor is.
///
/// Lines do not wrap in the editor — wrapping desyncs the one-number
/// gutter, the fixed row height and the arithmetic that turns pane
/// height into a line count — so a long line has to scroll sideways
/// instead. Same rule as the vertical viewport: the cursor's column is
/// the last visible one once it runs off the edge.
#[must_use]
pub const fn h_scroll_start(cursor_col: usize, cols: usize) -> usize {
    if cols == 0 || cursor_col < cols {
        return 0;
    }
    cursor_col + 1 - cols
}

/// A one-line prompt as it is laid out, and the byte range its caret
/// covers, from a `cursor` given as a byte offset.
///
/// Every one-line surface — the capture bar, rename/add/tags/property,
/// the palette's filter — used to paint `format!("{text}▏")`, which put
/// the caret after the last character no matter where the cursor was.
/// The core has always moved the cursor for Left/Right, the readline
/// chords and Alt+Backspace ([`closure_shell_core::ModalApp::
/// field_cursor`]); with the caret pinned to the end, all of those
/// looked unbound.
///
/// Splitting the string at the cursor instead fixed that and bought a
/// second bug: an inserted glyph takes width, so a caret moving to the
/// front shoved the whole line sideways ("weird shift in capture prompt
/// when ctlr+a is pressed"). A caret is not a character — it is a mark
/// over the cell it is on. This is [`cursor_cell`], which the body
/// editor's block cursor already uses, so the two cursors are one
/// implementation and cannot drift apart; the cell padding and the
/// multi-byte handling come with it.
#[must_use]
pub fn field_caret_cell(text: &str, cursor: usize) -> (String, std::ops::Range<usize>) {
    // `cursor_cell` counts in char columns, a `LineInput` cursor is a
    // byte offset, and an offset that lands inside a glyph belongs to
    // that glyph.
    let at = floor_boundary(text, cursor);
    cursor_cell(text, text[..at].chars().count())
}

/// The char boundary at or below `at`, so a byte offset that landed
/// mid-glyph names the glyph it is inside rather than panicking a
/// repaint.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Whether the cursor needs a caret element of its own on this line.
///
/// The block cursor is drawn by inverting the glyph underneath it, so
/// an empty line — or a cursor parked past the last glyph, which is
/// where end-of-line and vertical motion both leave it — has nothing
/// to invert and the cursor disappears. Those are exactly the cases
/// [`cursor_cell`] pads with a space.
#[must_use]
pub fn needs_trailing_caret(line: &str, on_cursor_line: bool, col: usize) -> bool {
    on_cursor_line && col >= line.chars().count()
}

/// The line as the cursor row is laid out, and the byte range the block
/// cursor covers in it.
///
/// A cursor past the last glyph — `$` on a short line, `j` onto a
/// shorter one, an empty line — has nothing to invert. It used to get a
/// hardcoded 8×18px rectangle appended after the text, which is the
/// right size at exactly one font size and one line height, and the
/// wrong one at every other. Padding the line with a space instead
/// makes the cursor a cell of *text*: the font decides how wide and how
/// tall a cell is, and the same inversion draws it as everywhere else.
#[must_use]
pub fn cursor_cell(line: &str, col: usize) -> (String, std::ops::Range<usize>) {
    let chars = line.chars().count();
    if col >= chars {
        let mut padded = line.to_owned();
        padded.push(' ');
        return (padded, line.len()..line.len() + 1);
    }
    let start = byte_for_col(line, col);
    let end = byte_for_col(line, col + 1);
    (line.to_owned(), start..end)
}

/// The cursor's mark on its own line, or `None` in INSERT.
///
/// INSERT draws a thin bar *between* two glyphs rather than a block over
/// one, so it is the one mode with no marked cell. Every other mode has
/// one — including VISUAL, which used to suppress the cursor entirely
/// because the selection was already a background range: you could not
/// see which end of the selection you were moving.
#[must_use]
pub fn cursor_mark(
    line: &str,
    col: usize,
    insert: bool,
) -> Option<(std::ops::Range<usize>, Emphasis)> {
    (!insert).then(|| (cursor_cell(line, col).1, Emphasis::Cursor))
}

/// Cut a run list at byte offset `at`, rebasing the tail to start at
/// zero. A run straddling the cut is split, keeping its kind and
/// highlight flag on both sides.
///
/// The INSERT caret is a thin bar *between* two glyphs rather than a
/// background block, so the cursor line is painted as two
/// `StyledText` halves with the bar between them — which needs the
/// line's runs cut at the caret. `at` past the end of the runs is
/// clamped: the whole line becomes the head and the tail is empty.
#[must_use]
pub fn split_runs(runs: &[StyledRun], at: usize) -> (Vec<StyledRun>, Vec<StyledRun>) {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    for (range, kind, hot) in runs {
        if range.end <= at {
            head.push((range.clone(), *kind, *hot));
        } else if range.start >= at {
            tail.push((range.start - at..range.end - at, *kind, *hot));
        } else {
            head.push((range.start..at, *kind, *hot));
            tail.push((0..range.end - at, *kind, *hot));
        }
    }
    (head, tail)
}

/// Char column for a byte offset into `line`.
///
/// The mouse seam: gpui hit-tests to a byte index, while
/// [`closure_shell_core::BodyEditor`] addresses positions in char
/// columns. Offsets past the end clamp to the line's char length (a
/// click in a line's empty tail parks the cursor at its end), and an
/// offset landing *inside* a multi-byte char rounds down to that
/// char's column rather than splitting it.
#[must_use]
pub fn col_for_byte(line: &str, byte: usize) -> usize {
    if byte >= line.len() {
        return line.chars().count();
    }
    // gpui hit-tests can land mid-char; slicing there would panic, so
    // snap down to the char that byte belongs to.
    let mut boundary = byte;
    while boundary > 0 && !line.is_char_boundary(boundary) {
        boundary -= 1;
    }
    line[..boundary].chars().count()
}

/// Byte offset of char column `col` in `line` — the inverse of
/// [`col_for_byte`]. Columns past the end clamp to the byte length, so
/// the result is always a valid slice index and char boundary.
#[must_use]
pub fn byte_for_col(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map_or_else(|| line.len(), |(b, _)| b)
}

/// Clip a buffer-global selection to one line and rebase it to a local
/// byte range, or `None` when the line carries no selected bytes.
///
/// `line_start` is the line's byte offset in the buffer and `line_len`
/// its byte length. Reversed input is normalised; an empty
/// intersection paints nothing.
#[must_use]
pub fn selection_in_line(
    line_start: usize,
    line_len: usize,
    selection: (usize, usize),
) -> Option<std::ops::Range<usize>> {
    let (lo, hi) = if selection.0 <= selection.1 {
        selection
    } else {
        (selection.1, selection.0)
    };
    let line_end = line_start + line_len;
    let start = lo.max(line_start);
    let end = hi.min(line_end);
    if start >= end {
        return None;
    }
    Some(start - line_start..end - line_start)
}

/// A scrollbar thumb, as fractions of its track: `top` is where it
/// starts, `height` how much of the track it covers. Both in `0.0..=1.0`
/// with `top + height <= 1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thumb {
    /// Distance from the top of the track, as a fraction of it.
    pub top: f32,
    /// The thumb's own length, as a fraction of the track.
    pub height: f32,
}

/// Thumb geometry for a pane showing `viewport` of `content` (both in
/// the same unit — pixels or rows) scrolled down by `scroll`.
///
/// `None` when the content fits, so a pane that does not scroll paints
/// no bar at all. `min_thumb` floors the thumb's length so a huge
/// vault still leaves something grabbable; the floor is taken out of
/// the free track rather than overhanging the end.
#[must_use]
pub fn thumb_geometry(viewport: f32, content: f32, scroll: f32, min_thumb: f32) -> Option<Thumb> {
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    let height = (viewport / content).max(min_thumb).min(1.0);
    let range = content - viewport;
    let progress = (scroll / range).clamp(0.0, 1.0);
    Some(Thumb {
        top: progress * (1.0 - height),
        height,
    })
}

/// Scroll offset for a thumb dragged to `fraction` of its track — the
/// inverse of [`thumb_geometry`], used by the drag handler.
///
/// Clamped to the scrollable range, and zero when the content fits, so
/// a drag can never scroll past the ends or divide by a zero range.
#[must_use]
pub fn scroll_for_track_fraction(viewport: f32, content: f32, fraction: f32) -> f32 {
    if viewport <= 0.0 || content <= viewport {
        return 0.0;
    }
    (content - viewport) * fraction.clamp(0.0, 1.0)
}

/// Classify a [`ModalApp`] status line into a toast for the window's
/// [`closure_shell_core::Feedback`] queue.
///
/// Failures are errors, destructive successes warn, positive outcomes
/// succeed, and hint/chatter lines return `None`.
#[must_use]
pub fn status_toast(status: &str) -> Option<(closure_shell_core::ToastLevel, String)> {
    if status.contains("failed") {
        return Some((closure_shell_core::ToastLevel::Error, status.to_owned()));
    }
    if status.starts_with("deleted: ") {
        return Some((closure_shell_core::ToastLevel::Warning, status.to_owned()));
    }
    if status == "body saved"
        || status == "undo"
        || status == "redo"
        || status.starts_with("folded: ")
        || status.starts_with("unfolded: ")
    {
        return Some((closure_shell_core::ToastLevel::Success, status.to_owned()));
    }
    None
}

/// UTC calendar date `YYYY-MM-DD` for a unix timestamp — the agenda
/// pane's injected *today* (pure: Howard Hinnant's `civil_from_days`,
/// no clock, no chrono dependency).
#[must_use]
pub fn today_ymd(unix_secs: u64) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    // Shift the epoch to the 0000-03-01 era so leap days land at the
    // end of the year-cycle (146097 days per 400-year era).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A `base` px text size at `zoom`.
///
/// Zoom reached the body pane alone at first, then the outline; every
/// other size in the window was a literal, so a picker, the block
/// output, the agenda and the status line all stayed at 11px under a
/// 3× body. This window is one surface, not a frame of independently
/// scaled Emacs buffers: one number scales all of its text, and the
/// sizes keep their ratios to each other.
#[must_use]
pub fn scaled_text_px(base: f32, zoom: f32) -> f32 {
    base * zoom
}

/// Which capture crumbs the bar draws, by index — `None` is the
/// elision gap — and the labels that gap stands for.
///
/// A path deeper than five is elided in the middle: the root, a gap,
/// and the last three, which are the ones that say where a capture is
/// actually going.
#[cfg(feature = "gpui")]
fn crumb_slots(crumbs: &[closure_shell_core::CaptureCrumb]) -> (Vec<Option<usize>>, Vec<String>) {
    let last = crumbs.len().saturating_sub(1);
    if crumbs.len() <= 5 {
        return ((0..crumbs.len()).map(Some).collect(), Vec::new());
    }
    let mut shown = vec![Some(0), None];
    shown.extend((last - 2..=last).map(Some));
    let hidden = crumbs[1..last - 2]
        .iter()
        .map(|c| c.label.clone())
        .collect();
    (shown, hidden)
}

/// [`scaled_text_px`] as gpui pixels, for the row builders that are
/// plain functions rather than methods and so carry the zoom by hand.
#[cfg(feature = "gpui")]
fn sz_at(base: f32, zoom: f32) -> gpui::Pixels {
    px(scaled_text_px(base, zoom))
}

/// The outline's text size at `zoom`, in px.
#[must_use]
pub fn outline_text_px(zoom: f32) -> f32 {
    scaled_text_px(OUTLINE_TEXT, zoom)
}

/// The body pane's text size at `zoom`, in px.
#[must_use]
pub fn body_text_px(zoom: f32) -> f32 {
    scaled_text_px(BODY_TEXT, zoom)
}

/// Unscaled outline row text.
const OUTLINE_TEXT: f32 = 14.0;
/// Unscaled body text.
const BODY_TEXT: f32 = 13.0;

/// The key name the core's editor vocabulary expects, from what gpui
/// reports.
///
/// gpui names letter keysyms in lower case: `shift-a` arrives as key
/// `"a"` with `modifiers.shift` set and `key_char` `"A"`. The body
/// editor's vim vocabulary distinguishes the two — `a` appends, `A`
/// appends at the line end — so passed through raw, every uppercase
/// command in the editor (`A O I G D C S X P V J Y`) was dead. Named
/// keys (`escape`, `enter`, `pageup`) and the symbols gpui already
/// spells out (`$`, `>`) pass through untouched: uppercasing them
/// would invent strokes the core never matches.
///
/// Caps Lock produces the uppercase char with no shift modifier, so
/// the char is honoured as well as the modifier.
#[must_use]
pub fn editor_key(key: &str, shift: bool, key_char: Option<&str>) -> String {
    let single_letter = key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic();
    if !single_letter {
        return key.to_owned();
    }
    let upper = key.to_ascii_uppercase();
    if shift || key_char == Some(upper.as_str()) {
        upper
    } else {
        key.to_owned()
    }
}

/// Whether `surface` can receive pasted text as typed characters.
///
/// A paste is only ever a sequence of keystrokes — the core takes typed
/// characters, and the window is the only place that can reach the
/// system clipboard. So the surfaces that *are* text fields take one,
/// and Browse and the read-only lists must not: there, the characters
/// would be resolved as chords and a pasted URL would run a dozen
/// commands. The body editor takes a paste only in INSERT, for the same
/// reason.
#[must_use]
pub const fn accepts_paste(surface: ModalSurface, insert: bool) -> bool {
    match surface {
        ModalSurface::Search
        | ModalSurface::Capture
        | ModalSurface::Rename
        | ModalSurface::AddSibling
        | ModalSurface::TagsEdit
        | ModalSurface::PropertyEdit
        | ModalSurface::Palette
        | ModalSurface::BodySearch
        | ModalSurface::Ex
        | ModalSurface::Sync
        // The two Q1 pickers are filter fields like Search: a pasted
        // path or buffer name is text to filter with, not chords.
        | ModalSurface::Buffers
        | ModalSurface::Files
        // The refile picker filters by typing, like the others.
        | ModalSurface::Refile
        | ModalSurface::TagPick
        | ModalSurface::Llm => true,
        ModalSurface::EditBody | ModalSurface::EditBlock | ModalSurface::EditFile => insert,
        ModalSurface::Browse
        | ModalSurface::Backlinks
        | ModalSurface::Agenda
        | ModalSurface::Blocks
        | ModalSurface::UndoHistory
        | ModalSurface::Headlines
        | ModalSurface::DbView
        | ModalSurface::Sniffer
        | ModalSurface::Conflicts
        | ModalSurface::Graph
        | ModalSurface::Journal
        | ModalSurface::Cron
        // The date picker takes single keys (h/l/j/k, digits); a
        // pasted blob of text is not a date.
        | ModalSurface::DatePick => false,
    }
}

/// The characters a pasted string types, in order.
///
/// `multiline` says whether the target can hold a line break: the body
/// editor can, a one-line field cannot — and there a break becomes a
/// space, so the words survive even though the shape does not. CRLF
/// collapses to LF, a tab becomes two spaces (a literal tab in INSERT
/// triggers org-tempo/table expansion, which a paste must not), and the
/// remaining control bytes are dropped rather than typed.
#[must_use]
pub fn paste_chars(text: &str, multiline: bool) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => out.push(if multiline { '\n' } else { ' ' }),
            '\t' => out.extend_from_slice(&[' ', ' ']),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Whether a change of `selected` should scroll the *outline* into view
/// on `surface`.
///
/// Nine surfaces reuse `selected` as their own list cursor in the
/// right-hand pane while the outline stays on screen. Revealing that
/// index in the outline scrolled the wrong pane: the left column jumped
/// around while the cursor the user was actually moving stayed out of
/// sight.
#[must_use]
pub const fn outline_follows_selection(surface: ModalSurface) -> bool {
    !matches!(
        surface,
        ModalSurface::Agenda
            | ModalSurface::Blocks
            | ModalSurface::Backlinks
            | ModalSurface::Headlines
            | ModalSurface::DbView
            | ModalSurface::BodySearch
            | ModalSurface::Graph
            | ModalSurface::Journal
            | ModalSurface::Cron
    )
}

/// Where the right-hand pane's cursor row sits among its children on
/// `surface`, or `None` when the pane cannot address a row by index.
///
/// `Some(n)` means row `i` is child `i + n`: the flat lists paint one
/// child per row and start at zero, while the sniffer and the conflict
/// resolver put a button row above theirs — which is why they used to
/// reveal nothing at all rather than reveal the wrong thing, and `j`
/// walked their cursor off the bottom. `None` is for the panes that
/// group rows under section headers, where a child index is not a row
/// index; those keep their cursor visible by windowing the rows
/// instead ([`visible_window`]).
#[must_use]
pub const fn side_reveal_offset(surface: ModalSurface) -> Option<usize> {
    match surface {
        ModalSurface::Headlines
        | ModalSurface::BodySearch
        | ModalSurface::Backlinks
        | ModalSurface::Journal
        | ModalSurface::Cron
        | ModalSurface::UndoHistory => Some(0),
        ModalSurface::Sniffer | ModalSurface::Conflicts => Some(1),
        _ => None,
    }
}

/// How many body lines the editor pane can show, from its measured
/// height.
///
/// The viewport used to be a constant 40 lines, which is two bugs: in a
/// short window the cursor walked off the bottom with nothing scrolling
/// after it (the core scrolls to keep the cursor inside *this* count),
/// and in a tall one the pane painted 40 lines and left the rest of the
/// column blank. `chrome` is the pane's own furniture — mode header,
/// padding — taken off the top.
///
/// Never zero: an unmeasured pane (height 0 before the first layout) or
/// a nonsense line height still paints a few lines rather than an empty
/// editor.
#[must_use]
pub fn body_viewport_lines(pane_height: f32, line_height: f32, chrome: f32) -> usize {
    /// Below this the editor would show nothing usable.
    const MIN: usize = 4;
    /// A hard ceiling: painting more than this per frame is a bug.
    const MAX: usize = 4096;
    if line_height <= 0.0 || !line_height.is_finite() {
        return MIN;
    }
    let usable = pane_height - chrome;
    if !usable.is_finite() || usable < line_height {
        return MIN;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lines = (usable / line_height).floor() as usize;
    lines.clamp(MIN, MAX)
}

/// Scroll fraction for a pointer at `y` on a scrollbar track, with the
/// thumb centred under it.
///
/// `track_top` is where the track starts in window space, `viewport`
/// its length and `thumb_height` the thumb's length as a fraction of
/// it ([`Thumb::height`]). The pointer used to be read as the scroll
/// fraction directly, which placed the *top* of the thumb wherever the
/// pointer was — so the thumb trailed the mouse by up to its own
/// length and never sat under the finger dragging it. Here the pointer
/// grabs the thumb's middle, and the free travel is the track minus
/// the thumb, so both ends are reachable without leaving the track.
///
/// Clamped to `0.0..=1.0`; a degenerate track or a full-length thumb
/// has nowhere to go and yields zero.
#[must_use]
pub fn track_fraction(y: f32, track_top: f32, viewport: f32, thumb_height: f32) -> f32 {
    let thumb = thumb_height.clamp(0.0, 1.0) * viewport;
    let free = viewport - thumb;
    if free <= 0.0 || !free.is_finite() {
        return 0.0;
    }
    ((y - track_top - thumb / 2.0) / free).clamp(0.0, 1.0)
}

/// How tall the floating palette's list is, in px, for `rows` matches
/// at `zoom`.
///
/// A `uniform_list` fills the space it is handed and asks for none of
/// its own, and the palette panel sizes to its content — so "grow" had
/// nothing to grow into and the list painted zero pixels tall, under a
/// query line that worked. The height is therefore stated: a row per
/// match, one row's worth when there are none (the empty line lives
/// there), and a cap so a long list stops rather than running off the
/// window — past the cap it scrolls, which is what the scrollbar
/// beside it is for.
#[must_use]
pub fn palette_list_height(rows: usize, zoom: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a row count this side of the cap is exact in f32"
    )]
    let n = rows.clamp(1, PALETTE_LIST_ROWS) as f32;
    n * scaled_text_px(PALETTE_ROW_H, zoom)
}

/// A palette row's text size.
const PALETTE_ROW_TEXT: f32 = 13.0;
/// One palette row: its line box plus the `py_1` above and below it.
const PALETTE_ROW_H: f32 = PALETTE_ROW_TEXT * 1.4 + 8.0;
/// The most rows the palette shows before it scrolls instead.
const PALETTE_LIST_ROWS: usize = 12;

/// Keep only the which-key entries that can follow the chord already
/// typed.
///
/// The panel opens on a pending chord, which is the one moment it
/// should be showing what comes *next* rather than the entire keymap —
/// Doom's behaviour, and the reason the panel exists. An empty prefix
/// (the pinned-open panel) returns everything unchanged; a group left
/// with no entries is dropped rather than shown as a bare title.
///
/// Matching is whole strokes: chords are space-separated, so `SPC f`
/// matches `SPC f f` and not `SPC fx y`.
#[must_use]
pub fn which_key_filter(
    groups: Vec<(String, Vec<(String, String)>)>,
    prefix: &str,
) -> Vec<(String, Vec<(String, String)>)> {
    if prefix.is_empty() {
        return groups;
    }
    let head = format!("{prefix} ");
    groups
        .into_iter()
        .filter_map(|(title, entries)| {
            let kept: Vec<(String, String)> = entries
                .into_iter()
                .filter(|(chord, _)| chord.starts_with(&head))
                .collect();
            (!kept.is_empty()).then_some((title, kept))
        })
        .collect()
}

/// Launch fallback when the `gpui` feature is disabled (the default,
/// hermetic build). The kernel-side [`Shell`] is always available; the
/// GPU window requires `--features gpui` and the system GPU/X11 libs.
#[cfg(not(feature = "gpui"))]
pub fn run(_vault_path: &Path) -> Result<(), String> {
    Err(
        "gpui shell not compiled: rebuild closure-cli with `--features gpui` \
         (pulls Zed's GPU stack + system X11/xkbcommon/freetype). \
         The egui shell is the default native path."
            .to_owned(),
    )
}

// === The reference GUI window ===
// A real Zed/gpui window over the ModalApp command surface: modal
// keybindings with pending-chord which-key, clickable everything
// (rows, fold arrows, which-key chips, palette, detail fields), theme
// tokens from config, live editing through the Shell (I8). Compiled
// only under `--features gpui`.

#[cfg(feature = "gpui")]
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    ScrollDelta, ScrollWheelEvent, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb,
    size,
};

/// Run this same command again with the Vulkan loader pointed at the
/// software rasteriser at `icd`, and report how it went.
///
/// The loader reads its driver override from the environment before
/// this process gets to ask, and setting an environment variable is
/// `unsafe` under edition 2024 — which this workspace forbids outright.
/// So the choice goes to a fresh process instead: the same executable,
/// the same arguments, one variable added. `VK_DRIVER_FILES` is the
/// current name and `VK_ICD_FILENAMES` the one older loaders read; a
/// loader that knows both prefers the former.
///
/// The child cannot loop back here: it finds the override set, and an
/// override is believed outright.
#[cfg(feature = "gpui")]
fn rerun_on_software_rasteriser(icd: &Path) -> Result<(), String> {
    eprintln!(
        "closure: no Vulkan driver on this machine — rendering on the lavapipe software \
         rasteriser ({}). Frames are slow; `closure tui` is not.",
        icd.display()
    );
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot find closure's own path to re-run it: {e}"))?;
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .env("VK_DRIVER_FILES", icd)
        .env("VK_ICD_FILENAMES", icd)
        .status()
        .map_err(|e| format!("cannot re-run on the software rasteriser: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("software rasteriser run ended: {status}"))
    }
}

/// Launch the gpui desktop window against the vault at `vault_path`.
/// Blocks until the window closes.
///
/// # Errors
///
/// Returns the vault open error as a string; window/runtime failures
/// surface through gpui's own panics on the UI thread.
#[cfg(feature = "gpui")]
pub fn run(vault_path: &Path) -> Result<(), String> {
    // gpui `unwrap`s its GPU context, so without this the failure mode
    // on a machine with no Vulkan driver is a panic and a backtrace
    // through `blade_graphics` that names nothing you could install.
    match gpui_preflight(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
        &vulkan_icd_dirs(),
        std::env::var("VK_ICD_FILENAMES").ok().as_deref(),
        std::env::var("CLOSURE_SOFTWARE_ICD").ok().as_deref(),
    ) {
        Preflight::Ready => {}
        Preflight::Software(icd) => return rerun_on_software_rasteriser(&icd),
        Preflight::Refused(why) => return Err(why),
    }
    let vault = Vault::open(vault_path).map_err(|e| format!("{e}"))?;
    let theme = resolve_theme(vault_path);
    let input_mode = resolve_input_mode(vault_path);
    let view = resolve_view(vault_path);
    let (sync_bind, sync_advertise) = resolve_sync_addrs(vault_path);
    let wrap =
        closure_config::Config::from_path(&vault_path.join("config.org")).is_ok_and(|c| c.wrap);
    // The window manager needs a name for the title bar, the task
    // switcher and the Wayland app id — an untitled window is the one
    // the user cannot find again. The vault is what distinguishes two
    // closure windows, so it is in the title.
    let title = format!(
        "closure — {}",
        vault_path.file_name().map_or_else(
            || vault_path.display().to_string(),
            |n| n.to_string_lossy().into_owned()
        )
    );
    let view_pref = view;
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1080.0), px(720.0)), cx);
        // Closing the last window must end the process. gpui keeps the
        // app alive with no windows, which left an invisible closure
        // running after the user clicked the X.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(title.clone().into()),
                    ..Default::default()
                }),
                app_id: Some("net.wolfhard.closure".to_owned()),
                window_min_size: Some(size(px(640.0), px(400.0))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut view = GpuiView::new(Shell::new(vault), input_mode, theme, cx);
                    // `view = editor` in the vault's config.org: open
                    // the file buffer before the first frame, so the
                    // window the user asked for is the one they get.
                    view.set_view(view_pref);
                    view.set_wrap(wrap);
                    // Pairing has to know where it listens before the
                    // user opens the surface: the ticket shown there is
                    // what gets pasted into the other machine.
                    view.app.configure_sync(sync_bind, sync_advertise);
                    // Peers paired with before are still peers.
                    view.app.load_peers(&view.shell);
                    // …and the note the last session was in is where
                    // this one starts.
                    view.app.restore_last_place(&view.shell);
                    view
                })
            },
        );
        match opened {
            Ok(window) => {
                window
                    .update(cx, |view, window, cx| {
                        window.focus(&view.focus_handle(cx));
                        // A body edit still in the buffer when the X is
                        // clicked used to go with the window. The
                        // gesture that closed it is repeatable; the
                        // paragraph that was in the buffer is not, so
                        // the text wins and the close goes ahead.
                        let handle = cx.entity().downgrade();
                        window.on_window_should_close(cx, move |_w, cx| {
                            handle
                                .update(cx, |view: &mut GpuiView, _cx| {
                                    if view.app.save_pending_edit(&mut view.shell) {
                                        eprintln!(
                                            "closure: saved the open body edit before closing"
                                        );
                                    }
                                    // Where you were, for the next
                                    // session — written here rather
                                    // than on every arrow key.
                                    view.app.save_last_place(&view.shell);
                                })
                                .ok();
                            true
                        });
                        let _ = view;
                    })
                    .ok();
            }
            // A failed open used to be dropped on the floor, leaving a
            // running process with no window and no explanation.
            Err(e) => {
                eprintln!("closure: opening the gpui window failed: {e}");
                cx.quit();
            }
        }
        cx.activate(true);
    });
    Ok(())
}

/// Theme colours packed for gpui, derived once per frame.
#[cfg(feature = "gpui")]
#[derive(Clone, Copy)]
struct Colors {
    bg: u32,
    panel: u32,
    fg: u32,
    muted: u32,
    accent: u32,
    selection: u32,
    /// A louder selection, for text: the row tint is deliberately
    /// subtle, and reusing it behind glyphs made VISUAL invisible.
    selection_text: u32,
    hover: u32,
    error: u32,
    warning: u32,
    success: u32,
    border: u32,
    heading2: u32,
    heading3: u32,
    code: u32,
}

#[cfg(feature = "gpui")]
impl Colors {
    fn of(theme: &Theme) -> Self {
        use closure_shell_core::ColorRole as R;
        let c = |r| color_u32(theme.color(r));
        let bg = c(R::Bg);
        let fg = c(R::Fg);
        let selection = c(R::Selection);
        Self {
            bg,
            panel: mix_u32(bg, selection, 64),
            fg,
            muted: c(R::Muted),
            accent: c(R::Accent),
            selection,
            selection_text: mix_u32(selection, c(R::Accent), 110),
            hover: mix_u32(bg, selection, 128),
            error: c(R::Error),
            warning: c(R::Warning),
            success: c(R::Success),
            border: mix_u32(bg, fg, 32),
            heading2: c(R::Heading2),
            heading3: c(R::Heading3),
            code: c(R::Code),
        }
    }

    /// doom-vibrant outline colour for a headline `level` (outline-1
    /// blue, outline-2 magenta, outline-3 violet, cycling).
    const fn outline(self, level: u8) -> u32 {
        match (level.saturating_sub(1)) % 3 {
            0 => self.accent,
            1 => self.heading2,
            _ => self.heading3,
        }
    }
}

/// Height of one body-editor line, in pixels — the `min_h` the pane
/// gives each row, and so the divisor that turns the pane's measured
/// height into a line count ([`body_viewport_lines`]).
#[cfg(feature = "gpui")]
const BODY_LINE_H: f32 = 18.0;

/// Width reserved for the TODO keyword in an outline row, painted or
/// not — a column that appears only on some rows moves every title
/// beside it.
#[cfg(feature = "gpui")]
const TODO_COL_W: f32 = 44.0;
/// Most a row will spend on its file name before clipping it.
#[cfg(feature = "gpui")]
const PATH_COL_W: f32 = 120.0;

/// Where the outline column starts, and the range a drag may put it in.
/// Narrower than the minimum is a column that cannot show a title;
/// wider than the maximum leaves no room for the pane it is beside.
#[cfg(feature = "gpui")]
const OUTLINE_W_DEFAULT: f32 = 420.0;
/// See [`OUTLINE_W_DEFAULT`].
#[cfg(feature = "gpui")]
const OUTLINE_W_MIN: f32 = 220.0;
/// See [`OUTLINE_W_DEFAULT`].
#[cfg(feature = "gpui")]
const OUTLINE_W_MAX: f32 = 900.0;

/// The editor pane's own furniture above the text: the mode header and
/// the pane padding, taken off the height before it is divided.
#[cfg(feature = "gpui")]
const BODY_CHROME: f32 = 46.0;

/// Lines to assume before the pane has ever been laid out (its measured
/// height is zero on the first frame).
#[cfg(feature = "gpui")]
const BODY_VIEW_DEFAULT: usize = 40;

/// Columns to assume before the pane has ever been laid out.
#[cfg(feature = "gpui")]
const BODY_COLS_DEFAULT: usize = 80;

/// How long a toast stays on the strip.
#[cfg(feature = "gpui")]
const TOAST_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// gpui view: owns the kernel-side [`Shell`] and the pure [`ModalApp`]
/// editor state, plus a focus handle so the root receives key events.
///
/// Public so the window can be built and driven by a test over gpui's
/// stub platform ([`test_window`]) rather than only compile-checked.
#[cfg(feature = "gpui")]
pub struct GpuiView {
    shell: Shell,
    app: ModalApp,
    theme: Theme,
    focus_handle: FocusHandle,
    /// The shared async-feedback queue (G7) this window renders as a
    /// toast strip; fed by [`status_toast`] over the status line.
    feedback: closure_shell_core::Feedback,
    /// Last absorbed status line (change detection for the toasts).
    last_status: String,
    /// Typing-idle generation for the completion auto-popup: each key
    /// bumps it, a delayed task only fires if it is still the newest.
    popup_gen: u64,
    /// Outline row drag-and-drop gesture (G5c machine); the drop maps
    /// to registry moves via `drag_drop_rows` (I8).
    drag: closure_shell_core::DragReorder,
    /// Scroll state of the virtualized outline list. Owning it here is
    /// what lets the pane paint a scrollbar and keep the keyboard
    /// selection in view.
    outline_scroll: gpui::UniformListScrollHandle,
    /// Scroll state of the right-hand pane (detail, lists, editor).
    side_scroll: gpui::ScrollHandle,
    /// Bounds of the body editor's painted text, so its own scrollbar
    /// knows where its track is. The editor virtualizes its lines, so
    /// this handle never actually scrolls — it is a measurement.
    body_track: gpui::ScrollHandle,
    /// Last selection the outline was scrolled to, so a keyboard move
    /// reveals its row exactly once instead of fighting the wheel.
    revealed: usize,
    /// The same, for the right-hand pane's own list cursor.
    side_revealed: usize,
    /// The same, for the virtualized command palette.
    palette_revealed: usize,
    /// Chat turns already scrolled to, so a new answer brings itself
    /// into view exactly once.
    chat_seen: usize,
    /// Toast generation: each new toast bumps it and arms a timer, and
    /// the timer only clears the strip if nothing arrived after it.
    toast_gen: u64,
    /// Where an open context menu is anchored, if one is open.
    menu: Option<(gpui::Point<gpui::Pixels>, closure_shell_core::ContextTarget)>,
    /// on its right edge.
    outline_w: f32,
    /// While the outline edge is being dragged: the offset between the
    /// pointer and the column edge, so the column does not jump to the
    /// cursor on the first pixel of movement.
    outline_drag: Option<f32>,
    /// Whether long body lines soft-wrap (`wrap = true` in config.org)
    /// rather than scrolling sideways.
    wrap: bool,
    /// The vault's directory name, for the window title.
    vault_name: String,
    /// The title last written to the window manager, so a frame that
    /// changed nothing does not re-announce it.
    window_title: String,
    /// Whether an inbound-sync accept is currently waiting on the
    /// listener. A network-facing listener that had nobody to trust
    /// refuses to accept, and the paste that fixes that has to re-arm
    /// it — without this, the second accept would be spawned onto a
    /// socket the first one is already blocked on.
    accept_armed: bool,
    /// Whether the full which-key panel is pinned open. A pending
    /// chord shows it regardless; this is the explicit "show me
    /// everything" toggle.
    /// Scroll state of the which-key panel — it lists every binding in
    /// the mode, which does not fit a window.
    which_key_scroll: gpui::ScrollHandle,
    /// Scroll state of the virtualized command palette.
    palette_scroll: gpui::UniformListScrollHandle,
    /// Memoised body highlighting, keyed on the text it describes.
    ///
    /// The detail pane and the editor both re-highlight on every
    /// frame, and scrolling repaints without changing a byte, so the
    /// classification is kept until the text does change.
    highlight_cache: std::cell::RefCell<Option<(String, HighlightedBody)>>,
    /// The IME's preedit: the byte range in the body buffer holding
    /// text the input method is still composing.
    ///
    /// A compose sequence or a CJK input method builds a character over
    /// several keystrokes and hands back provisional text on the way.
    /// Without this the window read `KeyDownEvent.key_char` only, so a
    /// dead key produced nothing and an IME could not type at all.
    marked: Option<std::ops::Range<usize>>,
    /// Vault-reload generation: each armed poll carries one, and only
    /// the newest re-arms, so the loop cannot fork.
    reload_gen: u64,
    /// How many body lines the *last painted frame* used.
    ///
    /// The editor sizes itself from its own measured height, which only
    /// exists after it has been laid out once — so the frame that opens
    /// a buffer paints with the previous layout's count. Opening a
    /// 17-line file into a pane last measured at 15 lines painted 15 of
    /// them and stopped, with half the window empty below, until some
    /// unrelated keystroke repainted. Comparing this against the fresh
    /// measurement is how the pane knows to ask for one more frame.
    painted_view: std::cell::Cell<usize>,
}

#[cfg(feature = "gpui")]
impl Focusable for GpuiView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(feature = "gpui")]
impl GpuiView {
    /// Build the view over `shell`. The only constructor, so a test
    /// window and the real one cannot drift apart in their setup.
    pub fn new(
        shell: Shell,
        input_mode: closure_config::InputMode,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        let vault_name = shell.vault.root().file_name().map_or_else(
            || shell.vault.root().display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut app = ModalApp::new(input_mode);
        // The window owns the clock; the core owns the calendar (Q3-V4
        // / V3). Set once here so the first frame already knows what
        // day it is, and again on every frame that paints a date.
        app.set_now(&closure_shell_core::now_local());
        Self {
            shell,
            app,
            theme,
            vault_name,
            wrap: false,

            outline_w: OUTLINE_W_DEFAULT,
            outline_drag: None,
            window_title: String::new(),
            focus_handle: cx.focus_handle(),
            feedback: closure_shell_core::Feedback::default(),
            last_status: String::new(),
            popup_gen: 0,
            drag: closure_shell_core::DragReorder::default(),
            outline_scroll: gpui::UniformListScrollHandle::new(),
            side_scroll: gpui::ScrollHandle::new(),
            body_track: gpui::ScrollHandle::new(),
            revealed: usize::MAX,
            side_revealed: usize::MAX,
            palette_revealed: usize::MAX,
            chat_seen: 0,
            toast_gen: 0,
            menu: None,
            accept_armed: false,
            which_key_scroll: gpui::ScrollHandle::new(),
            palette_scroll: gpui::UniformListScrollHandle::new(),
            highlight_cache: std::cell::RefCell::new(None),
            marked: None,
            reload_gen: 0,
            painted_view: std::cell::Cell::new(0),
        }
    }

    /// How many rows the outline is showing.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.app.rows_shared(&self.shell).len()
    }

    /// The text scale the panes are painted at.
    #[must_use]
    pub fn zoom(&self) -> f32 {
        self.app.zoom()
    }

    /// A `base` px text size at the window's current zoom — every
    /// `text_size` in the window goes through here ([`scaled_text_px`]).
    fn sz(&self, base: f32) -> gpui::Pixels {
        px(scaled_text_px(base, self.app.zoom()))
    }

    /// The active surface, for a test to assert on.
    #[must_use]
    pub const fn surface(&self) -> ModalSurface {
        self.app.surface()
    }

    /// What the window paints underneath the palette — the surface
    /// itself everywhere else.
    #[must_use]
    pub fn surface_beneath(&self) -> ModalSurface {
        self.app.surface_beneath()
    }

    /// Tell the core what day it is — the window owns the clock
    /// (Q3-V4); a test owns it too, which is why this is public.
    pub fn set_today(&mut self, ymd: &str) {
        self.app.set_today(ymd);
    }

    /// The date the picker's cursor is on (`YYYY-MM-DD`), empty when it
    /// is closed.
    #[must_use]
    pub fn picked_date(&self) -> String {
        self.app.date_grid().selected
    }

    /// Put the picker's cursor on `day` — the click path, reachable by
    /// a test without a pointer.
    pub fn date_click(&mut self, day: u32, cx: &mut Context<Self>) {
        self.app.date_click(day);
        cx.notify();
    }

    /// How many buffers this session has open (Q1).
    #[must_use]
    pub fn buffer_row_count(&self) -> usize {
        self.app.buffer_rows(&self.shell).len()
    }

    /// How many files the picker is offering (Q1).
    #[must_use]
    pub fn file_row_count(&self) -> usize {
        self.app.file_rows(&self.shell).len()
    }

    /// The name of the buffer on screen, as the tab strip shows it.
    #[must_use]
    pub fn current_buffer_name(&self) -> Option<String> {
        self.app
            .buffer_rows(&self.shell)
            .into_iter()
            .find(|r| r.current)
            .map(|r| r.name)
    }

    /// Whether the tab strip is painted this frame — it earns its row
    /// only once a second buffer exists.
    #[must_use]
    pub fn tab_strip_visible(&self) -> bool {
        self.buffer_row_count() >= 2
    }

    /// Switch to the buffer on row `i` of the list — the click path,
    /// reachable by a test without a pointer.
    pub fn buffer_click(&mut self, i: usize, cx: &mut Context<Self>) {
        self.app.buffer_click(&self.shell, i);
        cx.notify();
    }

    /// Put the window in `view` — the config's answer at startup, and
    /// what a test sets to look at the other shape.
    pub fn set_view(&mut self, view: closure_shell_core::ViewMode) {
        self.app.set_view(view, &self.shell);
    }

    /// Fill the window's middle row with the panes this surface wants.
    ///
    /// A buffer takes the window: editing a body in a third of it,
    /// beside a list of the headlines you are *not* editing, is a
    /// preview — `org-edit-special` gets its own frame in Emacs, and so
    /// does this. But writing *into* an outline is a different job from
    /// reading one, so `toggle-tree` brings the tree back beside the
    /// buffer without leaving it.
    fn panes(&self, body: gpui::Div, co: Colors, cx: &Context<Self>) -> gpui::Div {
        if !self.app.surface_beneath().is_editor() {
            return body
                .child(self.rail(co, cx))
                .child(self.rows_pane(co, cx))
                .child(self.side_pane(co, cx));
        }
        if self.app.tree_open() {
            body.child(self.rows_pane(co, cx))
                .child(self.side_pane(co, cx))
        } else {
            body.child(self.side_pane(co, cx))
        }
    }

    /// Tell the window manager what this window is called, when that
    /// has changed.
    ///
    /// The title is the only part of the app a task switcher shows, and
    /// it used to be set once at creation and never moved. Written only
    /// on a change — a window manager treats every set as an event.
    fn refresh_title(&mut self, window: &mut Window) {
        let title = self.app.window_title(&self.shell, &self.vault_name);
        if self.window_title != title {
            window.set_window_title(&title);
            self.window_title = title;
        }
    }

    /// Soft-wrap long body lines instead of scrolling sideways
    /// (`wrap = true` in config.org).
    pub const fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    /// Whether the editor is wrapping.
    #[must_use]
    pub const fn wraps(&self) -> bool {
        self.wrap
    }

    /// The activity rail's destinations — what [`Self::rail`] paints.
    #[must_use]
    pub fn destinations(&self) -> Vec<closure_shell_core::Destination> {
        self.app.destinations(&self.shell)
    }

    /// The body editor's buffer, for a test to assert on.
    #[must_use]
    pub fn body(&self) -> &str {
        self.app.body_buffer()
    }

    /// The status line, for a test to assert on.
    #[must_use]
    pub fn status(&self) -> &str {
        self.app.status()
    }

    /// Write out a body edit still in progress — what the window's
    /// close hook does. `true` when there was something to save.
    pub fn save_pending_edit(&mut self) -> bool {
        self.app.save_pending_edit(&mut self.shell)
    }

    /// Whether any file in the vault contains `needle`, for a test to
    /// check that an edit reached disk.
    #[must_use]
    pub fn vault_contains(&self, needle: &str) -> bool {
        self.shell
            .vault
            .iter()
            .any(|(_, doc)| doc.source().contains(needle))
    }

    /// Commit composed text, as the platform's input method does.
    ///
    /// The named entry points to the `EntityInputHandler` impl: a test
    /// drives the same methods the platform calls, so what is covered
    /// is the handler rather than a paraphrase of it.
    pub fn ime_commit(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        gpui::EntityInputHandler::replace_text_in_range(self, range, text, window, cx);
    }

    /// Hand over provisional (preedit) text mid-composition.
    pub fn ime_mark(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        gpui::EntityInputHandler::replace_and_mark_text_in_range(
            self, range, text, None, window, cx,
        );
    }

    /// Abandon a composition in progress.
    pub fn ime_unmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        gpui::EntityInputHandler::unmark_text(self, window, cx);
    }

    /// Run one vault-reload pass, as the armed poll does.
    pub fn poll_vault(&mut self, cx: &mut Context<Self>) {
        self.reload_vault(cx);
    }

    /// Run a registry command the way a mouse affordance does — the
    /// same [`Self::click`] a which-key chip, a detail field or a menu
    /// entry goes through.
    pub fn run_command(&mut self, command: &str, cx: &mut Context<Self>) {
        self.click(command, cx);
    }

    /// Run one `:` line, the way the ex overlay does — what the buffer
    /// is left by (`:q`, `:w`, `:wq`) since Esc became the mode key.
    pub fn run_ex_line(&mut self, line: &str, cx: &mut Context<Self>) {
        self.app.run_ex_line(&mut self.shell, line);
        if self.app.should_quit() {
            cx.quit();
        }
        cx.notify();
    }

    /// The outline's selected row index, for a test to assert a click
    /// landed on the row it aimed at.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.app.selected()
    }

    /// Whether row `i` is folded, for a test to assert a fold-arrow
    /// click did what the chord does.
    #[must_use]
    pub fn row_folded(&self, i: usize) -> bool {
        self.app
            .rows_shared(&self.shell)
            .get(i)
            .is_some_and(|r| r.folded)
    }

    /// Row `i`'s TODO keyword, if it has one.
    #[must_use]
    pub fn row_todo(&self, i: usize) -> Option<String> {
        self.app
            .rows_shared(&self.shell)
            .get(i)
            .and_then(|r| r.todo.clone())
    }

    /// Row `i`'s title.
    #[must_use]
    pub fn row_title(&self, i: usize) -> Option<String> {
        self.app
            .rows_shared(&self.shell)
            .get(i)
            .map(|r| r.title.clone())
    }

    /// Whether a context menu is open.
    #[must_use]
    pub const fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Whether the which-key panel is pinned open.
    ///
    /// The state is the core's: the panel answers to a command and a
    /// chord (`?`), and the button here runs the same command rather
    /// than keeping a second copy of the answer.
    #[must_use]
    pub const fn which_key_open(&self) -> bool {
        self.app.which_key_open()
    }

    /// The chord waiting for its next key, if one is.
    #[must_use]
    pub fn pending_chord(&self) -> String {
        self.app.pending_chord()
    }

    /// How many toasts are on the strip.
    #[must_use]
    pub fn toast_count(&self) -> usize {
        self.feedback.items().len()
    }

    /// The first visible body line, for a test to assert the wheel
    /// moved the editor's own viewport.
    #[must_use]
    pub fn body_scroll_start(&self) -> usize {
        self.app.body_scroll_start(self.body_view())
    }

    /// Where the outline is scrolled to, in pixels from the top.
    #[must_use]
    pub fn outline_scroll_top(&self) -> f32 {
        -f32::from(self.outline_scroll.0.borrow().base_handle.offset().y)
    }

    /// The same, for the right-hand pane.
    #[must_use]
    pub fn side_scroll_top(&self) -> f32 {
        -f32::from(self.side_scroll.offset().y)
    }

    /// The right-hand pane's measured viewport height and how far its
    /// content runs past it — what [`scrollbar`] sizes its thumb from.
    #[must_use]
    pub fn side_scroll_extent(&self) -> (f32, f32) {
        (
            f32::from(self.side_scroll.bounds().size.height),
            f32::from(self.side_scroll.max_offset().height),
        )
    }

    /// The body editor's cursor, as (line, column).
    #[must_use]
    pub fn body_cursor(&self) -> (usize, usize) {
        self.app.body_cursor()
    }

    /// Feed a keystroke the way the window's own handler does.
    ///
    /// The same seam `on_key` uses, so a test drives the shell through
    /// exactly the translation the window performs rather than a
    /// parallel one that can agree with nothing.
    pub fn press(&mut self, key: &str, shift: bool, ctrl: bool, cx: &mut Context<Self>) {
        self.press_with(key, shift, ctrl, false, cx);
    }

    /// [`Self::press`] with the meta layer as well — `M-x` is a chord
    /// the window answers and the buffer does not.
    pub fn press_with(
        &mut self,
        key: &str,
        shift: bool,
        ctrl: bool,
        alt: bool,
        cx: &mut Context<Self>,
    ) {
        let text = (!ctrl && !alt && key.chars().count() == 1).then(|| {
            let c = key.chars().next().unwrap_or(' ');
            if shift { c.to_ascii_uppercase() } else { c }
        });
        let named = editor_key(key, shift, text.map(|c| c.to_string()).as_deref());
        self.dispatch_key(&named, ctrl, alt, text, cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = &ks.modifiers;
        // The window is a modal editor: every key belongs to it, and
        // saying so is not a formality. When a key event bubbles out
        // unclaimed, gpui hands its `key_char` straight to the
        // installed input method handler as if it were composed text
        // (`x11/window.rs`, `wayland/window.rs`, `Window::
        // dispatch_keystroke`) — and this window installs one, for
        // dead keys and CJK. So every printable keystroke was applied
        // twice: once here, once by gpui behind us. `hello` typed into
        // a body arrived as `hheelllloo`.
        cx.stop_propagation();
        let text = ks
            .key_char
            .as_ref()
            .and_then(|s| s.chars().next())
            .filter(|_| !m.control && !m.alt && !m.platform && !m.function);
        // The clipboard is the window's to reach, and the desktop
        // chords for it are not in any keymap.
        if self.clipboard_key(ks, cx) {
            cx.notify();
            return;
        }
        // gpui lowercases letter keysyms; the editor's vim vocabulary
        // does not ([`editor_key`]).
        let key = editor_key(&ks.key, m.shift, ks.key_char.as_deref());
        // The core's key names carry ctrl and alt as flags but not
        // shift, so the keys where shift *changes the command* arrive
        // spelled out: Shift+Enter is the newline in a field whose
        // plain Enter means "accept", and org's table chords are the
        // arrows and TAB with shift on them (`M-S-<right>` inserts a
        // column where `M-<right>` moves one).
        let key = if m.shift
            && matches!(
                key.as_str(),
                "enter" | "tab" | "left" | "right" | "up" | "down"
            ) {
            format!("shift-{key}")
        } else {
            key
        };
        self.dispatch_key(&key, m.control, m.alt, text, cx);
    }

    /// Everything a keystroke does once it has a name: dispatch it into
    /// the core, then the three window-side follow-ups (asking a
    /// provider, quitting, arming the completion popup).
    ///
    /// Factored out so [`Self::press`] drives the same path a real key
    /// event does — a test seam that agrees with nothing is worse than
    /// none.
    fn dispatch_key(
        &mut self,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
        cx: &mut Context<Self>,
    ) {
        let asking = self.app.surface() == ModalSurface::Llm && key == "enter";
        let pairing = self.app.surface() == ModalSurface::Sync && key == "enter";
        self.app.on_key(&mut self.shell, key, ctrl, alt, text);
        // Pasting a ticket is what makes a network-facing listener
        // willing to answer, so it is also what re-arms the accept the
        // guard refused.
        if pairing && !self.accept_armed && self.app.sync_mut().listener().is_some() {
            self.accept_one(cx);
        }
        // Enter on the assistant surface records the question in the
        // core; sending it is I/O, so it happens here.
        if asking
            && self.app.chat_busy()
            && let Some(question) = self
                .app
                .chat_turns()
                .last()
                .filter(|t| t.from_user)
                .map(|t| t.text.clone())
        {
            self.ask_llm(question, cx);
        }
        if self.app.should_quit() {
            cx.quit();
        }
        // C2: dabbrev auto-popup after a typing-idle delay. Each key
        // bumps the generation; the timer only fires for the newest.
        self.popup_gen = self.popup_gen.wrapping_add(1);
        if self.app.completion_should_popup(&self.shell) {
            let generation = self.popup_gen;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(350))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.popup_gen == generation {
                        this.app.open_completion_popup(&this.shell);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// [`highlight_body`] for `text`, memoised on the text itself.
    ///
    /// Both the editor and the read-only preview re-highlight every
    /// frame, and scrolling repaints without changing a byte — so the
    /// classification is kept until the text actually differs.
    fn highlighted(&self, text: &str) -> HighlightedBody {
        {
            let cache = self.highlight_cache.borrow();
            if let Some((cached, spans)) = cache.as_ref()
                && cached == text
            {
                return std::rc::Rc::clone(spans);
            }
        }
        let spans = std::rc::Rc::new(highlight_body(text));
        *self.highlight_cache.borrow_mut() = Some((text.to_owned(), std::rc::Rc::clone(&spans)));
        spans
    }

    /// Feed a changed status line through [`status_toast`] into the
    /// shared feedback queue (the toast strip's only source).
    ///
    /// Called once per frame rather than per gesture: every path that
    /// sets a status — a click, a chord, a finished sync, a followed
    /// link — then reaches the strip, instead of only the two that
    /// remembered to ask.
    fn absorb_status(&mut self, cx: &Context<Self>) {
        let status = self.app.status().to_owned();
        if status != self.last_status {
            if let Some((level, text)) = status_toast(&status) {
                self.feedback.notify(level, text);
                self.arm_toast_timer(cx);
            }
            self.last_status = status;
        }
    }

    /// Expire the toast strip after [`TOAST_TTL`].
    ///
    /// [`closure_shell_core::Feedback`] is a queue with no clock in it,
    /// so nothing used to remove an item: the strip kept the last three
    /// messages on screen for the rest of the session — a stale "body
    /// saved" over an hour of editing — and the queue behind it grew
    /// without bound. Each new toast bumps the generation and arms a
    /// timer; the timer clears the strip only if it is still the
    /// newest, so a burst of messages expires once, together.
    fn arm_toast_timer(&mut self, cx: &Context<Self>) {
        self.toast_gen = self.toast_gen.wrapping_add(1);
        let generation = self.toast_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_TTL).await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_gen == generation {
                    this.feedback.clear();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The desktop clipboard chords, which live in no keymap: `C-v` /
    /// `S-insert` paste, `C-c` copies the body selection. `true` when
    /// the key was one of them and has been handled.
    ///
    /// Nothing else in the shell can reach the system clipboard — the
    /// core is dep-free and the vault's kill ring is internal — so a
    /// ticket handed over in the sync pane, a headline copied out of a
    /// browser, or a snippet pasted into a note all pass through here.
    fn clipboard_key(&mut self, ks: &gpui::Keystroke, cx: &Context<Self>) -> bool {
        let m = &ks.modifiers;
        let cmd = m.control || m.platform;
        let pasting =
            (cmd && ks.key == "v") || (m.shift && ks.key == "insert") || ks.key == "paste";
        let copying = (cmd && ks.key == "c") || ks.key == "copy";
        if pasting {
            return self.paste_clipboard(cx);
        }
        if copying {
            return self.copy_selection(cx);
        }
        false
    }

    /// Type the clipboard into the active surface.
    ///
    /// Only where the characters are text rather than commands
    /// ([`accepts_paste`]) — everywhere else the key falls through to
    /// the keymap, so a future `C-v` binding still works. The paste is
    /// fed as keystrokes, which is what makes it undoable and what
    /// keeps the field logic (slash menu, completion, table alignment)
    /// in charge of it.
    fn paste_clipboard(&mut self, cx: &Context<Self>) -> bool {
        /// A paste is typed character by character; past this a single
        /// gesture would stall the UI thread.
        const MAX: usize = 20_000;
        let surface = self.app.surface();
        let insert = self.app.body_mode() == closure_shell_core::EditorMode::Insert;
        // A screenshot on the clipboard is a paste too. It is filed in
        // the vault and the note gets a relative link, so what is in
        // the file is the same org an editor on the other machine will
        // read.
        if surface.is_editor() && self.paste_clipboard_image(cx) {
            return true;
        }
        if !accepts_paste(surface, insert) {
            // Outside INSERT the editor still takes a paste — it just
            // must not arrive as keystrokes, which in NORMAL would run
            // a pasted URL as a dozen commands. `C-v` in vim/Doom mode
            // did nothing at all before this.
            if surface.is_editor() {
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    self.app.set_status("clipboard is empty".to_owned());
                    return true;
                };
                self.app.body_paste_text(&text);
                return true;
            }
            return false;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.app.set_status("clipboard is empty".to_owned());
            return true;
        };
        let multiline = surface.is_editor();
        let mut chars = paste_chars(&text, multiline);
        let truncated = chars.len() > MAX;
        chars.truncate(MAX);
        if chars.is_empty() {
            self.app.set_status("nothing to paste".to_owned());
            return true;
        }
        let n = chars.len();
        for c in chars {
            if c == '\n' {
                self.app
                    .on_key(&mut self.shell, "enter", false, false, None);
            } else {
                self.app
                    .on_key(&mut self.shell, &c.to_string(), false, false, Some(c));
            }
        }
        self.app.set_status(if truncated {
            format!("pasted the first {n} character(s) — the rest was too long")
        } else {
            format!("pasted {n} character(s)")
        });
        true
    }

    /// File a picture from the clipboard into the vault and link it at
    /// the cursor. `true` when the clipboard held one.
    ///
    /// The window is the only place that can reach the system
    /// clipboard, and the kernel is the only place that knows where the
    /// vault keeps its assets — so this is the seam, and it is thin:
    /// bytes and an extension go one way, the link comes back.
    fn paste_clipboard_image(&mut self, cx: &Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let Some(image) = item.entries().iter().find_map(|entry| match entry {
            gpui::ClipboardEntry::Image(image) => Some(image),
            gpui::ClipboardEntry::String(_) => None,
        }) else {
            return false;
        };
        let extension = match image.format {
            gpui::ImageFormat::Png => "png",
            gpui::ImageFormat::Jpeg => "jpg",
            gpui::ImageFormat::Webp => "webp",
            gpui::ImageFormat::Gif => "gif",
            gpui::ImageFormat::Svg => "svg",
            gpui::ImageFormat::Bmp => "bmp",
            gpui::ImageFormat::Tiff => "tiff",
        };
        self.app
            .paste_image(&self.shell, extension, &image.bytes)
            .is_some()
    }

    /// Copy the body editor's VISUAL selection to the system clipboard
    /// and the vault's kill ring, so it can be pasted either back into
    /// closure or into another application.
    ///
    /// `false` — key not handled — anywhere else, because `C-c` is a
    /// leader prefix in the Emacs keymap (`C-c c` captures) and must
    /// not be swallowed.
    fn copy_selection(&mut self, cx: &Context<Self>) -> bool {
        if !self.app.surface().is_editor() {
            return false;
        }
        let Some((a, b)) = self.app.body_selection() else {
            return false;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        // `get` rather than an index: a range that is not on a char
        // boundary yields None instead of a panic (I5).
        let Some(text) = self.app.body_buffer().get(lo..hi).map(ToOwned::to_owned) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let n = text.chars().count();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        self.shell.vault.push_kill_ring(text);
        self.app
            .set_status(format!("copied {n} character(s) to the clipboard"));
        true
    }

    /// Follow an org link target ([`link_action`]).
    ///
    /// Ids and titles resolve inside the vault; a `file:` link selects
    /// the first headline of that file — it used to be refused, which
    /// made an ordinary org cross-reference dead in the reference
    /// shell. A URL closure will not open: launching a browser is the
    /// user's call. It goes to the clipboard instead, which is the one
    /// thing that makes ctrl-clicking it worth anything, and the
    /// status line says so rather than leaving the paste a surprise.
    pub fn follow_link(&mut self, target: &str, cx: &mut Context<Self>) {
        match link_action(target) {
            LinkAction::None => {}
            LinkAction::Block(id) => self.jump_to(&id, target, cx),
            LinkAction::Fuzzy(what) => self.jump_to(&what, target, cx),
            LinkAction::File(path) => self.jump_to_file(&path, None, cx),
            LinkAction::FileAt(path, at) => self.jump_to_file(&path, Some(&at), cx),
            LinkAction::External(url) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                self.app
                    .set_status(format!("{url} — copied to the clipboard"));
                cx.notify();
            }
        }
    }

    /// Select the headline `what` names — by id, then by title.
    fn jump_to(&mut self, what: &str, target: &str, cx: &mut Context<Self>) {
        if self.app.select_by_id(&self.shell, what) || self.app.select_by_title(&self.shell, what) {
            // Leaving the editor is what makes the jump visible.
            self.app.run(&mut self.shell, "open-file");
            self.app.set_status(format!("followed {target}"));
        } else {
            self.app
                .set_status(format!("{target} — not a headline in this vault"));
        }
        cx.notify();
    }

    /// Select the first headline of `path`, or the one `at` names.
    fn jump_to_file(&mut self, path: &str, at: Option<&str>, cx: &mut Context<Self>) {
        // `::*Heading` is org's in-file search; the leading `*` is the
        // headline sigil, not part of the title.
        let title = at.map(|a| a.trim_start_matches('*').trim());
        if self.app.select_in_file(&self.shell, path, title) {
            self.app.run(&mut self.shell, "open-file");
            self.app.set_status(format!("followed file:{path}"));
        } else {
            self.app
                .set_status(format!("file:{path} — not in this vault"));
        }
        cx.notify();
    }

    /// Run a command from a mouse affordance (which-key chip, detail
    /// field, header button) — the same dispatch the chords use (I8).
    fn click(&mut self, command: &str, cx: &mut Context<Self>) {
        self.app.run(&mut self.shell, command);
        if self.app.should_quit() {
            cx.quit();
        }
        cx.notify();
    }

    /// G5: wheel over the body-editor pane scrolls its own viewport
    /// (`body_scroll_by`). The outline needs no equivalent — its
    /// `uniform_list` owns its own scroll state.
    fn on_body_scroll(
        &mut self,
        ev: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dy = match ev.delta {
            ScrollDelta::Lines(l) => l.y,
            ScrollDelta::Pixels(p) => f32::from(p.y) / 20.0,
        };
        #[allow(clippy::cast_possible_truncation)]
        let steps = dy.abs().ceil().min(1000.0) as i32;
        let delta = if dy < 0.0 { steps } else { -steps };
        self.app.body_scroll_by(delta, self.body_view());
        cx.stop_propagation();
        cx.notify();
    }

    /// Bring every keyboard cursor into view, in the pane it belongs
    /// to.
    ///
    /// Each pane remembers what it last scrolled to, so a reveal is
    /// requested only when the cursor actually moved — otherwise the
    /// wheel would be dragged back on every frame.
    ///
    /// Three separate cursors, and they used to share one reveal: the
    /// outline was scrolled to `selected` even on the nine surfaces
    /// where `selected` is a *right-hand pane* list index (the left
    /// column jumped while the row being moved stayed off screen), and
    /// neither the palette nor the side lists were revealed at all — so
    /// arrowing down the palette walked the highlight off the bottom
    /// with nothing following it.
    fn reveal_cursors(&mut self) {
        let surface = self.app.surface();
        let selected = self.app.selected();
        if outline_follows_selection(surface) && selected != self.revealed {
            self.outline_scroll
                .scroll_to_item(selected, gpui::ScrollStrategy::Center);
            self.revealed = selected;
        }
        if surface == ModalSurface::Palette {
            let cursor = self.app.palette_cursor();
            if cursor != self.palette_revealed {
                self.palette_scroll
                    .scroll_to_item(cursor, gpui::ScrollStrategy::Center);
                self.palette_revealed = cursor;
            }
        }
        if let Some(offset) = side_reveal_offset(surface) {
            // Each pane's own cursor, and where its row 0 sits among
            // the pane's children — the sniffer and the conflict
            // resolver have a button row above theirs.
            let cursor = match surface {
                ModalSurface::UndoHistory => self.app.undo_history_cursor(),
                ModalSurface::Sniffer => self.app.sniffer_cursor(),
                ModalSurface::Conflicts => self.app.conflicts().selected(),
                _ => selected,
            };
            if cursor != self.side_revealed {
                self.side_scroll.scroll_to_item(cursor + offset);
                self.side_revealed = cursor;
            }
        }
        // The assistant's newest turn: a long transcript pushed the
        // answer below the fold, and the reply is the whole point of
        // asking. The transcript is one element, so this scrolls the
        // pane to the end rather than to a row — where the newest turn
        // and the question field both are.
        if surface == ModalSurface::Llm {
            let turns = self.app.chat_turns().len();
            if turns != self.chat_seen {
                let max = self.side_scroll.max_offset().height;
                self.side_scroll.set_offset(gpui::point(px(0.0), -max));
                self.chat_seen = turns;
            }
        }
    }

    /// How many body lines the editor shows, from the right-hand pane's
    /// measured height ([`body_viewport_lines`]).
    ///
    /// The pane's bounds are last frame's, which is exactly right: the
    /// count only changes when the window is resized, and the frame
    /// after a resize corrects it. Before the first layout there are no
    /// bounds at all, and the pane assumes [`BODY_VIEW_DEFAULT`].
    #[must_use]
    pub fn body_view(&self) -> usize {
        let height = f32::from(self.side_scroll.bounds().size.height);
        if height <= 0.0 {
            return BODY_VIEW_DEFAULT;
        }
        body_viewport_lines(height, BODY_LINE_H * self.app.zoom(), BODY_CHROME)
    }

    /// How many columns of body text the editor pane can show.
    ///
    /// The gutter and the scrollbar come off the measured width first.
    /// Never zero: an unmeasured pane (no bounds before the first
    /// layout) assumes a usable line rather than scrolling every line
    /// off the left edge.
    fn body_cols(&self) -> usize {
        /// Advance of one monospace glyph at the editor's text size.
        const COL_W: f32 = 7.2;
        /// The line-number gutter plus its margin, and the scrollbar.
        const CHROME: f32 = 34.0 + 8.0 + 10.0;
        /// Below this the pane cannot show a word.
        const MIN: usize = 8;
        let width = f32::from(self.body_track.bounds().size.width) - CHROME;
        // A zoomed glyph is wider, so the pane holds fewer columns —
        // the horizontal scroll has to know that or the cursor runs off
        // the edge it is supposed to be following.
        let col_w = COL_W * self.app.zoom();
        if !width.is_finite() || width < col_w {
            return BODY_COLS_DEFAULT;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cols = (width / col_w).floor() as usize;
        cols.max(MIN)
    }

    /// Context line describing the active surface (with the live input
    /// buffer + caret for the typing surfaces).
    /// The capture overlay's context row: where this thought will be
    /// filed, as breadcrumbs, followed by what is being typed.
    ///
    /// The path is the answer to "which “Notes”?" and the control at
    /// the same time — every crumb is a click, and the click retargets
    /// the capture in place instead of making the user cancel,
    /// re-select and type the line again. The step it will file into
    /// is the filled chip; the rest are quiet until hovered.
    ///
    /// A deep path collapses in the middle rather than pushing the
    /// typed line off the row: the file and the last three steps stay,
    /// and the hidden ones are named in the ellipsis' tooltip. The
    /// ends are what identify a path; the middle is what a person
    /// skips reading anyway.
    fn capture_bar(&self, co: Colors, zoom: f32, cx: &Context<Self>) -> gpui::Div {
        let crumbs = self.app.capture_crumbs(&self.shell);
        let (shown, hidden) = crumb_slots(&crumbs);
        let sep = |text: &'static str| {
            div()
                .flex_none()
                .px(px(2.0))
                .text_color(rgb(co.border))
                .child(text)
        };
        let mut row = div()
            .debug_selector(|| "capture-bar".to_owned())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(1.0))
            .child(
                div()
                    .flex_none()
                    .pr_1()
                    .text_color(rgb(co.success))
                    .child("＋"),
            );
        for (n, slot) in shown.into_iter().enumerate() {
            if n > 0 {
                row = row.child(sep("›"));
            }
            let Some(i) = slot else {
                let names = hidden.join("  ›  ");
                row = row.child(
                    div()
                        .id("capture-crumb-gap")
                        .flex_none()
                        .px_1()
                        .rounded_md()
                        .text_color(rgb(co.muted))
                        .child("…")
                        .tooltip(move |_w, cx| {
                            let names = names.clone();
                            cx.new(move |_| Hint {
                                text: names,
                                co,
                                zoom,
                            })
                            .into()
                        }),
                );
                continue;
            };
            let crumb = &crumbs[i];
            // The file is not a headline and should not read like one.
            let idle = if crumb.id.is_none() { co.muted } else { co.fg };
            // A headline can be a sentence. Left whole, one crumb
            // pushes the line being typed off its own row — so the
            // chip is trimmed and the tooltip keeps the whole of it.
            let hint = if crumb.active {
                format!("filing here: {}", crumb.label)
            } else if crumb.id.is_none() {
                format!("file it at the top of {}", crumb.label)
            } else {
                format!("file it under “{}”", crumb.label)
            };
            let mut chip = div()
                .id(gpui::SharedString::from(format!("capture-crumb-{i}")))
                .flex_none()
                .px_2()
                .rounded_md()
                .text_color(rgb(if crumb.active { co.bg } else { idle }))
                .child(gpui::SharedString::from(elide(&crumb.label, 28)))
                .tooltip(move |_w, cx| {
                    let hint = hint.clone();
                    cx.new(move |_| Hint {
                        text: hint,
                        co,
                        zoom,
                    })
                    .into()
                });
            if crumb.active {
                chip = chip.bg(rgb(co.accent)).font_weight(gpui::FontWeight::BOLD);
            } else {
                chip = chip
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.app.pick_capture_crumb(&this.shell, i);
                            cx.notify();
                        }),
                    );
            }
            row = row.child(chip);
        }
        row.child(sep("·"))
            .child(div().flex_grow().text_color(rgb(co.fg)).child(caret_text(
                co,
                self.app.capture_buffer(),
                self.app.capture_cursor(),
            )))
    }

    /// The row under the header: breadcrumbs while capturing, a live
    /// field while a prompt is open, one line of text otherwise.
    fn context_row(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let row = div()
            .debug_selector(|| "context-line".to_owned())
            .px_3()
            .py_1()
            .bg(rgb(co.panel))
            .text_color(rgb(co.fg))
            .text_size(self.sz(12.0));
        // Capture draws its target as clickable breadcrumbs; every
        // other surface has one line to say and says it.
        if self.app.surface() == ModalSurface::Capture {
            row.child(self.capture_bar(co, self.app.zoom(), cx))
        } else if let Some(prompt) = self.prompt_row(co) {
            row.child(prompt)
        } else {
            row.child(self.context_line())
        }
    }

    /// The prompt surfaces paint a label and a live field with a block
    /// caret in it; everything else has one line to say and says it
    /// ([`Self::context_line`]).
    fn prompt_row(&self, co: Colors) -> Option<gpui::Div> {
        let (label, text, cursor) = match self.app.surface() {
            ModalSurface::Rename => (
                "\u{270e} rename: ",
                self.app.field_buffer(),
                self.app.field_cursor(),
            ),
            ModalSurface::AddSibling => (
                "\u{ff0b} add: ",
                self.app.field_buffer(),
                self.app.field_cursor(),
            ),
            ModalSurface::TagsEdit => (
                "\u{270e} tags: ",
                self.app.field_buffer(),
                self.app.field_cursor(),
            ),
            ModalSurface::PropertyEdit => (
                "\u{270e} prop: ",
                self.app.field_buffer(),
                self.app.field_cursor(),
            ),
            _ => return None,
        };
        Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(div().flex_none().child(label))
                .child(div().flex_grow().child(caret_text(co, text, cursor))),
        )
    }

    /// The string form of the context row.
    ///
    /// The prompt surfaces are painted as elements instead
    /// ([`Self::prompt_row`], [`Self::capture_bar`]) because their
    /// caret is a *mark* over a cell rather than a character in the
    /// text; what is returned here for them is the same line without
    /// one, for anything reading the context row as a string.
    fn context_line(&self) -> String {
        let n = self.app.rows(&self.shell).len();
        match self.app.surface() {
            ModalSurface::Browse => format!("{n} headline(s)"),
            ModalSurface::Search => self.app.search_context(&self.shell),
            // The capture surface draws breadcrumbs instead
            // ([`Self::capture_bar`]); this is the text fallback for
            // anything reading the context line as a string.
            ModalSurface::Capture => format!(
                "＋ capture {} : {}",
                self.app.capture_target_label(&self.shell),
                self.app.capture_buffer()
            ),
            ModalSurface::Rename => format!("✎ rename: {}", self.app.field_buffer()),
            ModalSurface::AddSibling => format!("＋ add: {}", self.app.field_buffer()),
            ModalSurface::TagsEdit => format!("✎ tags: {}", self.app.field_buffer()),
            ModalSurface::PropertyEdit => format!("✎ prop: {}", self.app.field_buffer()),
            ModalSurface::Palette => format!("❯ {}", self.app.field_buffer()),
            // A full-window buffer names itself, the way a modeline
            // does: which headline this is, and which file it came from.
            ModalSurface::EditBody | ModalSurface::EditBlock | ModalSurface::EditFile => self
                .app
                .buffer_name(&self.shell)
                .map_or_else(|| "✎ body".to_owned(), |name| format!("✎ {name}")),
            ModalSurface::DatePick => {
                let grid = self.app.date_grid();
                format!("{} — {}", grid.field, grid.selected)
            }
            ModalSurface::Refile => format!("refile to — {}▏", self.app.query()),
            ModalSurface::TagPick => {
                format!("tags — {}▏ · SPC toggles · RET writes", self.app.query())
            }
            ModalSurface::Buffers => format!(
                "buffers — {}▏ · {} open · RET opens · Esc back",
                self.app.query(),
                self.app.buffer_rows(&self.shell).len()
            ),
            ModalSurface::Files => format!(
                "files — {}▏ · {} in this vault · RET opens · Esc back",
                self.app.query(),
                self.app.file_rows(&self.shell).len()
            ),
            ModalSurface::Backlinks => "backlinks — Esc back".to_owned(),
            ModalSurface::Agenda => "agenda — RET jump, Esc back".to_owned(),
            ModalSurface::Blocks => "src blocks — RET jump, Esc back".to_owned(),
            ModalSurface::UndoHistory => {
                "undo history — j/k move · RET/click jump · Esc back".to_owned()
            }
            ModalSurface::Headlines => {
                format!("headlines — {n} in this file · Esc back")
            }
            ModalSurface::DbView => format!(
                "database — {} row(s) · Esc back",
                self.app.db_rows(&self.shell).1.len()
            ),
            ModalSurface::BodySearch => format!(
                "⌕ body: {}▏ — {} hit(s)",
                self.app.query(),
                self.app.body_search_rows(&self.shell).len()
            ),
            ModalSurface::Sniffer => format!(
                "flows — {} captured · a allow · b block · Esc back",
                self.app.sniffer().events().len()
            ),
            ModalSurface::Conflicts => format!(
                "conflicts — {} pending · o ours · t theirs · Esc back",
                self.app.conflicts().conflicts().len()
            ),
            ModalSurface::Llm => {
                let s = self.app.llm_config_status(&self.shell);
                format!("assistant — {} · Enter sends · Esc back", s.detail)
            }
            ModalSurface::Graph => format!(
                "graph — {} hub(s), {} orphan(s), {} dead link(s) · Esc back",
                self.app.hub_rows(&self.shell).len(),
                self.app.orphan_rows(&self.shell).len(),
                self.app.dead_link_rows(&self.shell).len()
            ),
            ModalSurface::Journal => format!(
                "journal — {} recorded command(s) · Esc back",
                self.app.journal_rows(&self.shell).len()
            ),
            ModalSurface::Cron => format!(
                "scheduled jobs — {} declared in this vault · Esc back",
                self.app.cron_rows(&self.shell).len()
            ),
            ModalSurface::Sync => format!(
                "sync — {} peer(s) · Enter adds a pasted ticket · Esc back",
                self.app.sync().map_or(0, |s| s.peers().len())
            ),
            ModalSurface::Ex => format!(
                ":{}▏  — :w :q :wq :x, or any command name",
                self.app.ex_buffer()
            ),
        }
    }

    /// The left outline list (Browse/Search and the typing surfaces
    /// that keep the tree visible).
    fn rows_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let count = self.app.rows_shared(&self.shell).len();
        if count == 0 {
            // An empty outline is either a new vault or a search that
            // matched nothing, and the two need different sentences.
            // Either way a blank column teaches nothing.
            return self.outline_empty_state(co, cx);
        }
        let list = gpui::uniform_list(
            "outline",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _w, cx| {
                // The theme lives on the view, so the colours are
                // re-derived here rather than captured — the closure
                // outlives this frame.
                let co = Colors::of(&this.theme);
                range
                    .map(|i| this.outline_row(co, i, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.outline_scroll.clone())
        .flex_grow();
        div()
            .flex()
            .flex_row()
            .w(px(self.outline_w))
            .min_w(px(OUTLINE_W_MIN))
            .border_r_1()
            .border_color(rgb(co.border))
            .child(list)
            .child(scrollbar(
                "outline-scrollbar",
                co,
                &self.outline_scroll.0.borrow().base_handle.clone(),
                cx,
            ))
            // The column was a fixed 420px, so a vault of long titles
            // had no way to show them and a narrow window no way to get
            // out of their way. This is the grab handle on its edge.
            .child(
                div()
                    .debug_selector(|| "outline-resize".to_owned())
                    .w(px(6.0))
                    .h_full()
                    .cursor_col_resize()
                    .hover(move |s| s.bg(rgb(co.accent)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, ev: &gpui::MouseDownEvent, _w, cx| {
                            this.outline_drag = Some(f32::from(ev.position.x) - this.outline_w);
                            cx.notify();
                        }),
                    ),
            )
    }

    /// What the outline column says when it has no rows.
    ///
    /// Two different situations wearing the same blank column: a vault
    /// with nothing in it yet, and a search that matched nothing. The
    /// first wants to tell you how to start, the second how to get
    /// back — so it says which one it is, and names the chord.
    fn outline_empty_state(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let searching = self.app.surface() == ModalSurface::Search;
        let (headline, hint, command) = if searching {
            (
                "No matches",
                "Nothing in the vault matches that. Esc clears the search.",
                None,
            )
        } else {
            (
                "This vault is empty",
                "Capture your first note, or point closure at a directory of .org files.",
                Some("capture-start"),
            )
        };
        let mut column = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .w(px(420.0))
            .min_w(px(300.0))
            .px_6()
            .border_r_1()
            .border_color(rgb(co.border))
            .child(
                div()
                    .text_color(rgb(co.fg))
                    .text_size(self.sz(15.0))
                    .child(headline),
            )
            .child(
                div()
                    .text_color(rgb(co.muted))
                    .text_size(self.sz(12.0))
                    .child(hint),
            );
        if let Some(command) = command {
            let chord = self.app.chord_for(command).unwrap_or_default();
            column = column.child(
                div()
                    .mt_2()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(co.panel))
                    .text_size(self.sz(12.0))
                    .text_color(rgb(co.success))
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.click(command, cx);
                        }),
                    )
                    .child(format!("＋ capture a note   {chord}")),
            );
        }
        column
    }

    /// One outline row: the frame, the selection styling and the
    /// mouse gestures (select, drag-to-reorder, right-click menu); the
    /// cells inside come from [`Self::outline_cells`].
    fn outline_row(&self, co: Colors, i: usize, cx: &Context<Self>) -> gpui::Div {
        let rows = self.app.rows_shared(&self.shell);
        let Some(row) = rows.get(i).cloned() else {
            return div();
        };
        // Escape drops the selection so a capture files loose; the row
        // has to stop looking selected when it does, or the outline is
        // claiming something the next capture will contradict.
        let is_sel = i == self.app.selected() && self.app.selection_active();
        let mut line = div()
            // A named element a test can find the painted bounds of, so
            // a click lands where the user's would rather than at a
            // coordinate the test made up.
            .debug_selector(|| format!("outline-row-{i}"))
            .flex()
            .items_center()
            // The row is the click target — selecting it, right-clicking
            // it, dragging it. Sized to its cells it was 193px wide in
            // a 420px column, so the right half of every row in the
            // outline was dead space that looked exactly like the row.
            .w_full()
            .overflow_hidden()
            .px_2()
            .py_1()
            .text_size(self.sz(OUTLINE_TEXT))
            .cursor_pointer()
            // The selection marker is on every row, transparent on the
            // ones that are not selected. Added only to the selected
            // row, its 2px pushed that row's whole content right — so
            // moving the cursor down the list nudged each title
            // sideways as it arrived and back as it left.
            .border_l_2()
            .border_color(if is_sel {
                rgb(co.accent).into()
            } else {
                gpui::transparent_black()
            })
            .bg(rgb(if is_sel { co.selection } else { co.bg }))
            .hover(move |s| s.bg(rgb(if is_sel { co.selection } else { co.hover })))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev, _w, cx| {
                    this.app.select(i, &this.shell);
                    // G3: a held press starts a potential row drag.
                    this.drag.begin(i);
                    this.menu = None;
                    cx.notify();
                }),
            )
            // Right-click selects the row and opens its context menu.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _w, cx| {
                    this.app.select(i, &this.shell);
                    this.menu = Some((ev.position, closure_shell_core::ContextTarget::Row));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            // G3: dragging across rows retargets the drop slot…
            .on_mouse_move(cx.listener(move |this, ev: &gpui::MouseMoveEvent, _w, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.drag.over(i);
                    cx.notify();
                }
            }))
            // …and release completes it as registry moves (I8).
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev, _w, cx| {
                    if let Some((f, t)) = this.drag.drop()
                        && f != t
                    {
                        this.app.drag_drop_rows(&mut this.shell, f, t);
                    }
                    cx.notify();
                }),
            );
        // The drop target under a live drag reads as an insertion line.
        if self.drag.target() == Some(i) && self.drag.source() != Some(i) {
            line = line.border_b_2().border_color(rgb(co.warning));
        }
        Self::outline_cells(line, co, self.app.zoom(), i, &row, cx)
    }

    /// The cells of an outline row: indent, fold arrow, status glyph,
    /// TODO chip, title and file. Each is its own click target running
    /// the same registry command its chord does (I8).
    fn outline_cells(
        line: gpui::Div,
        co: Colors,
        zoom: f32,
        i: usize,
        row: &Row,
        cx: &Context<Self>,
    ) -> gpui::Div {
        // The fold state rides on the row: asking the vault per row per
        // frame is the same answer at wheel speed.
        let folded = row.folded;
        let indent = f32::from(row.level.saturating_sub(1)) * 14.0;
        let (todo_col, glyph) = match row.todo.as_deref() {
            Some("DONE" | "CANCELLED" | "KILL") => (co.success, "●"),
            Some(_) => (co.error, "○"),
            None => (co.muted, "·"),
        };
        // Both the fold arrow and the status glyph select their row
        // first, so a click acts on what it points at.
        let act = |command: &'static str| {
            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                this.app.select(i, &this.shell);
                this.app.run(&mut this.shell, command);
                cx.notify();
            })
        };
        // Every fixed cell says `flex_none`. A flex item shrinks by
        // default, so without it the indent, the arrow and the glyph
        // gave up width to a long title — the columns of a row moved
        // with the length of its headline.
        let mut line = line
            .child(div().w(px(indent)).flex_none())
            // The arrow is there only when there is a subtree under it.
            // Painted on every row, a leaf offered an affordance that
            // does nothing when clicked — which is most of what "the
            // collapse isn't working" looks like from the outside. The
            // column stays either way, so the titles still line up.
            .child(if row.has_children {
                div()
                    .debug_selector(|| format!("fold-{i}"))
                    .w(px(18.0))
                    .flex_none()
                    .text_color(rgb(if folded { co.accent } else { co.muted }))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, act("toggle-fold"))
                    .child(if folded { "▸" } else { "▾" })
            } else {
                div().w(px(18.0)).flex_none()
            })
            .child(
                div()
                    .debug_selector(|| format!("todo-{i}"))
                    .w(px(18.0))
                    .flex_none()
                    .text_color(rgb(todo_col))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, act("toggle-todo"))
                    .child(glyph),
            );
        // The keyword column is always there, whether or not this row
        // has a keyword. Painted only when there is one, the titles of
        // a mixed list started at two different x positions and the
        // whole column appeared to shuffle as you moved through it.
        let keyword = div().w(px(TODO_COL_W)).mr_1().flex_none();
        line = line.child(if let Some(todo) = &row.todo {
            keyword
                .px_1()
                .rounded_sm()
                .text_color(rgb(todo_col))
                .text_size(sz_at(11.0, zoom))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .on_mouse_down(MouseButton::Left, act("toggle-todo"))
                .child(todo.clone())
        } else {
            keyword
        });
        line
            // The title takes the slack and is clipped by it. Left to
            // size itself, a long title pushed the file name off the
            // end and a short one let it slide back — which is the
            // "juggling" as the selection moved down the list.
            .child(
                div()
                    .debug_selector(move || format!("title-{i}"))
                    // `flex_1`, not `flex_grow`: basis zero. Sized from
                    // its content the title overflowed the row, and the
                    // overflow was taken out of *every* shrinkable cell
                    // beside it — so the indent, the fold arrow and the
                    // glyph all moved left by an amount that depended on
                    // how long the headline was.
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(rgb(co.outline(row.level)))
                    .child(row.title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .ml_2()
                    .max_w(px(PATH_COL_W))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(rgb(co.muted))
                    .text_size(sz_at(10.0, zoom))
                    .child(short_path(&row.path)),
            )
    }

    /// Right-hand pane: detail (clickable fields), palette, a list
    /// surface, or the body editor — driven by the active surface.
    fn side_pane(&self, co: Colors, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            // `flex_1` rather than `flex_grow`: the difference is the
            // basis. Growing from a basis of *auto* makes this pane's
            // base size the width of what is in it, so a long headline
            // made the row's total wider than the window and the
            // outline column paid the difference out of its own width —
            // the tree jumped wider and narrower as the selection moved
            // between short and long titles. From a basis of zero the
            // pane takes what is left over and nothing else decides it.
            // The matching `min_w` kills the automatic minimum size,
            // which is content-sized for the same reason the body row
            // needed `min_h` ([`Render::render`]).
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(self.side_content(co, cx))
            .child(scrollbar(
                "side-scrollbar",
                co,
                &self.side_scroll.clone(),
                cx,
            ))
    }

    /// What the right-hand pane actually shows for the active surface.
    ///
    /// This *is* the scrolling element: the rows of a list surface are
    /// its direct children, which is what lets the pane's scroll handle
    /// address a row by index and bring the keyboard cursor into view
    /// ([`side_reveals_selection`]). Wrapping them in an inner div, as
    /// it used to, left the handle with a single child and nothing to
    /// scroll to.
    fn side_content(&self, co: Colors, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let pane = div()
            .id("side")
            .flex()
            .flex_col()
            .flex_grow()
            // This is the element whose bounds [`Self::body_view`]
            // measures, so how tall it is decides how much of a body
            // the editor paints. It is kept honest by the `min_h` on
            // the row in [`Render::render`], not by anything here.
            .px_4()
            .py_3()
            .gap_2()
            .overflow_y_scroll()
            .track_scroll(&self.side_scroll)
            .bg(rgb(co.bg));
        match self.app.surface_beneath() {
            ModalSurface::Agenda => pane.child(self.agenda_pane(co, cx)),
            ModalSurface::Headlines => pane.children(
                self.id_rows(
                    co,
                    self.app
                        .headline_rows(&self.shell)
                        .into_iter()
                        .map(|(title, id)| (format!("{title}    [{id}]"), id))
                        .collect(),
                    cx,
                ),
            ),
            ModalSurface::DbView => pane.child(self.db_table(co)),
            ModalSurface::BodySearch => pane.children(
                self.id_rows(
                    co,
                    self.app
                        .body_search_rows(&self.shell)
                        .into_iter()
                        .map(|(id, text)| (text, id))
                        .collect(),
                    cx,
                ),
            ),
            ModalSurface::DatePick => pane.child(self.date_pane(co, cx)),
            ModalSurface::Refile => pane.children(self.refile_pane(co, cx)),
            ModalSurface::TagPick => pane.children(self.tag_pane(co, cx)),
            ModalSurface::Buffers => pane.children(self.buffer_pane(co, cx)),
            ModalSurface::Files => pane.children(self.file_pane(co, cx)),
            ModalSurface::Sync => pane.child(self.sync_pane(co, cx)),
            ModalSurface::Llm => pane.child(self.llm_pane(co, cx)),
            ModalSurface::Graph => pane.child(self.graph_pane(co, cx)),
            ModalSurface::Journal => pane.children(Self::text_rows(
                co,
                self.app.zoom(),
                self.app.journal_rows(&self.shell),
                "no commands recorded yet — the journal fills as you edit",
            )),
            ModalSurface::Cron => pane.children(Self::text_rows(
                co,
                self.app.zoom(),
                self.app
                    .cron_rows(&self.shell)
                    .into_iter()
                    .map(|(spec, command)| format!("{spec}   {command}"))
                    .collect(),
                "no scheduled jobs — declare them in the vault with a :CRON: property",
            )),
            ModalSurface::Sniffer => pane.child(self.sniffer_pane(co, cx)),
            ModalSurface::Conflicts => pane.child(self.conflicts_pane(co, cx)),
            ModalSurface::UndoHistory => pane.children(self.undo_history_pane(co, cx)),
            ModalSurface::Blocks => pane.child(self.blocks_pane(co, cx)),
            ModalSurface::Backlinks => pane.children(
                self.app
                    .backlink_rows(&self.shell)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (_id, title))| {
                        list_row(
                            co,
                            self.app.zoom(),
                            i == self.app.selected(),
                            format!("⟵ {title}"),
                            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                this.app.backlink_click(&this.shell, i);
                                cx.notify();
                            }),
                        )
                    }),
            ),
            // org-edit-special is the same editor with one block in it,
            // so it paints the same way; the header says which.
            ModalSurface::EditBody | ModalSurface::EditBlock | ModalSurface::EditFile => {
                pane.child(self.editor_pane(co, cx))
            }
            _ => self.detail_pane(pane, co, cx),
        }
    }

    /// One painted body line: a single `StyledText` carrying both the
    /// syntax colours and the cursor/selection background, wrapped in
    /// a div whose mouse handlers hit-test through gpui's own text
    /// layout.
    ///
    /// This replaced a per-character element grid (one `div` per
    /// glyph, so a 40-line viewport of 80-column text cost ~3200
    /// elements per frame; it costs at most 42 here). Click precision
    /// is unchanged: `TextLayout::index_for_position` resolves a
    /// window position to a byte offset *inside* the glyph run, and
    /// [`col_for_byte`] maps that back to the char column
    /// [`closure_shell_core::BodyEditor`] addresses.
    ///
    /// The cursor is a background block in NORMAL/VISUAL and a thin
    /// bar in INSERT; the bar sits between glyphs, so the INSERT
    /// cursor line is the one case painted as two halves around it
    /// ([`split_runs`]), each hit-testing against its own layout.
    fn editor_line(
        &self,
        co: Colors,
        geom: LineGeom,
        spans: &[(BodySpan, String)],
        cx: &Context<Self>,
    ) -> gpui::Div {
        use closure_shell_core::EditorMode;
        let LineGeom {
            ln,
            line_start,
            line_len,
            h_start,
            cols,
        } = geom;
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        let (cur_line, cur_col) = self.app.body_cursor();
        let insert = self.app.body_mode() == EditorMode::Insert;
        // Unwrapped, a logical line is one row and owns its cursor.
        // Wrapped, only the row the column actually falls in does —
        // otherwise every row of a paragraph would draw one.
        let on_cursor_line =
            ln == cur_line && cols.is_none_or(|n| (h_start..h_start + n + 1).contains(&cur_col));
        // Search hits first, so the cursor and the selection are drawn
        // over one they happen to sit on. Marking them at all is new:
        // `/` moved the cursor to a match and left every match on
        // screen looking like ordinary prose.
        let mut marks: Vec<(std::ops::Range<usize>, Emphasis)> = self
            .app
            .body_search_pattern()
            .map(|pattern| {
                line_matches(&text, &pattern)
                    .into_iter()
                    .map(|r| (r, Emphasis::Search))
                    .collect()
            })
            .unwrap_or_default();
        // A selection and a cursor are both a background range, and
        // they must not look alike: the selection tint is the same
        // colour the outline uses for its selected row, which left the
        // block cursor all but invisible. The cursor inverts instead —
        // background-coloured glyphs on foreground — the way every
        // terminal draws one.
        if let Some(sel) = self.app.body_selection() {
            marks.extend(
                selection_in_line(line_start, line_len, sel).map(|r| (r, Emphasis::Selection)),
            );
        }
        // The cursor is marked *last*, so it wins wherever it overlaps
        // ([`styled_runs`]). VISUAL used to suppress it entirely, which
        // is the one mode where knowing which end you are moving matters
        // most. The line is padded with a space when the cursor sits
        // past its last glyph, so there is always a cell to invert.
        let mut spans = spans.to_vec();
        let mut text = text;
        if on_cursor_line && let Some(mark) = cursor_mark(&text, cur_col, insert) {
            let (padded, _) = cursor_cell(&text, cur_col);
            if padded.len() > text.len() {
                spans.push((BodySpan::Plain, " ".to_owned()));
                text = padded;
            }
            marks.push(mark);
        }
        let runs = styled_runs(&spans, &marks);
        // Lines do not wrap: wrapping desyncs the one-number gutter,
        // the fixed row height and the arithmetic that turns pane
        // height into a line count. A long line scrolls sideways with
        // the cursor instead, and the runs are rebased with it.
        let shift = byte_for_col(&text, h_start);
        let (_, runs) = split_runs(&runs, shift);
        let text = text.get(shift..).unwrap_or_default().to_owned();
        // A wrapped row also *ends* somewhere: the rest of the logical
        // line belongs to the rows below it.
        let (text, runs) = match cols {
            None => (text, runs),
            Some(n) => {
                let end = byte_for_col(&text, n);
                let (head, _) = split_runs(&runs, end);
                (text.get(..end).unwrap_or(&text).to_owned(), head)
            }
        };
        let cur_col = cur_col.saturating_sub(h_start);
        let mut row = div()
            .flex()
            .flex_grow()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_text();
        if on_cursor_line && insert {
            let at = byte_for_col(&text, cur_col);
            let (head, tail) = split_runs(&runs, at);
            row = row
                .child(editor_segment(
                    co,
                    ln,
                    h_start,
                    text[..at].to_owned(),
                    head,
                    cx,
                ))
                // INSERT draws a bar between the glyphs, in the accent
                // colour. Its height is the row's — a flex child with no
                // height of its own stretches — rather than a hardcoded
                // 18px that is only right at one font size.
                .child(div().w(px(2.0)).bg(rgb(co.accent)))
                .child(editor_segment(
                    co,
                    ln,
                    h_start + cur_col,
                    text[at..].to_owned(),
                    tail,
                    cx,
                ));
        } else {
            // The cursor — block or none — is already in the runs, over
            // a real cell, so the whole line is one laid-out segment.
            row = row.child(editor_segment(co, ln, h_start, text, runs, cx));
        }
        row
    }

    /// The org-edit-special editor pane: syntax-highlighted lines
    /// ([`highlight_body`]), a real caret at the editor cursor, the
    /// vim mode chip (doom spaceline colours: INSERT green / NORMAL
    /// blue), and the C-n completion popup.
    /// The editor pane's status row: the mode chip, the modified dot,
    /// the recording register, the chord in flight and the mode's hint.
    fn editor_header(&self, co: Colors, mode_col: u32, cx: &Context<Self>) -> gpui::Div {
        use closure_shell_core::EditorMode;
        let mode = self.app.body_mode();
        // `R` is INSERT to the core, but it overwrites rather than
        // pushes right, and a chip that hides that is a lie.
        let mode_txt = match mode {
            EditorMode::Insert if self.app.body_replacing() => "REPLACE",
            EditorMode::Insert => "INSERT",
            EditorMode::Normal => "NORMAL",
            EditorMode::Visual => "VISUAL",
            EditorMode::VisualLine => "V·LINE",
        };
        let chip = |text: String, bg: u32| {
            div()
                .px_1()
                .rounded_sm()
                .bg(rgb(bg))
                .text_color(rgb(co.bg))
                .text_size(self.sz(11.0))
                .child(text)
        };
        let mut header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(chip(mode_txt.to_owned(), mode_col).px_2());
        // Modified-and-unwritten, the way every editor marks it. The
        // shell had no dirty state at all, so an unsaved buffer looked
        // exactly like a saved one right up until it was lost.
        if self.app.body_dirty() {
            header = header.child(chip("●".to_owned(), co.warning));
        }
        // A macro under the needle, the way vim's `recording @q` says
        // it: without this the editor looks idle while every stroke is
        // being taped.
        if let Some(reg) = self.app.body_recording() {
            header = header.child(chip(format!("● @{reg}"), co.error));
        }
        // The chord in progress, echoed the way vim's showcmd does — so
        // a half-typed `2d3i` is visible rather than a silent editor.
        // An open `/` search line comes through the same field.
        let pending = self.app.body_pending_chord();
        if !pending.is_empty() {
            header = header.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(mode_col))
                    .text_color(rgb(mode_col))
                    .text_size(self.sz(11.0))
                    .child(pending),
            );
        }
        header = header.child(
            div()
                .text_color(rgb(co.muted))
                .text_size(self.sz(11.0))
                .child(closure_shell_core::editor_hint(mode)),
        );
        // Saving and discarding were chords and nothing else: `C-Enter`
        // if you knew, and an Esc that used to throw the buffer away if
        // you did not. Both are buttons now, and both say their chord.
        let button = |label: &'static str, colour: u32, chord: &'static str| {
            div()
                .px_2()
                .rounded_md()
                .bg(rgb(co.panel))
                .border_1()
                .border_color(rgb(colour))
                .text_color(rgb(colour))
                .text_size(self.sz(11.0))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .child(format!("{label}  {chord}"))
        };
        header
            .child(button("✓ save", co.success, "C-⏎").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _ev, _w, cx| {
                    this.app.commit_edit_body(&mut this.shell);
                    cx.notify();
                }),
            ))
            .child(button("✕ discard", co.error, ":q!").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _ev, _w, cx| {
                    this.app.run_ex_line(&mut this.shell, "q!");
                    cx.notify();
                }),
            ))
    }

    /// How many body lines the last painted frame used — what
    /// [`Self::body_view`] measured when that frame was built.
    #[must_use]
    pub const fn painted_view(&self) -> usize {
        self.painted_view.get()
    }

    /// The column windows one logical line is painted in: exactly one
    /// when clipping (from the horizontal scroll offset to the end of
    /// the line), one per wrapped row when wrapping.
    fn row_windows_for(
        text: &str,
        h_start: usize,
        wrap_cols: Option<usize>,
    ) -> Vec<(usize, Option<usize>)> {
        let Some(cols) = wrap_cols else {
            return vec![(h_start, None)];
        };
        let mut at = 0usize;
        closure_shell_core::wrap_body(text, cols)
            .into_iter()
            .map(|row| {
                let width = text[row.start..row.end].chars().count();
                let window = (at, Some(width));
                at += width;
                window
            })
            .collect()
    }

    fn editor_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        use closure_shell_core::EditorMode;
        let view = self.body_view();
        let scroll_start = self.app.body_scroll_start(view);
        let (cur_line, cur_col) = self.app.body_cursor();
        // Lines are clipped rather than wrapped, so a cursor past the
        // right edge pulls the whole pane sideways with it.
        let h_start = h_scroll_start(cur_col, self.body_cols());
        // doom spaceline colours: insert green, normal blue, visual grey-violet.
        let mode_col = match self.app.body_mode() {
            EditorMode::Insert if self.app.body_replacing() => co.warning,
            EditorMode::Insert => co.success,
            EditorMode::Normal => co.accent,
            EditorMode::Visual => co.heading3,
            EditorMode::VisualLine => co.heading2,
        };
        let header = self.editor_header(co, mode_col, cx);
        let mut body = with_menu(
            div()
                .id("body-text")
                .flex()
                .flex_col()
                .flex_grow()
                .p_2()
                .bg(rgb(co.panel))
                .rounded_md()
                .text_size(self.sz(BODY_TEXT))
                // Not a scroll: the editor paints only the visible
                // lines, so this handle never has anything to move.
                // It is here to record the text's bounds, which is
                // where [`Self::body_scrollbar`] puts its track.
                .overflow_y_scroll()
                .track_scroll(&self.body_track)
                .on_scroll_wheel(cx.listener(Self::on_body_scroll)),
            closure_shell_core::ContextTarget::Body,
            cx,
        );
        // `wrap = true` in config.org: a long line becomes several rows
        // instead of scrolling sideways. The two are alternatives — a
        // wrapped row has no horizontal offset to have — so the rows
        // are cut by [`closure_shell_core::wrap_body`], which hands back
        // an exact byte partition, and each one is painted through the
        // same path with its own column window.
        let wrap_cols = self.wrap.then(|| self.body_cols());
        let hidden = self.app.body_hidden_lines();
        let mut line_start = 0usize;
        for (ln, spans) in self.highlighted(self.app.body_buffer()).iter().enumerate() {
            let line_len: usize = spans.iter().map(|(_, s)| s.len()).sum();
            // G5: only the wheel-scrolled window of lines is painted;
            // byte offsets still accumulate for the skipped lines.
            if !(scroll_start..scroll_start + view).contains(&ln) {
                line_start += line_len + 1;
                continue;
            }
            // Folded lines are painted by nobody: the kernel decides
            // which ones, so every shell hides the same text.
            if hidden.binary_search(&ln).is_ok() {
                line_start += line_len + 1;
                continue;
            }
            let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
            for (i, (start_col, width)) in Self::row_windows_for(&text, h_start, wrap_cols)
                .into_iter()
                .enumerate()
            {
                // L5: line-number gutter, current line accented. A
                // continuation row carries no number — the gutter says
                // which *logical* line this is, once.
                let gutter = div().w(px(34.0)).mr_2();
                let gutter = if i == 0 {
                    gutter
                        .text_size(self.sz(11.0))
                        .text_color(rgb(if ln == cur_line { co.accent } else { co.muted }))
                        .child(format!("{:>3}", ln + 1))
                } else {
                    gutter
                };
                let mut row = div()
                    .flex()
                    .min_h(px(BODY_LINE_H * self.app.zoom()))
                    .child(gutter);
                row = row.child(self.editor_line(
                    co,
                    LineGeom {
                        ln,
                        line_start,
                        line_len,
                        h_start: start_col,
                        cols: width,
                    },
                    spans,
                    cx,
                ));
                if ln == cur_line {
                    row = row.bg(rgb(mix_u32(co.panel, co.selection, 96)));
                }
                body = body.child(row);
            }
            line_start += line_len + 1;
        }
        // The editor virtualizes its own lines, so the pane it sits in
        // never overflows and the shared scrollbar had nothing to
        // measure: a 500-line body scrolled by wheel with no
        // indication of where in it you were. This bar reads the
        // editor's own scroll state instead.
        let lines = self.app.body_buffer().split('\n').count();
        let pane = div()
            .flex()
            .flex_col()
            .flex_grow()
            // [`BODY_LINE_H`] is the line height the viewport count is
            // computed from, and the glyphs are a little taller than
            // that — so the editor asks for a few more lines than it
            // has room for and the column runs off the bottom of the
            // window. Clipped here rather than trusting the estimate:
            // the scrollbar beside it is `h_full`, so an overflowing
            // column put the bottom of its own track past the window
            // edge, where no drag could reach it.
            .min_h(px(0.0))
            .overflow_hidden()
            .gap_2()
            // Composed text — dead keys, compose sequences, any CJK
            // input method — arrives through `EntityInputHandler`
            // rather than as key events, and `handle_input` may only be
            // called during paint. A zero-size canvas is the smallest
            // paint hook gpui offers; it lives in the editor pane so
            // the handler is installed exactly while the editor is on
            // screen and taking text.
            .child(self.ime_hook(cx))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_grow()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(body)
                    .child(Self::body_scrollbar(co, lines, view, scroll_start, cx)),
            );
        self.with_editor_overlays(pane, co, cx)
    }

    /// Hang the editor's two overlays — the "/" block menu and the
    /// completion popup — on the pane, when they are open.
    fn with_editor_overlays(
        &self,
        mut pane: gpui::Div,
        co: Colors,
        cx: &Context<Self>,
    ) -> gpui::Div {
        if let Some(menu) = self.slash_menu(co, cx) {
            pane = pane.child(menu);
        }
        if let Some(popup) = self.completion_popup(co) {
            pane = pane.child(popup);
        }
        pane
    }

    /// A zero-size element whose paint installs the input method
    /// handler.
    ///
    /// `Window::handle_input` debug-asserts it is called during paint,
    /// and `render` is the element-tree build rather than paint — so
    /// this is the hook. Without it the window read
    /// `KeyDownEvent.key_char` and nothing else, which meant a dead key
    /// produced no character and no IME could type at all.
    fn ime_hook(&self, cx: &Context<Self>) -> gpui::Canvas<()> {
        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        gpui::canvas(
            |_bounds, _w, _cx| (),
            move |bounds, (), window, cx| {
                window.handle_input(&focus, gpui::ElementInputHandler::new(bounds, entity), cx);
            },
        )
    }

    /// A scrollbar for the body editor's own viewport.
    ///
    /// The pane's [`scrollbar`] cannot serve here: the editor paints
    /// only the visible lines, so its container never overflows and
    /// the shared bar measures a content height equal to its viewport.
    /// The geometry is the same ([`thumb_geometry`]) in units of
    /// lines, and a drag lands on a first-visible line through
    /// [`ModalApp::body_scroll_to`].
    fn body_scrollbar(
        co: Colors,
        lines: usize,
        view: usize,
        scroll_start: usize,
        cx: &Context<Self>,
    ) -> gpui::Div {
        /// Keeps the thumb grabbable in a very long body.
        const MIN_THUMB: f32 = 0.06;
        #[allow(clippy::cast_precision_loss)]
        let (content, viewport, scroll) = (lines as f32, view as f32, scroll_start as f32);
        let track = div()
            .debug_selector(|| "body-scrollbar".to_owned())
            .w(px(10.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(mix_u32(co.bg, co.panel, 160)));
        // The bar is the editor body's sibling and shares its height,
        // so the body's measured bounds are the track's — read when the
        // mouse arrives rather than when the element is built, because
        // on the frame the editor opens there are no bounds yet and a
        // bar built from them would take the first drag and drop it
        // (see [`scrollbar`]).
        let jump = move |this: &mut Self, y: gpui::Pixels| {
            let bounds = this.body_track.bounds();
            let track_h = f32::from(bounds.size.height);
            let Some(thumb) = thumb_geometry(viewport, content, scroll, MIN_THUMB) else {
                return;
            };
            if track_h <= 0.0 {
                return;
            }
            // In line units: the fraction of the *scrollable* range,
            // scaled back onto the lines the body actually has.
            let fraction = track_fraction(
                f32::from(y),
                f32::from(bounds.origin.y),
                track_h,
                thumb.height,
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let line = scroll_for_track_fraction(viewport, content, fraction).round() as usize;
            this.app.body_scroll_to(line, view);
        };
        let track = track
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut Self, ev: &gpui::MouseDownEvent, _w, cx| {
                    jump(this, ev.position.y);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(
                move |this: &mut Self, ev: &gpui::MouseMoveEvent, _w, cx| {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        jump(this, ev.position.y);
                        cx.notify();
                    }
                },
            ));
        let Some(thumb) = thumb_geometry(viewport, content, scroll, MIN_THUMB) else {
            return track;
        };
        track
            .cursor_pointer()
            .child(div().h(gpui::relative(thumb.top)))
            .child(
                div()
                    .h(gpui::relative(thumb.height))
                    .w_full()
                    .rounded_sm()
                    .bg(rgb(co.muted))
                    .hover(move |s| s.bg(rgb(co.accent))),
            )
    }

    /// The Notion "/" block menu, when open: each entry inserts real
    /// org syntax at the cursor, and clicking one is the same accept
    /// Enter performs.
    fn slash_menu(&self, co: Colors, cx: &Context<Self>) -> Option<gpui::Div> {
        let query = self.app.slash_query()?;
        let items = self.app.slash_items();
        if items.is_empty() {
            return None;
        }
        let cursor = self.app.slash_cursor();
        Some(
            div()
                .debug_selector(|| "slash-menu".to_owned())
                .flex()
                .flex_col()
                .p_1()
                .rounded_md()
                .bg(rgb(co.bg))
                .border_1()
                .border_color(rgb(co.accent))
                .child(
                    div()
                        .px_2()
                        .text_size(self.sz(10.0))
                        .text_color(rgb(co.muted))
                        .child(format!("insert block  /{query}")),
                )
                .children(items.into_iter().enumerate().map(|(i, tpl)| {
                    let hot = i == cursor;
                    // The first line of the template is the preview —
                    // what you are about to put in the file.
                    let preview = tpl.text.lines().next().unwrap_or_default().to_owned();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(rgb(if hot { co.selection } else { co.bg }))
                        .hover(move |s| s.bg(rgb(if hot { co.selection } else { co.hover })))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                this.app.slash_click(i);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .w(px(90.0))
                                .text_size(self.sz(12.0))
                                .text_color(rgb(if hot { co.fg } else { co.muted }))
                                .child(tpl.label),
                        )
                        .child(
                            div()
                                .text_size(self.sz(11.0))
                                .text_color(rgb(co.code))
                                .child(preview),
                        )
                })),
        )
    }

    /// The C-n / typing-idle completion popup, when one is open.
    fn completion_popup(&self, co: Colors) -> Option<gpui::Div> {
        let items = self.app.body_completion_items();
        if items.is_empty() {
            return None;
        }
        let ix = self.app.body_completion_ix().unwrap_or(0);
        Some(
            div()
                .debug_selector(|| "completion-popup".to_owned())
                .flex()
                .flex_col()
                .p_1()
                .rounded_md()
                .bg(rgb(co.bg))
                .border_1()
                .border_color(rgb(co.border))
                .children(items.iter().enumerate().map(|(i, item)| {
                    div()
                        .px_2()
                        .text_size(self.sz(12.0))
                        .bg(rgb(if i == ix { co.selection } else { co.bg }))
                        .text_color(rgb(if i == ix { co.fg } else { co.muted }))
                        .child(item.clone())
                })),
        )
    }

    /// The tag picker (Q3-V6): every tag the vault uses, ticked where
    /// this headline carries it.
    fn tag_pane(&self, co: Colors, cx: &Context<Self>) -> Vec<gpui::Div> {
        let rows: Vec<_> = self
            .app
            .tag_rows(&self.shell)
            .into_iter()
            .filter(|r| r.matches_filter)
            .collect();
        if rows.is_empty() {
            return vec![
                div()
                    .text_color(rgb(co.muted))
                    .child("no tag matches — SPC makes it a new one"),
            ];
        }
        rows.into_iter()
            .enumerate()
            .map(|(i, r)| {
                let name = r.name.clone();
                list_row(
                    co,
                    self.app.zoom(),
                    i == self.app.selected(),
                    format!("{} {}", if r.on { "☑" } else { "☐" }, r.name),
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.tag_toggle(&name);
                        cx.notify();
                    }),
                )
            })
            .collect()
    }

    /// The refile target picker (Q3-V1): every headline that could take
    /// the subtree, indented by level, with the file it is in.
    fn refile_pane(&self, co: Colors, cx: &Context<Self>) -> Vec<gpui::Div> {
        let rows: Vec<_> = self
            .app
            .refile_rows(&self.shell)
            .into_iter()
            .enumerate()
            .filter(|(_, r)| r.matches_filter)
            .collect();
        if rows.is_empty() {
            return vec![
                div()
                    .text_color(rgb(co.muted))
                    .child("no headline matches that"),
            ];
        }
        rows.into_iter()
            .enumerate()
            .map(|(shown, (i, r))| {
                let indent = "  ".repeat(usize::from(r.level.saturating_sub(1)));
                list_row(
                    co,
                    self.app.zoom(),
                    shown == self.app.selected(),
                    format!("{indent}{}    {}", r.title, r.path),
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.refile_click(&mut this.shell, i);
                        cx.notify();
                    }),
                )
            })
            .collect()
    }

    /// The date picker (Q3-V4): a month grid over `SCHEDULED:` or
    /// `DEADLINE:`, with the selected day inverted and today outlined.
    ///
    /// Every cell is a click target: the keyboard moves by day and week,
    /// and a pointer picks a date the way a pointer expects to.
    fn date_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        const HEAD: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
        let grid = self.app.date_grid();
        let today = self.app.today().to_owned();
        let header = div()
            .flex()
            .flex_row()
            .gap_2()
            .children(HEAD.into_iter().map(|d| {
                div()
                    .w(px(32.0))
                    .text_size(self.sz(11.0))
                    .text_color(rgb(co.muted))
                    .child(d)
            }));
        let weeks: Vec<gpui::Div> = grid
            .weeks
            .iter()
            .map(|week| {
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .children(week.iter().map(|cell| {
                        let Some(day) = *cell else {
                            return div().w(px(32.0)).child(" ");
                        };
                        let ymd = format!("{:04}-{:02}-{day:02}", grid.year, grid.month);
                        let selected = ymd == grid.selected;
                        let is_today = ymd == today;
                        div()
                            .w(px(32.0))
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_size(self.sz(13.0))
                            .bg(rgb(if selected { co.accent } else { co.bg }))
                            .text_color(rgb(if selected {
                                co.bg
                            } else if is_today {
                                co.accent
                            } else {
                                co.fg
                            }))
                            .hover(move |s| s.bg(rgb(co.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                    this.app.date_click(day);
                                    cx.notify();
                                }),
                            )
                            .child(format!("{day:2}"))
                    }))
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(self.sz(13.0))
                    .text_color(rgb(co.accent))
                    .child(format!(
                        "{}   {} {}",
                        grid.field,
                        month_name(grid.month),
                        grid.year
                    )),
            )
            .child(header)
            .children(weeks)
            .child(
                div()
                    .text_size(self.sz(11.0))
                    .text_color(rgb(co.muted))
                    .child(if grid.typed.is_empty() {
                        "h/l day · j/k week · </> month · . today · RET set · x clear".to_owned()
                    } else {
                        format!("typed: {}▏", grid.typed)
                    }),
            )
    }

    /// The open-buffer list (Q1-B1): every buffer this session has, the
    /// current one first, unsaved ones marked, filtered as you type.
    fn buffer_pane(&self, co: Colors, cx: &Context<Self>) -> Vec<gpui::Div> {
        let rows: Vec<_> = self
            .app
            .buffer_rows(&self.shell)
            .into_iter()
            .enumerate()
            .filter(|(_, r)| r.matches_filter)
            .collect();
        if rows.is_empty() {
            return vec![
                div()
                    .text_color(rgb(co.muted))
                    .child("no buffers open — open a note and it lands here"),
            ];
        }
        rows.into_iter()
            .enumerate()
            .map(|(shown, (i, r))| {
                let label = format!(
                    "{} {}{}",
                    if r.current { "●" } else { "○" },
                    r.name,
                    if r.dirty { "  [+]" } else { "" }
                );
                list_row(
                    co,
                    self.app.zoom(),
                    shown == self.app.selected(),
                    label,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.buffer_click(&this.shell, i);
                        cx.notify();
                    }),
                )
            })
            .collect()
    }

    /// The file picker (Q1-B4): every file in the vault, the ones recent
    /// sessions were in first.
    fn file_pane(&self, co: Colors, cx: &Context<Self>) -> Vec<gpui::Div> {
        let rows: Vec<_> = self
            .app
            .file_rows(&self.shell)
            .into_iter()
            .enumerate()
            .filter(|(_, r)| r.matches_filter)
            .collect();
        if rows.is_empty() {
            return vec![
                div()
                    .text_color(rgb(co.muted))
                    .child("no file matches that"),
            ];
        }
        rows.into_iter()
            .enumerate()
            .map(|(shown, (i, r))| {
                let label = if r.recent {
                    format!("★ {}", r.name)
                } else {
                    format!("  {}", r.name)
                };
                list_row(
                    co,
                    self.app.zoom(),
                    shown == self.app.selected(),
                    label,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.file_click(&this.shell, i);
                        cx.notify();
                    }),
                )
            })
            .collect()
    }

    /// The tab strip (Q1-B5): the open buffers across the top, click to
    /// switch. Hidden when there is at most one — a strip with a single
    /// tab in it is furniture, not information.
    fn tab_strip(&self, co: Colors, cx: &Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        if !self.tab_strip_visible() {
            return None;
        }
        let rows = self.app.buffer_rows(&self.shell);
        let tabs: Vec<gpui::Div> = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(self.sz(12.0))
                    .whitespace_nowrap()
                    .bg(rgb(if r.current { co.selection } else { co.bg }))
                    .text_color(rgb(if r.current { co.fg } else { co.muted }))
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.app.buffer_click(&this.shell, i);
                            cx.notify();
                        }),
                    )
                    .child(format!("{}{}", r.name, if r.dirty { " +" } else { "" }))
            })
            .collect();
        Some(
            div()
                .id("tab-strip")
                .flex()
                .flex_row()
                .gap_1()
                .px_2()
                .py_1()
                .overflow_x_hidden()
                .bg(rgb(co.panel))
                .children(tabs),
        )
    }

    /// The undo tree (Q2-U3): rows indented by depth, the active node
    /// marked, and every row a click target that jumps there — the
    /// same move Enter makes.
    fn undo_history_pane(&self, co: Colors, cx: &Context<Self>) -> Vec<gpui::Div> {
        let cursor = self.app.undo_history_cursor();
        self.app
            .undo_history_rows(&self.shell)
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                div()
                    .flex()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .bg(rgb(if i == cursor { co.selection } else { co.bg }))
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.app.undo_history_click(&mut this.shell, i);
                            cx.notify();
                        }),
                    )
                    // The branches, in the muted colour: they are the
                    // shape of the history, not part of any one edit.
                    // The window's font is mono ([`app_font`]), so the
                    // bars stack into columns without help.
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(rgb(co.muted))
                            .child(r.graph.clone()),
                    )
                    .child(
                        div()
                            .text_color(rgb(if r.is_current { co.accent } else { co.muted }))
                            .child(format!(
                                "{} {}",
                                if r.is_current { "●" } else { "○" },
                                r.label
                            )),
                    )
            })
            .collect()
    }

    /// Clickable `(label, block id)` rows: a click moves the outline
    /// selection to that headline, which is the same jump Enter makes
    /// (I8). Shared by the headline list and the body-search hits.
    fn id_rows(
        &self,
        co: Colors,
        rows: Vec<(String, String)>,
        cx: &Context<Self>,
    ) -> Vec<gpui::Div> {
        let selected = self.app.selected();
        rows.into_iter()
            .enumerate()
            .map(|(i, (label, id))| {
                list_row(
                    co,
                    self.app.zoom(),
                    i == selected,
                    label,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.select_by_id(&this.shell, &id);
                        cx.notify();
                    }),
                )
            })
            .collect()
    }

    /// The Notion-style database table: a header row and one aligned
    /// row per headline, empty cells left empty.
    fn db_table(&self, co: Colors) -> gpui::Div {
        let (header, rows) = self.app.db_rows(&self.shell);
        // Fixed column widths keep the table readable without a
        // measurement pass; the title takes the slack.
        let widths = [260.0_f32, 90.0, 70.0, 160.0];
        let cell = |text: String, w: f32, colour: u32, size: f32| {
            div()
                .w(px(w))
                .pr_2()
                .overflow_hidden()
                .text_color(rgb(colour))
                .text_size(self.sz(size))
                .child(text)
        };
        let selected = self.app.selected();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .pb_1()
                    .border_b_1()
                    .border_color(rgb(co.border))
                    .children(
                        header
                            .into_iter()
                            .zip(widths)
                            .map(|(h, w)| cell(h.to_uppercase(), w, co.heading2, 10.0)),
                    ),
            )
            .children(rows.into_iter().enumerate().map(|(i, cells)| {
                let hot = i == selected;
                div()
                    .flex()
                    .py_1()
                    .bg(rgb(if hot { co.selection } else { co.bg }))
                    .hover(move |s| s.bg(rgb(if hot { co.selection } else { co.hover })))
                    .children(
                        cells
                            .into_iter()
                            .zip(widths)
                            .enumerate()
                            .map(|(col, (text, w))| {
                                // Title reads as content, the rest as metadata.
                                let colour = match col {
                                    0 => co.fg,
                                    1 => co.error,
                                    2 => co.warning,
                                    _ => co.muted,
                                };
                                cell(text, w, colour, 12.0)
                            }),
                    )
            }))
    }

    /// Every `#+BEGIN_SRC` block in the vault, with a run button and
    /// the output of the last run (org-babel).
    ///
    /// Clicking a row selects it; ▶ runs it through the kernel's
    /// `eval-block`, which honours the `eval_trust` allowlist — a
    /// refusal shows up in the status line rather than being worked
    /// around. Output is cleared whenever the cursor moves, so what is
    /// on screen always belongs to the block it sits under.
    fn blocks_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let selected = self.app.selected();
        let mut pane = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .px_2()
                        .rounded_md()
                        .bg(rgb(co.panel))
                        .text_color(rgb(co.success))
                        .text_size(self.sz(11.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(rgb(co.hover)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this: &mut Self, _ev, _w, cx| {
                                this.click("eval-block", cx);
                            }),
                        )
                        .child("▶ run"),
                )
                .children(self.app.chord_for("eval-block").map(|chord| {
                    div()
                        .text_size(self.sz(10.0))
                        .text_color(rgb(co.accent))
                        .child(chord)
                })),
        );
        pane = pane.children(
            self.app
                .block_rows(&self.shell)
                .into_iter()
                .enumerate()
                .map(|(i, (path, lang, first))| {
                    list_row(
                        co,
                        self.app.zoom(),
                        i == selected,
                        format!("{lang:8} {first}  — {}", short_path(&path)),
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.app.jump_list_row(&this.shell, i);
                            cx.notify();
                        }),
                    )
                }),
        );
        if let Some(out) = self.app.block_output() {
            pane = pane.child(
                div()
                    .mt_2()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(co.panel))
                    .border_1()
                    .border_color(rgb(co.success))
                    .text_size(self.sz(12.0))
                    .child(
                        div()
                            .text_size(self.sz(10.0))
                            .text_color(rgb(co.muted))
                            .child("output"),
                    )
                    .children(
                        out.lines()
                            .map(|l| div().text_color(rgb(co.fg)).child(l.to_owned())),
                    ),
            );
        }
        pane
    }

    /// A plain read-only list, with a line saying so when it is empty
    /// — a blank pane is indistinguishable from a broken one.
    ///
    /// `empty` names what is missing and how it gets filled: "nothing
    /// here yet" tells a reader nothing they cannot already see.
    fn text_rows(co: Colors, zoom: f32, rows: Vec<String>, empty: &'static str) -> Vec<gpui::Div> {
        if rows.is_empty() {
            return vec![div().text_color(rgb(co.muted)).child(empty)];
        }
        rows.into_iter()
            .map(|line| {
                div()
                    .px_2()
                    .py_1()
                    .text_size(sz_at(12.0, zoom))
                    .text_color(rgb(co.fg))
                    .child(line)
            })
            .collect()
    }

    /// The assistant: what `config.org` says is behind it, the
    /// transcript, and the question field.
    ///
    /// The configuration line is the important part. A chat box that
    /// silently does nothing because no provider is set — or because
    /// the key's environment variable is not exported — is the worst
    /// version of this, so the pane says which it is, and says it
    /// before you type rather than after.
    fn llm_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let status = self.app.llm_config_status(&self.shell);
        let render_granted = self.app.llm_render_access();
        let mut pane = div().flex().flex_col().gap_2().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px_2()
                        .rounded_md()
                        .text_size(self.sz(10.0))
                        .bg(rgb(if status.ready { co.success } else { co.warning }))
                        .text_color(rgb(co.bg))
                        .child(if status.ready {
                            "ready"
                        } else {
                            "not configured"
                        }),
                )
                .child(
                    div()
                        .flex_grow()
                        .text_size(self.sz(11.0))
                        .text_color(rgb(co.muted))
                        .child(status.detail),
                )
                // The render grant is louder than the rest: it lets
                // a model read the screen, so it is stated here as
                // well as in the status bar, and it is one click.
                .child(
                    div()
                        .px_2()
                        .rounded_md()
                        .text_size(self.sz(10.0))
                        .bg(rgb(co.panel))
                        .text_color(rgb(if render_granted { co.accent } else { co.muted }))
                        .cursor_pointer()
                        .hover(move |s| s.bg(rgb(co.hover)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this: &mut Self, _ev, _w, cx| {
                                this.click("toggle-llm-render", cx);
                            }),
                        )
                        .child(if render_granted {
                            "◉ can read the screen"
                        } else {
                            "○ cannot read the screen"
                        }),
                ),
        );
        pane = pane.children(self.chat_transcript(co));
        if self.app.chat_busy() {
            pane = pane.child(
                div()
                    .px_2()
                    .text_size(self.sz(11.0))
                    .text_color(rgb(co.warning))
                    .child("…thinking".to_owned()),
            );
        }
        pane.child(
            div()
                .mt_2()
                .p_2()
                .rounded_md()
                .bg(rgb(co.bg))
                .border_1()
                .border_color(rgb(if status.ready { co.accent } else { co.border }))
                .text_size(self.sz(12.0))
                .text_color(rgb(co.fg))
                .child(format!("{}▏", self.app.chat_buffer())),
        )
    }

    /// The transcript, or an explanation of what this pane is for when
    /// there is not one yet.
    fn chat_transcript(&self, co: Colors) -> Vec<gpui::Div> {
        if self.app.chat_turns().is_empty() {
            return vec![
                div()
                    .text_color(rgb(co.muted))
                    .text_size(self.sz(12.0))
                    .child(
                        "Ask about the vault. The assistant reads and edits it through the same \
                 commands you do, and can only see the rendered view if you grant it."
                            .to_owned(),
                    ),
            ];
        }
        self.app
            .chat_turns()
            .iter()
            .map(|turn| {
                let (who, colour) = if turn.from_user {
                    ("you", co.accent)
                } else {
                    ("assistant", co.success)
                };
                div()
                    .flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(if turn.from_user { co.bg } else { co.panel }))
                    .child(
                        div()
                            .w(px(70.0))
                            .text_size(self.sz(10.0))
                            .text_color(rgb(colour))
                            .child(who),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .text_size(self.sz(12.0))
                            .text_color(rgb(co.fg))
                            .child(turn.text.clone()),
                    )
            })
            .collect()
    }

    /// Send the pending question to the configured provider.
    ///
    /// The provider call is blocking I/O, so it runs on the background
    /// executor and only the answer comes back — the window stays
    /// responsive while a model thinks. Tool calls are executed back on
    /// the UI thread, because they touch the vault.
    fn ask_llm(&mut self, question: String, cx: &mut Context<Self>) {
        let status = self.app.llm_config_status(&self.shell);
        if !status.ready {
            self.app.chat_answer(status.detail);
            cx.notify();
            return;
        }
        let provider_name = status.provider.clone();
        let model = status.model.clone().unwrap_or_default();
        let endpoint = status.endpoint;
        let key = closure_config::Config::from_path(&self.shell.vault.root().join("config.org"))
            .ok()
            .and_then(|c| c.llm_key_env)
            .and_then(|var| closure_llm::resolve_key(&var))
            .unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move {
                    // `llm_endpoint` is what points the local/OpenAI-
                    // compatible providers somewhere other than their
                    // default host.
                    let host = endpoint.as_deref().unwrap_or("http://localhost:11434");
                    let provider = closure_llm::build_provider(
                        closure_llm::provider_kind(provider_name.as_deref()),
                        &model,
                        host,
                        &key,
                    );
                    // One shot for now: the tool loop needs to run
                    // commands against the vault, which lives on the UI
                    // thread, so multi-turn tool use is the next step
                    // rather than a lie told here.
                    provider.complete(&question).map_err(|e| format!("{e}"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.app
                    .chat_answer(answer.unwrap_or_else(|e| format!("assistant failed: {e}")));
                cx.notify();
            });
        })
        .detach();
    }

    /// The link graph: what the vault points at most, what it points
    /// at not at all, and what it points at in vain.
    ///
    /// The keyboard cursor walks hubs then orphans (the core counts them
    /// as one list), so the row under it is marked here — the pane used
    /// to show no cursor at all, which made `j`/`k` on this surface look
    /// broken. The lists are capped, and a capped list says so instead
    /// of quietly ending.
    fn graph_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        /// Hubs shown before the list is cut off.
        const HUBS: usize = 20;
        /// Orphans shown before the list is cut off.
        const ORPHANS: usize = 50;
        let section = |title: &'static str, colour: u32| {
            div()
                .mt_2()
                .text_size(self.sz(10.0))
                .text_color(rgb(colour))
                .child(title)
        };
        let more = |hidden: usize| {
            (hidden > 0).then(|| {
                div()
                    .px_2()
                    .text_size(self.sz(10.0))
                    .text_color(rgb(co.muted))
                    .child(format!("     …and {hidden} more"))
            })
        };
        let cursor = self.app.selected();
        let jump = |i: usize, id: String, label: String| {
            let hot = i == cursor;
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_size(self.sz(12.0))
                .text_color(rgb(co.fg))
                .bg(rgb(if hot { co.selection } else { co.bg }))
                .hover(move |s| s.bg(rgb(if hot { co.selection } else { co.hover })))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| {
                        this.app.select_by_id(&this.shell, &id);
                        cx.notify();
                    }),
                )
                .child(label)
        };
        let hubs = self.app.hub_rows(&self.shell);
        let orphans = self.app.orphan_rows(&self.shell);
        let dead = self.app.dead_link_rows(&self.shell);
        // The cursor's index runs across both lists, so the orphans
        // continue where the hubs left off.
        let orphan_base = hubs.len();
        // The lists are capped, and the cap used to be the *first* n:
        // past it the cursor was simply not painted, so `j` moved
        // something invisible and nothing scrolled after it. The window
        // follows the cursor instead, and says what it is hiding on
        // either side rather than quietly ending.
        let hub_win = visible_window(cursor, hubs.len(), HUBS);
        let orphan_win = visible_window(cursor.saturating_sub(orphan_base), orphans.len(), ORPHANS);
        let (hub_before, hub_after) = (hub_win.start, hubs.len() - hub_win.end);
        let (orphan_before, orphan_after) = (orphan_win.start, orphans.len() - orphan_win.end);
        div()
            .flex()
            .flex_col()
            .child(section("hubs — most linked to", co.accent))
            .children(more(hub_before))
            .children(
                hubs.into_iter()
                    .enumerate()
                    .filter(|(i, _)| hub_win.contains(i))
                    .map(|(i, (id, title, n))| jump(i, id, format!("{n:>3}  {title}"))),
            )
            .children(more(hub_after))
            .child(section("orphans — nothing links here", co.warning))
            .children(more(orphan_before))
            .children(
                orphans
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| orphan_win.contains(i))
                    .map(|(i, (id, title))| jump(orphan_base + i, id, format!("     {title}"))),
            )
            .children(more(orphan_after))
            .child(section("dead links — targets that do not exist", co.error))
            .children(Self::text_rows(
                co,
                self.app.zoom(),
                dead,
                "none — every link in the vault resolves",
            ))
    }

    /// Pairing and collaboration.
    ///
    /// Explains itself, because nothing else does: two people each
    /// open this, one hands over the line under "your ticket", the
    /// other pastes it and presses Enter, and either side can then run
    /// a round. Divergent titles land in the Conflicts surface rather
    /// than being resolved silently.
    /// The vault's own sync ticket, selectable-looking; a click copies
    /// it out.
    fn sync_ticket_box(&self, co: Colors, ticket: String, cx: &Context<Self>) -> gpui::Div {
        div()
            .p_2()
            .rounded_md()
            .bg(rgb(co.panel))
            .border_1()
            .border_color(rgb(co.border))
            .text_size(self.sz(11.0))
            .text_color(rgb(co.success))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(co.hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _ev, _w, cx| {
                    // A ticket is handed to *another application* — a
                    // chat window, a mail — so the kill ring alone was
                    // useless here: it never left closure.
                    let ticket = this.app.sync_mut().ticket();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(ticket.clone()));
                    this.shell.vault.push_kill_ring(ticket);
                    this.app
                        .set_status("ticket copied to the clipboard".to_owned());
                    cx.notify();
                }),
            )
            .child(ticket)
    }

    fn sync_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        // The `sync` command initialises this before switching here,
        // so the None arm only fires if someone paints the surface
        // without opening it.
        let Some(sync) = self.app.sync() else {
            return div()
                .text_color(rgb(co.muted))
                .child("sync is not initialised — run the sync command".to_owned());
        };
        let ticket = sync.ticket();
        let peers = sync.peers().to_vec();
        let blocks = sync.block_count();
        let typed = self.app.sync_buffer().to_owned();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(self.sz(11.0))
                    .text_color(rgb(co.muted))
                    .child(
                        "Give the other person your ticket. Paste theirs below and press Enter. \
                         Then ▲ push to send them your replica; conflicting titles appear under \
                         Conflicts instead of one side winning silently."
                            .to_owned(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(self.sz(10.0))
                            .text_color(rgb(co.heading2))
                            .child("your ticket — hand this over"),
                    )
                    // A ticket naming a port nothing listens on is
                    // worse than no ticket, so binding is one click and
                    // the ticket is rewritten to the real address.
                    .child(
                        div()
                            .px_2()
                            .rounded_md()
                            .bg(rgb(co.panel))
                            .text_size(self.sz(10.0))
                            .text_color(rgb(co.accent))
                            .cursor_pointer()
                            .hover(move |s| s.bg(rgb(co.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this: &mut Self, _ev, _w, cx| {
                                    this.start_listening(cx);
                                }),
                            )
                            .child("◎ listen"),
                    ),
            )
            .child(self.sync_ticket_box(co, ticket, cx))
            .child(
                div()
                    .text_size(self.sz(10.0))
                    .text_color(rgb(co.heading2))
                    .child("their ticket — type or paste, then Enter"),
            )
            .child(
                div()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(co.bg))
                    .border_1()
                    .border_color(rgb(co.accent))
                    .text_size(self.sz(11.0))
                    .text_color(rgb(co.fg))
                    .child(format!("{typed}▏")),
            )
            .child(
                div()
                    .text_size(self.sz(10.0))
                    .text_color(rgb(co.muted))
                    .child(format!("replica: {blocks} block(s) known")),
            )
            .children(
                peers
                    .into_iter()
                    .map(|peer| Self::peer_row(co, self.app.zoom(), &peer, cx)),
            )
    }

    /// One peer: a push button, its address, and what the last round
    /// with it did.
    fn peer_row(
        co: Colors,
        zoom: f32,
        peer: &closure_shell_core::Peer,
        cx: &Context<Self>,
    ) -> gpui::Div {
        use closure_shell_core::PeerState as S;
        let (colour, state) = match &peer.state {
            S::Known => (co.muted, "known".to_owned()),
            S::Synced { blocks } => (co.success, format!("synced · {blocks} blocks")),
            S::Failed(e) => (co.error, format!("failed · {e}")),
        };
        let addr = peer.addr;
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(co.panel))
            .child(
                div()
                    .px_2()
                    .rounded_md()
                    .text_size(sz_at(11.0, zoom))
                    .text_color(rgb(co.accent))
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.push_to_peer(addr, cx);
                        }),
                    )
                    .child("▲ push"),
            )
            .child(
                div()
                    .w(px(160.0))
                    .text_size(sz_at(11.0, zoom))
                    .text_color(rgb(co.fg))
                    .child(addr.to_string()),
            )
            .child(
                div()
                    .text_size(sz_at(11.0, zoom))
                    .text_color(rgb(colour))
                    .child(state),
            )
    }

    /// Write the converged replica back into the vault's files.
    ///
    /// A borrow-split helper: `SyncApp` lives inside `app` and the
    /// vault inside `shell`, and both are fields of this view, so the
    /// call needs to name them separately.
    fn apply_sync_to_vault(&mut self) -> usize {
        let Some(sync) = self.app.sync() else {
            return 0;
        };
        // Cloning the replica is cheaper than restructuring ownership
        // for a per-sync operation, and keeps the write path honest:
        // it reads a snapshot and writes through kernel commands.
        let snapshot = sync.session().clone();
        let mut applied = 0usize;
        let ids: Vec<closure_core::BlockId> = snapshot.block_ids().cloned().collect();
        for id in ids {
            let Some((headline, _)) = self.shell.vault.find_by_id(&id) else {
                continue;
            };
            let current_title = headline.title().to_owned();
            let current_body = headline.body_text().to_owned();
            if let Some(title) = snapshot.title_of(&id)
                && title != current_title
                && self.shell.rename_headline(&id, title).is_ok()
            {
                applied += 1;
            }
            if let Some(body) = snapshot.body_of(&id)
                && body != current_body
                && self.shell.set_body(&id, &body).is_ok()
            {
                applied += 1;
            }
        }
        applied
    }

    /// Bind a listener and accept one inbound sync round on the
    /// background executor, so pairing works in both directions.
    ///
    /// Binding alone is what this used to do, which made the ticket a
    /// lie: the address was real, the port was open, and nothing ever
    /// answered on it — the peer's `▲ push` hung in the backlog until it
    /// timed out. The responder round is blocking socket I/O, so it runs
    /// off the UI thread and only its outcome comes back; the replica
    /// the peer sent is merged and written into the vault exactly as an
    /// outbound round's is, and the listener re-arms for the next one.
    fn start_listening(&mut self, cx: &mut Context<Self>) {
        let bound = match self.app.sync_mut().listen() {
            Ok(addr) => addr,
            Err(e) => {
                self.app.set_status(format!("listen failed: {e}"));
                cx.notify();
                return;
            }
        };
        // What we bound and what a peer dials are two addresses now, and
        // the useful one is the second: `0.0.0.0:7420` in a status line
        // tells the user nothing they can hand to anyone.
        let ticket_addr = self.app.sync_mut().ticket_addr();
        self.app.set_status(format!(
            "listening on {bound}, dial {ticket_addr} — hand over your ticket"
        ));
        cx.notify();
        self.accept_one(cx);
    }

    /// Wait for one peer to dial in, merge what it sends, and arm the
    /// next wait.
    fn accept_one(&mut self, cx: &Context<Self>) {
        self.app.sync_mut().snapshot(&self.shell);
        // A socket open to the network is answered only once there is
        // someone to tell apart from a stranger — an inbound round
        // writes into the vault, and an empty trusted set accepts any
        // well-formed signature.
        if let Err(why) = self.app.sync_mut().inbound_ready() {
            self.accept_armed = false;
            self.app.set_status(why);
            return;
        }
        let sync = self.app.sync_mut();
        let Some(listener) = sync.listener() else {
            self.accept_armed = false;
            return;
        };
        self.accept_armed = true;
        let mut session = sync.session().clone();
        let signing = sync.signing_key().clone();
        let trusted = sync.trusted_keys();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    closure_sync::TcpSyncTransport::serve_once_secure(
                        &listener,
                        &mut session,
                        &signing,
                        &trusted,
                    )
                    .map(|()| session)
                    .map_err(|e| format!("{e}"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(session) => {
                        let conflicts = this.app.sync_mut().merge_session(&session);
                        let blocks = this.app.sync_mut().block_count();
                        let applied = this.apply_sync_to_vault();
                        if conflicts.is_empty() {
                            this.app.set_status(format!(
                                "a peer synced in — {blocks} block(s), {applied} field(s) written"
                            ));
                        } else {
                            let n = conflicts.len();
                            this.app.set_conflicts(conflicts);
                            this.app.set_status(format!(
                                "a peer synced in — {n} conflict(s) to resolve (g m)"
                            ));
                        }
                        // Keep answering: pairing is not a single round.
                        this.accept_one(cx);
                    }
                    // A failed accept is the end of the loop rather than
                    // a spin: the listener is reported broken and the
                    // user can arm it again.
                    Err(e) => {
                        this.accept_armed = false;
                        this.app.set_status(format!("inbound sync failed: {e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Dial `addr` and exchange replicas.
    ///
    /// Blocking sockets on the UI thread would freeze the window, so
    /// the round runs on the background executor and only its outcome
    /// comes back — which is also why `SyncApp` keeps the networking
    /// out of the core.
    fn push_to_peer(&mut self, addr: std::net::SocketAddr, cx: &mut Context<Self>) {
        self.app.sync_mut().snapshot(&self.shell);
        let sync = self.app.sync_mut();
        let mut session = sync.session().clone();
        let signing = sync.signing_key().clone();
        let trusted = sync.trusted_keys();
        self.app.set_status(format!("connecting to {addr}…"));
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    closure_sync::TcpSyncTransport::connect_and_sync_secure(
                        addr,
                        &mut session,
                        &signing,
                        &trusted,
                    )
                    .map(|()| session)
                    .map_err(|e| format!("{e}"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(session) => {
                        let conflicts = this.app.sync_mut().merge_session(&session);
                        let blocks = this.app.sync_mut().block_count();
                        this.app.sync_mut().record_outcome(addr, Ok(blocks));
                        // The replica converging is half a sync; the
                        // files are the other half, and the only half
                        // the user can see.
                        let applied = this.apply_sync_to_vault();
                        if conflicts.is_empty() {
                            this.app.set_status(format!(
                                "synced with {addr} — {blocks} block(s), {applied} field(s) written"
                            ));
                        } else {
                            let n = conflicts.len();
                            this.app.set_conflicts(conflicts);
                            this.app.set_status(format!(
                                "synced with {addr} — {n} conflict(s) to resolve (g m)"
                            ));
                        }
                    }
                    Err(e) => {
                        this.app.sync_mut().record_outcome(addr, Err(e.clone()));
                        this.app.set_status(format!("sync failed: {e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Captured network flows with their verdict, and the allow/block
    /// buttons that write a rule for the selected one (X3).
    fn sniffer_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let sniffer = self.app.sniffer();
        let cursor = self.app.sniffer_cursor();
        let events = sniffer.events();
        if events.is_empty() {
            return div().text_color(rgb(co.muted)).child(
                "no captured flows — run `closure sniff` or feed the mock backend".to_owned(),
            );
        }
        let button = |label: &'static str, colour: u32, command: &'static str| {
            div()
                .px_2()
                .rounded_md()
                .bg(rgb(co.panel))
                .text_color(rgb(colour))
                .text_size(self.sz(11.0))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| this.click(command, cx)),
                )
                .child(label)
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("allow", co.success, "allow-flow"))
                    .child(button("block", co.error, "block-flow"))
                    .child(
                        div()
                            .text_color(rgb(co.muted))
                            .text_size(self.sz(11.0))
                            .child(format!("{} rule(s)", sniffer.rules().len())),
                    ),
            )
            .children(events.iter().enumerate().map(|(i, ev)| {
                let hot = i == cursor;
                let (verdict, colour) = match ev.action {
                    Some(closure_shell_core::FlowAction::Block) => ("BLOCK", co.error),
                    Some(closure_shell_core::FlowAction::Allow) => ("ALLOW", co.success),
                    Some(closure_shell_core::FlowAction::Log) => ("LOG", co.warning),
                    None => ("—", co.muted),
                };
                div()
                    .flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(rgb(if hot { co.selection } else { co.bg }))
                    .hover(move |s| s.bg(rgb(if hot { co.selection } else { co.hover })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                            this.app.sniffer_mut().select(i);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(56.0))
                            .text_size(self.sz(10.0))
                            .text_color(rgb(colour))
                            .child(verdict),
                    )
                    .child(div().text_color(rgb(co.fg)).child(ev.candidate.clone()))
            }))
    }

    /// Outstanding CRDT field conflicts and the ours/theirs decision
    /// for the selected one.
    fn conflicts_pane(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let app = self.app.conflicts();
        let conflicts = app.conflicts();
        if conflicts.is_empty() {
            return div()
                .text_color(rgb(co.muted))
                .child("no conflicts — every field converged".to_owned());
        }
        let button = |label: &'static str, command: &'static str| {
            div()
                .px_2()
                .rounded_md()
                .bg(rgb(co.panel))
                .text_color(rgb(co.accent))
                .text_size(self.sz(11.0))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| this.click(command, cx)),
                )
                .child(label)
        };
        let cursor = app.selected();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("keep ours", "resolve-ours"))
                    .child(button("take theirs", "resolve-theirs")),
            )
            .children(conflicts.iter().enumerate().map(|(i, c)| {
                let hot = i == cursor;
                div()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(if hot { co.selection } else { co.bg }))
                    .child(
                        div()
                            .text_size(self.sz(10.0))
                            .text_color(rgb(co.muted))
                            .child(format!("{} · {:?}", c.block, c.field)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().text_color(rgb(co.success)).child(c.ours.clone()))
                            .child(div().text_color(rgb(co.muted)).child("vs"))
                            .child(div().text_color(rgb(co.warning)).child(c.theirs.clone())),
                    )
            }))
    }

    /// The command palette, floating over the work.
    ///
    /// It used to be a *pane*: opening the launcher replaced the
    /// right-hand column, which is where the note is — so M-x hid the
    /// thing you opened it for, and from inside a buffer it could not
    /// be opened at all. Every editor that has one puts it in front
    /// instead (VS Code, Zed, `LazyVim`'s Telescope, Raycast): a bar near
    /// the top with the query in it, the matches under it, and the work
    /// still visible behind. A click on the scrim dismisses it, which
    /// is the same "never mind" Escape is.
    fn palette_overlay(&self, co: Colors, cx: &Context<Self>) -> Option<gpui::Deferred> {
        if self.app.surface() != ModalSurface::Palette {
            return None;
        }
        let panel = div()
            .debug_selector(|| "palette-panel".to_owned())
            .flex()
            .flex_col()
            .w(px(660.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(co.border))
            .bg(rgb(co.panel))
            // The panel eats the mouse; the scrim behind it is what
            // dismisses on a click.
            .occlude()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(co.border))
                    .text_size(self.sz(15.0))
                    .child(div().text_color(rgb(co.accent)).child("\u{276f}"))
                    .child(div().flex_grow().text_color(rgb(co.fg)).child(caret_text(
                        co,
                        self.app.field_buffer(),
                        self.app.field_cursor(),
                    )))
                    .child(
                        div()
                            .text_color(rgb(co.muted))
                            .text_size(self.sz(11.0))
                            .child(format!("{} commands", self.app.palette_shared().len())),
                    ),
            )
            .child(self.palette_list(co, cx))
            .child(
                div()
                    .px_4()
                    .py_1()
                    .border_t_1()
                    .border_color(rgb(co.border))
                    .text_color(rgb(co.muted))
                    .text_size(self.sz(10.0))
                    .child("\u{2191}\u{2193} move  ·  RET run  ·  Esc dismiss"),
            );
        Some(
            gpui::deferred(
                div()
                    .debug_selector(|| "palette-overlay".to_owned())
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(90.0))
                    .bg(gpui::rgba(0x0000_0059))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _ev, _w, cx| {
                            this.app
                                .on_key(&mut this.shell, "escape", false, false, None);
                            cx.notify();
                        }),
                    )
                    .child(panel),
            )
            .with_priority(2),
        )
    }

    /// The palette's matches, virtualized.
    ///
    /// It listed every command as a real element — the palette is the
    /// longest list in the shell, so it was also the slowest thing to
    /// scroll. A `uniform_list` builds only what the viewport shows,
    /// and the entries themselves come from the memo rather than being
    /// rescored per frame.
    fn palette_list(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let count = self.app.palette_shared().len();
        div()
            .flex()
            .flex_row()
            // Stated, not grown: see [`palette_list_height`].
            .h(px(palette_list_height(count, self.app.zoom())))
            .p_1()
            .child(
                gpui::uniform_list(
                    "palette",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _w, cx| {
                        let co = Colors::of(&this.theme);
                        let zoom = this.app.zoom();
                        let entries = this.app.palette_shared();
                        let cursor = this.app.palette_cursor();
                        range
                            .filter_map(|i| entries.get(i).map(|e| (i, e)))
                            .map(|(i, e)| {
                                let is_cur = i == cursor;
                                div()
                                    .debug_selector(move || format!("palette-row-{i}"))
                                    .flex()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    // Stated so a row is the height
                                    // [`palette_list_height`] budgets
                                    // for; inheriting gpui's default
                                    // made every row taller than the
                                    // list thought it was.
                                    .text_size(sz_at(PALETTE_ROW_TEXT, zoom))
                                    .bg(rgb(if is_cur { co.selection } else { co.bg }))
                                    .hover(move |s| {
                                        s.bg(rgb(if is_cur { co.selection } else { co.hover }))
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                            this.app.palette_click(&mut this.shell, i);
                                            if this.app.should_quit() {
                                                cx.quit();
                                            }
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        div()
                                            .w(px(140.0))
                                            .text_color(rgb(co.fg))
                                            .child(e.label.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_grow()
                                            .text_color(rgb(co.muted))
                                            .text_size(sz_at(11.0, zoom))
                                            .child(e.description.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(co.accent))
                                            .text_size(sz_at(11.0, zoom))
                                            .child(e.action.chord().to_owned()),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.palette_scroll.clone())
                .flex_grow(),
            )
            .child(scrollbar(
                "palette-scrollbar",
                co,
                &self.palette_scroll.0.borrow().base_handle.clone(),
                cx,
            ))
    }

    /// Agenda pane: rows grouped under date headers, SCHEDULED accent /
    /// DEADLINE error kind chips, the today group accented, overdue red.
    /// Row click jumps like Enter (`jump_list_row`).
    fn agenda_pane(&self, co: Colors, cx: &Context<Self>) -> impl IntoElement {
        let today = today_ymd(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        );
        let rows = self.app.agenda_context(&self.shell, &today);
        let selected = self.app.selected();
        let mut out = div().flex().flex_col().gap_0();
        let mut last_date = String::new();
        for (i, row) in rows.into_iter().enumerate() {
            if row.date != last_date {
                last_date.clone_from(&row.date);
                let (header_color, suffix) = if row.is_today {
                    (co.accent, "  · today")
                } else if row.is_overdue {
                    (co.error, "  · overdue")
                } else {
                    (co.heading2, "")
                };
                out = out.child(
                    div()
                        .mt_2()
                        .text_size(self.sz(11.0))
                        .text_color(rgb(header_color))
                        .child(format!("{}{suffix}", row.date)),
                );
            }
            let is_cur = i == selected;
            let kind_color = if row.kind == "DEADLINE" {
                co.error
            } else {
                co.accent
            };
            let title_color = if row.is_overdue { co.error } else { co.fg };
            out = out.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(rgb(if is_cur { co.selection } else { co.bg }))
                    .hover(move |s| s.bg(rgb(if is_cur { co.selection } else { co.hover })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev, _w, cx| {
                            this.app.jump_list_row(&this.shell, i);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .text_size(self.sz(10.0))
                            .text_color(rgb(kind_color))
                            .child(row.kind),
                    )
                    .child(div().text_color(rgb(title_color)).child(row.title)),
            );
        }
        out
    }

    /// Detail pane with click-to-edit fields: title → rename, meta →
    /// toggle-todo, tags → edit-tags, properties → edit-property,
    /// body → edit-body.
    fn detail_pane(
        &self,
        pane: gpui::Stateful<gpui::Div>,
        co: Colors,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let Some(d) = self.app.selected_detail(&self.shell) else {
            return pane.child(
                div()
                    .text_color(rgb(co.muted))
                    .child("no selection — j/k to move, / to search"),
            );
        };
        let props = d
            .properties
            .iter()
            .map(|(k, v)| format!(":{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tags = if d.tags.is_empty() {
            "+ tags".to_owned()
        } else {
            format!(":{}:", d.tags.join(":"))
        };
        // Right-clicking a field offers the per-field edit commands; the
        // body preview below offers the body ones (`with_menu`).
        let pane = with_menu(pane, closure_shell_core::ContextTarget::Detail, cx);
        pane.child(clickable(
            co,
            div()
                .text_color(rgb(co.accent))
                .text_lg()
                .child(d.title.clone()),
            "rename",
            cx,
        ))
        .child(clickable(
            co,
            div()
                .text_color(rgb(co.muted))
                .text_size(self.sz(12.0))
                .child(meta_line(&d)),
            "toggle-todo",
            cx,
        ))
        .child(clickable(
            co,
            div()
                .text_color(rgb(co.warning))
                .text_size(self.sz(12.0))
                .child(tags),
            "edit-tags",
            cx,
        ))
        .child(clickable(
            co,
            div()
                .text_color(rgb(co.error))
                .text_size(self.sz(11.0))
                .child(props),
            "edit-property",
            cx,
        ))
        .child(
            div()
                .text_color(rgb(co.muted))
                .text_size(self.sz(10.0))
                .child(d.path.clone()),
        )
        .child(clickable(
            co,
            {
                with_menu(
                    self.body_preview(co, &Self::preview_text(d.as_ref()), cx),
                    closure_shell_core::ContextTarget::Body,
                    cx,
                )
            },
            "edit-body",
            cx,
        ))
    }

    /// What the preview pane paints: the headline's own prose, then
    /// everything under it.
    ///
    /// A headline whose content is its children — most of them, in an
    /// outline — previewed as blank, so the only way to find out
    /// whether there was anything there was to open the editor. Same
    /// text, same highlighting as the editor gives it, which is what
    /// "just like in the body editor" asked for.
    fn preview_text(d: &Detail) -> String {
        if d.children.is_empty() {
            return d.body.clone();
        }
        let mut text = d.body.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&d.children);
        text
    }

    /// The read-only body preview under the detail fields.
    ///
    /// C3: it reads exactly like the editor — same spans, same palette,
    /// Whether image links are being painted (`toggle-inline-images`).
    #[must_use]
    pub const fn images_shown(&self) -> bool {
        self.app.images_shown()
    }

    /// How many images the selected note would paint — every link whose
    /// file resolves, or none while the toggle is off. What a test
    /// asserts on instead of hunting for a texture.
    #[must_use]
    pub fn painted_images(&self) -> usize {
        if !self.app.images_shown() {
            return 0;
        }
        let Some(detail) = self.app.selected_detail(&self.shell) else {
            return 0;
        };
        closure_shell_core::image_links(&detail.body)
            .into_iter()
            .filter(|link| self.image_path(&link.path).is_some())
            .count()
    }

    /// Resolve an image link's target to a file that is actually there.
    ///
    /// Relative targets are relative to the vault, which is what makes
    /// a note portable: the same link resolves in Emacs, on the other
    /// machine, and here. A path that does not exist paints nothing —
    /// a broken-image box tells the user less than the link does.
    fn image_path(&self, target: &str) -> Option<std::path::PathBuf> {
        let path = std::path::Path::new(target);
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.shell.vault.root().join(path)
        };
        full.is_file().then_some(full)
    }

    /// same weight and slant — but as one `StyledText` per line rather
    /// than a div per span. Ctrl-click follows a link here too; it used
    /// to work only in the editor, so reading a note and wanting to go
    /// where it points meant opening the editor first, which turned
    /// every link into an invitation to start typing.
    fn body_preview(&self, co: Colors, body: &str, cx: &Context<Self>) -> gpui::Div {
        let mut el = div()
            .mt_2()
            .flex_grow()
            .flex()
            .flex_col()
            .text_color(rgb(co.fg))
            .text_size(self.sz(13.0));
        if body.is_empty() {
            return el.child("+ body".to_owned());
        }
        for spans in self.highlighted(body).iter() {
            let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
            // The pictures this line points at, kept before `text` is
            // moved into the click handler below.
            let images: Vec<std::path::PathBuf> = if self.app.images_shown() {
                closure_shell_core::image_links(&text)
                    .into_iter()
                    .filter_map(|link| self.image_path(&link.path))
                    .collect()
            } else {
                Vec::new()
            };
            let styled = gpui::StyledText::new(text.clone()).with_highlights(
                span_ranges(spans).into_iter().map(|(range, kind)| {
                    (
                        range,
                        gpui::HighlightStyle {
                            color: Some(rgb(span_color(co, kind)).into()),
                            ..decorated(kind)
                        },
                    )
                }),
            );
            let layout = styled.layout().clone();
            el = el.child(div().min_h(px(17.0)).child(styled).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut Self, ev: &gpui::MouseDownEvent, _w, cx| {
                    if !ev.modifiers.control {
                        return;
                    }
                    let byte = layout
                        .index_for_position(ev.position)
                        .unwrap_or_else(|i| i)
                        .min(text.len());
                    if let Some(link) = line_links(&text)
                        .into_iter()
                        .find(|l| l.range.contains(&byte))
                    {
                        this.follow_link(&link.target, cx);
                        cx.stop_propagation();
                    }
                }),
            ));
            // …and the pictures the line points at, under it. The
            // editor's own lines are a fixed height — every viewport
            // measurement is derived from it — so this is where a note
            // *shows* its images, and the buffer keeps the link.
            for path in images {
                el = el.child(
                    div().my_1().child(
                        gpui::img(path)
                            .max_w(px(560.0))
                            .max_h(px(360.0))
                            .rounded_md(),
                    ),
                );
            }
        }
        el
    }

    /// Footer: a single compact line.
    ///
    /// It used to paint the entire keymap, every frame, as a grid that
    /// grew with the mode — hundreds of elements permanently occupying
    /// the bottom of the window. The bindings now live in the
    /// which-key panel ([`Self::which_key_panel`]), which opens on a
    /// pending chord or on demand; the footer keeps only what is
    /// always worth a line: the mode, the chord in flight, and the way
    /// in.
    fn footer(&self, co: Colors, cx: &Context<Self>) -> impl IntoElement {
        let pending = self.app.pending_chord();
        // The hints and the chord completions are as long as they are —
        // a dozen `x → command` chips is wider than any window. They go
        // in their own shrinkable, clipped group, because a flex row
        // that cannot shrink grows instead: the bar ran to 5195px in a
        // 1920px window and carried the `keys` toggle out past the
        // right edge, where no mouse could reach it.
        let mut hints = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(
                div()
                    .px_1()
                    .text_color(rgb(co.accent))
                    .child(format!("[{:?}]", self.app.input_mode())),
            );
        // A chord in flight: show what it is and what can follow.
        hints = if pending.is_empty() {
            hints.child(div().text_color(rgb(co.muted)).child(self.app.key_hints()))
        } else {
            hints
                .child(
                    div()
                        .text_color(rgb(co.warning))
                        .child(format!("{pending} ‸")),
                )
                .children(
                    self.app
                        .completions()
                        .into_iter()
                        .take(12)
                        .map(|(rest, cmd)| {
                            div()
                                .flex()
                                .px_1()
                                .child(div().text_color(rgb(co.accent)).child(rest))
                                .child(div().text_color(rgb(co.muted)).child(format!(" → {cmd}")))
                        }),
                )
        };
        let bar = div()
            .debug_selector(|| "footer-bar".to_owned())
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(rgb(co.panel))
            .text_size(self.sz(11.0))
            .child(hints);
        let open = self.app.which_key_open();
        bar.child(div().flex_grow()).child(
            div()
                .debug_selector(|| "which-key-toggle".to_owned())
                .px_2()
                .rounded_md()
                .bg(rgb(if open { co.selection } else { co.bg }))
                .text_color(rgb(co.accent))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this: &mut Self, _ev, _w, cx| {
                        this.app.run(&mut this.shell, "toggle-which-key");
                        cx.notify();
                    }),
                )
                .child(if open { "▾ keys" } else { "▸ keys" }),
        )
    }

    /// The Doom-style which-key panel: one column per palette section,
    /// group title on top, chord-sorted entries beneath (I4 — the same
    /// `which_key_groups` data every shell reads), every entry
    /// clickable.
    ///
    /// Shown only when pinned open or while a chord is pending, and
    /// scrollable, because the full keymap does not fit a window.
    ///
    /// While a chord *is* pending the panel narrows to what can follow
    /// it ([`which_key_filter`]) — the whole keymap is exactly the wrong
    /// answer at the one moment the user has asked a specific question.
    fn which_key_panel(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let groups = which_key_filter(self.app.which_key_groups(), &self.app.pending_chord());
        div()
            .flex()
            .flex_row()
            .max_h(px(280.0))
            .border_t_1()
            .border_color(rgb(co.border))
            .bg(rgb(co.panel))
            .child(
                div()
                    .id("which-key")
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .flex_grow()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .text_size(self.sz(11.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.which_key_scroll)
                    .children(groups.into_iter().map(|(title, entries)| {
                        let mut col = div()
                            .flex()
                            .flex_col()
                            .px_2()
                            .child(div().text_color(rgb(co.heading2)).child(title));
                        for (chord, cmd) in entries {
                            let run = cmd.clone();
                            let selector = format!("wk-{cmd}");
                            col = col.child(
                                div()
                                    .debug_selector(move || selector)
                                    .flex()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(rgb(co.hover)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                            let cmd = run.clone();
                                            this.click(&cmd, cx);
                                        }),
                                    )
                                    .child(
                                        div().w(px(56.0)).text_color(rgb(co.accent)).child(chord),
                                    )
                                    .child(div().text_color(rgb(co.muted)).child(cmd)),
                            );
                        }
                        col
                    })),
            )
            .child(scrollbar(
                "which-key-scrollbar",
                co,
                &self.which_key_scroll.clone(),
                cx,
            ))
    }

    /// The activity rail: the app's panes as a column of labelled
    /// buttons down the left edge, each carrying its chord.
    ///
    /// The chords were the only door into the subsystems, so the ones
    /// nobody had memorised did not exist: the sniffer, the assistant,
    /// the link graph, the journal, the job list — and pairing, which
    /// had no clickable entry point in any shell at all. A glyph in the
    /// status bar was not enough; a name, a key and a place that never
    /// moves is.
    ///
    /// The list itself is [`closure_shell_core::Destination`] data, so
    /// the TUI and the web tier can grow the same rail without
    /// re-deciding what is in it (I4/G5a).
    fn rail(&self, co: Colors, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .debug_selector(|| "rail".to_owned())
            .id("rail")
            .flex()
            .flex_col()
            .gap_1()
            .flex_none()
            .py_2()
            .px_1()
            .w(px(146.0))
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(co.panel))
            .border_r_1()
            .border_color(rgb(co.border))
            .children(
                self.destinations()
                    .into_iter()
                    .map(|dest| Self::rail_button(co, self.app.zoom(), dest, cx)),
            )
    }

    /// One rail button: icon, name, live badge, and the chord that does
    /// the same thing from the keyboard.
    fn rail_button(
        co: Colors,
        zoom: f32,
        dest: closure_shell_core::Destination,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let command = dest.command;
        let tooltip = dest.chord.map_or_else(
            || dest.label.to_owned(),
            |chord| format!("{}  [{chord}]", dest.label),
        );
        let fg = if dest.active { co.accent } else { co.fg };
        let mut button = div()
            .debug_selector(move || format!("rail-{}", dest.id))
            .id(dest.id)
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .py_1()
            .rounded_md()
            .text_size(sz_at(12.0, zoom))
            .text_color(rgb(fg))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(co.hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut Self, _ev, _w, cx| {
                    this.click(command, cx);
                }),
            )
            .tooltip(move |_w, cx| {
                let text = tooltip.clone();
                cx.new(move |_| Hint { text, co, zoom }).into()
            })
            // The open pane is marked by a filled bar down its left
            // edge, not by colour alone: at a glance the rail has to
            // answer "where am I?" without asking the eye to compare
            // two greys.
            .child(
                div()
                    .w(px(2.0))
                    .h(px(14.0))
                    .rounded_sm()
                    .bg(rgb(if dest.active { co.accent } else { co.panel })),
            )
            .child(div().text_size(sz_at(12.0, zoom)).child(dest.icon))
            .child(div().flex_grow().child(dest.label));
        if dest.active {
            button = button.bg(rgb(co.selection));
        }
        if let Some(badge) = dest.badge {
            button = button.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .text_size(sz_at(10.0, zoom))
                    .bg(rgb(if dest.urgent { co.error } else { co.hover }))
                    .text_color(rgb(if dest.urgent { co.bg } else { co.muted }))
                    .child(badge),
            );
        } else if let Some(chord) = dest.chord {
            // Every command shows its keybinding where the command is
            // (the vision's rule), so the rail reads as a keymap you
            // can click.
            button = button.child(
                div()
                    .text_size(sz_at(9.0, zoom))
                    .text_color(rgb(co.muted))
                    .child(chord.to_owned()),
            );
        }
        button
    }

    /// The status line: the message on the left, the subsystem
    /// indicators bottom-right, VS Code style.
    ///
    /// Each indicator reports live state, is a click target for the
    /// surface it belongs to, and carries the chord that opens it —
    /// which is also how you find out that a sniffer, an LLM gate and
    /// a conflict resolver exist at all.
    fn status_bar(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        use closure_shell_core::IndicatorLevel as L;
        let zoom = self.app.zoom();
        div()
            .debug_selector(|| "status-bar".to_owned())
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .bg(rgb(co.panel))
            .text_size(self.sz(11.0))
            .child(
                div()
                    .flex_grow()
                    .overflow_hidden()
                    .text_color(rgb(co.muted))
                    .child(self.app.status().to_owned()),
            )
            // A clock you cannot see is a clock you forget to stop
            // (Q3-V3): the running one sits beside the indicators,
            // and clicking it jumps to the note it is running on.
            .children(self.app.running_clock(&self.shell).map(|label| {
                div()
                    .id("running-clock")
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(rgb(co.accent))
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _ev, _w, cx| {
                            this.click("clock-goto", cx);
                        }),
                    )
                    .child(label)
            }))
            .children(self.app.indicators(&self.shell).into_iter().map(|item| {
                let colour = match item.level {
                    L::Idle => co.muted,
                    L::Active => co.accent,
                    L::Warn => co.warning,
                };
                let mut chip = div()
                    .id(item.id)
                    .px_2()
                    .rounded_md()
                    .text_color(rgb(colour))
                    .child(item.label);
                if let Some(command) = item.command {
                    // Hovering explains what it is — in a real tooltip.
                    // It used to *overwrite the status line* on every
                    // mouse move, which destroyed whatever the last
                    // command had reported and never put it back.
                    let hint = item.chord.map_or_else(
                        || item.tooltip.clone(),
                        |c| format!("{}  [{c}]", item.tooltip),
                    );
                    chip = chip
                        .cursor_pointer()
                        .hover(move |s| s.bg(rgb(co.hover)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                this.click(command, cx);
                            }),
                        )
                        .tooltip(move |_w, cx| {
                            let hint = hint.clone();
                            cx.new(move |_| Hint {
                                text: hint,
                                co,
                                zoom,
                            })
                            .into()
                        });
                }
                chip
            }))
    }

    /// Toast strip: the newest three feedback items, severity-coloured.
    ///
    /// Deferred and anchored rather than a row in the layout. As a flex
    /// child it took its height from whether it had anything in it, so
    /// every toast appearing and expiring — five seconds apart, all day
    /// — pushed the outline and the editor down and let them back up.
    /// `None` when there is nothing to say, so it costs no element.
    fn toast_overlay(&self, co: Colors) -> Option<gpui::Deferred> {
        if self.feedback.items().is_empty() {
            return None;
        }
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(gpui::point(px(12.0), px(52.0)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(self.toast_strip(co)),
            )
            .with_priority(1),
        )
    }

    /// The toasts themselves.
    fn toast_strip(&self, co: Colors) -> gpui::Div {
        use closure_shell_core::FeedbackKind as K;
        div()
            .debug_selector(|| "toast-strip".to_owned())
            .flex()
            .flex_col()
            .gap_1()
            .children(self.feedback.items().iter().rev().take(3).map(|item| {
                let col = match item.kind {
                    K::Error => co.error,
                    K::Warning => co.warning,
                    K::Success => co.success,
                    K::Info | K::Progress(_) => co.accent,
                };
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    // Opaque, not tinted: it floats over the panes now,
                    // and a translucent toast over body text is a
                    // smear rather than a message.
                    .bg(rgb(mix_u32(co.panel, col, 48)))
                    .border_1()
                    .border_color(rgb(col))
                    .text_color(rgb(col))
                    .text_size(self.sz(11.0))
                    .child(format!("⚑ {}", item.text))
            }))
    }

    /// The window's top bar: title, the clickable mode chip, and the
    /// Notion-style capture and palette buttons.
    fn header_bar(&self, co: Colors, cx: &Context<Self>) -> gpui::Div {
        let button = |label: String, colour: u32, command: &'static str| {
            div()
                .debug_selector(move || format!("header-{command}"))
                .px_2()
                .rounded_md()
                .bg(rgb(co.panel))
                .text_size(self.sz(11.0))
                .text_color(rgb(colour))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(co.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Self, _ev, _w, cx| this.click(command, cx)),
                )
                .child(label)
        };
        div()
            .debug_selector(|| "header-bar".to_owned())
            .flex()
            .items_center()
            .px_3()
            .py_1()
            .gap_2()
            .child(div().text_color(rgb(co.accent)).text_lg().child("closure"))
            .child(button(
                format!("{:?}", self.app.input_mode()),
                co.warning,
                "cycle-mode",
            ))
            .child(div().flex_grow())
            .child(button("＋ capture".to_owned(), co.success, "capture-start"))
            .child(button("❯ palette".to_owned(), co.accent, "palette"))
    }

    /// The right-click context menu, anchored where the click landed.
    ///
    /// Entries come from [`closure_shell_core::context_menu`], so each
    /// one shows the chord that runs it in the active mode — the mouse
    /// path teaches the keyboard path.
    fn context_menu_overlay(&self, co: Colors, cx: &Context<Self>) -> Option<gpui::Deferred> {
        let (position, target) = self.menu?;
        let items = closure_shell_core::context_menu(target, self.app.input_mode());
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .debug_selector(|| "context-menu".to_owned())
                            .flex()
                            .flex_col()
                            .min_w(px(230.0))
                            .py_1()
                            .rounded_md()
                            // A menu has to swallow the mouse: without
                            // this, hovering an entry also hovers the
                            // outline row it happens to be covering,
                            // and both light up.
                            .occlude()
                            .bg(rgb(co.bg))
                            .border_1()
                            .border_color(rgb(co.border))
                            .text_size(self.sz(12.0))
                            .children(items.into_iter().map(|item| {
                                let command = item.action.command().to_owned();
                                let selector = format!("menu-{command}");
                                div()
                                    .debug_selector(move || selector)
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(rgb(co.hover)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                            let command = command.clone();
                                            this.menu = None;
                                            this.click(&command, cx);
                                        }),
                                    )
                                    .child(
                                        div().flex_grow().text_color(rgb(co.fg)).child(item.label),
                                    )
                                    // The binding, always, on every entry.
                                    .child(
                                        div()
                                            .text_color(rgb(co.accent))
                                            .text_size(self.sz(11.0))
                                            .child(item.action.chord().to_owned()),
                                    )
                            })),
                    ),
            )
            .with_priority(2),
        )
    }
}

/// A whole body classified line by line, shared so a repaint that
/// changed no text costs a refcount bump.
#[cfg(feature = "gpui")]
type HighlightedBody = std::rc::Rc<Vec<Vec<(BodySpan, String)>>>;

/// Textual input from the platform's input method.
///
/// The window read `KeyDownEvent.key_char` and nothing else, so a dead
/// key (`´` then `e` for `é`), a compose sequence and every CJK input
/// method produced no text at all — closure could not type half the
/// characters its user's keyboard makes. gpui routes those through this
/// trait instead of the key path, in UTF-16 code units; the conversion
/// to the byte offsets [`closure_shell_core::BodyEditor`] addresses is
/// [`byte_for_utf16`] and [`utf16_for_byte`].
///
/// Only the body editor takes it. The one-line fields are keystroke
/// surfaces in the core with no range to replace, and Browse would
/// read composed text as chords.
#[cfg(feature = "gpui")]
impl gpui::EntityInputHandler for GpuiView {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let buf = self.app.body_buffer();
        let start = byte_for_utf16(buf, range.start);
        let end = byte_for_utf16(buf, range.end);
        *adjusted = Some(utf16_for_byte(buf, start)..utf16_for_byte(buf, end));
        buf.get(start..end).map(ToOwned::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        if !self.editing_text() {
            return None;
        }
        let buf = self.app.body_buffer();
        let (lo, hi) = self.app.body_selection().map_or_else(
            || (self.body_byte(), self.body_byte()),
            |(a, b)| {
                if a <= b { (a, b) } else { (b, a) }
            },
        );
        Some(gpui::UTF16Selection {
            range: utf16_for_byte(buf, lo)..utf16_for_byte(buf, hi),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        let buf = self.app.body_buffer();
        let m = self.marked.clone()?;
        Some(utf16_for_byte(buf, m.start)..utf16_for_byte(buf, m.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Abandoning a composition removes the provisional text with
        // it; leaving it behind would commit something the user backed
        // out of.
        if let Some(range) = self.marked.take() {
            self.app.body_replace_range(range, "");
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_text() {
            return;
        }
        let target = self.resolve_ime_range(range);
        self.marked = None;
        self.app.body_replace_range(target, text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _new_selected: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_text() {
            return;
        }
        let target = self.resolve_ime_range(range);
        let start = target.start;
        self.app.body_replace_range(target, text);
        // Still composing: remember where the provisional text sits so
        // the next hand-off replaces it rather than appending to it.
        self.marked = (!text.is_empty()).then(|| start..start + text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        element_bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        // Where the IME puts its candidate window. The editor pane's
        // bounds are the best answer available without laying the
        // cursor's glyph out again.
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(feature = "gpui")]
impl GpuiView {
    /// Whether composed text belongs in the body editor right now.
    fn editing_text(&self) -> bool {
        self.app.surface().is_editor()
            && self.app.body_mode() == closure_shell_core::EditorMode::Insert
    }

    /// The body cursor as a byte offset into the buffer.
    fn body_byte(&self) -> usize {
        let buf = self.app.body_buffer();
        let (line, col) = self.app.body_cursor();
        let start: usize = buf.split('\n').take(line).map(|l| l.len() + 1).sum();
        let text = buf.split('\n').nth(line).unwrap_or_default();
        start + byte_for_col(text, col)
    }

    /// The byte range an IME hand-off should replace: what it asked for,
    /// else the text it is already composing, else the cursor.
    fn resolve_ime_range(&self, range: Option<std::ops::Range<usize>>) -> std::ops::Range<usize> {
        if let Some(r) = range {
            let buf = self.app.body_buffer();
            return byte_for_utf16(buf, r.start)..byte_for_utf16(buf, r.end);
        }
        self.marked.clone().unwrap_or_else(|| {
            let at = self.body_byte();
            at..at
        })
    }

    /// Poll the vault for files changed underneath it, and re-read
    /// `config.org` for a theme or input mode that changed with them.
    ///
    /// closure is local-first, which means the files are the API: an
    /// Emacs on the same vault, a `git pull`, an inbound sync round all
    /// write org that the window then knew nothing about until the user
    /// navigated hard enough to force a re-read. Each armed poll carries
    /// a generation and only the newest re-arms, so the loop cannot
    /// fork.
    fn arm_reload(&mut self, cx: &Context<Self>) {
        /// How often the vault is checked. Long enough to be free at
        /// idle, short enough that an external edit shows up before the
        /// user wonders whether it worked.
        const EVERY: std::time::Duration = std::time::Duration::from_millis(1500);
        self.reload_gen = self.reload_gen.wrapping_add(1);
        let generation = self.reload_gen;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(EVERY).await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.reload_gen != generation {
                            return false;
                        }
                        this.reload_vault(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
        .detach();
    }

    /// One reload pass: reparse what changed on disk, then re-read the
    /// config if anything did.
    fn reload_vault(&mut self, cx: &mut Context<Self>) {
        // Never while a body edit is open: reparsing under the editor
        // would swap the text out from under the cursor, and the buffer
        // is the user's, not the file's.
        if self.app.body_dirty() {
            return;
        }
        let Ok(reparsed) = self.shell.vault.reload_incremental() else {
            return;
        };
        if reparsed == 0 {
            return;
        }
        let root = self.shell.vault.root().to_owned();
        self.theme = resolve_theme(&root);
        self.app
            .set_status(format!("{reparsed} file(s) changed on disk — reloaded"));
        cx.notify();
    }
}

/// UTF-16 code-unit offset for a byte offset into `text`.
///
/// The platform's input methods count in UTF-16 while the editor
/// counts in bytes, and a conversion that is wrong by one puts a
/// composed character in the wrong place — or panics on a slice that is
/// not a char boundary. Offsets past the end clamp.
#[must_use]
pub fn utf16_for_byte(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text.get(..byte)
        .unwrap_or(text)
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// Byte offset for a UTF-16 code-unit offset into `text` — the inverse
/// of [`utf16_for_byte`].
///
/// An offset landing inside a surrogate pair rounds down to that
/// character's start rather than splitting it, so the result is always
/// a valid slice index.
#[must_use]
pub fn byte_for_utf16(text: &str, utf16: usize) -> usize {
    let mut seen = 0usize;
    for (byte, c) in text.char_indices() {
        // Round *down*: an offset that lands inside a surrogate pair
        // belongs to the character that started before it, and handing
        // back the byte after it would split the pair on the next
        // slice.
        if seen + c.len_utf16() > utf16 {
            return byte;
        }
        seen += c.len_utf16();
    }
    text.len()
}

/// Where one painted body line sits: its index, its byte offset and
/// length in the buffer, and the column the pane is scrolled to.
#[cfg(feature = "gpui")]
#[derive(Clone, Copy)]
struct LineGeom {
    /// Zero-based line index in the buffer.
    ln: usize,
    /// The line's byte offset in the buffer.
    line_start: usize,
    /// The line's byte length.
    line_len: usize,
    /// First visible column ([`h_scroll_start`]) — or, when wrapping,
    /// the column this visual row starts at.
    h_start: usize,
    /// How many columns this row paints, when wrapping. `None` paints
    /// to the end of the line and lets the pane clip it, which is the
    /// unwrapped behaviour.
    cols: Option<usize>,
}

/// A hover explanation, in the theme's own colours.
///
/// gpui's core ships no tooltip widget (Zed's lives in its `ui` crate),
/// and a tooltip has to be a view — so this is the smallest one that
/// can be: a line of text on a panel.
#[cfg(feature = "gpui")]
struct Hint {
    text: String,
    co: Colors,
    zoom: f32,
}

#[cfg(feature = "gpui")]
impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(self.co.panel))
            .border_1()
            .border_color(rgb(self.co.border))
            .text_size(sz_at(11.0, self.zoom))
            .text_color(rgb(self.co.fg))
            .child(self.text.clone())
    }
}

/// A one-line prompt with a block caret over the cell its cursor is on.
///
/// The caret is a mark, not a character: painting it as an inserted
/// glyph made the line reflow every time the cursor moved, which is
/// what "weird shift in capture prompt when ctlr+a is pressed" was.
/// Inverse video in the accent colour, the same two colours the body
/// editor's block cursor uses in [`editor_segment`] — one cursor in one
/// shape wherever it appears.
#[cfg(feature = "gpui")]
fn caret_text(co: Colors, text: &str, cursor: usize) -> gpui::StyledText {
    let (laid, cell) = field_caret_cell(text, cursor);
    gpui::StyledText::new(gpui::SharedString::from(laid)).with_highlights([(
        cell,
        gpui::HighlightStyle {
            color: Some(rgb(co.bg).into()),
            background_color: Some(rgb(co.accent).into()),
            ..Default::default()
        },
    )])
}

/// One hit-testable run of body text. `col_offset` is the char column
/// `text` starts at, so a segment painted after the INSERT caret still
/// reports absolute columns back to the editor.
#[cfg(feature = "gpui")]
fn editor_segment(
    co: Colors,
    ln: usize,
    col_offset: usize,
    text: String,
    runs: Vec<StyledRun>,
    cx: &Context<GpuiView>,
) -> gpui::Div {
    let styled = gpui::StyledText::new(text.clone()).with_highlights(runs.into_iter().map(
        |(range, kind, mark)| {
            let plain = rgb(span_color(co, kind)).into();
            let (fg, bg) = match mark {
                None => (plain, None),
                // Inverse video, in the accent colour rather than plain
                // foreground: the block cursor and the INSERT bar are
                // the same cursor in two shapes, and drawing one white
                // and the other blue made them look like two different
                // things.
                Some(Emphasis::Cursor) => (rgb(co.bg).into(), Some(rgb(co.accent).into())),
                Some(Emphasis::Selection) => (plain, Some(rgb(co.selection_text).into())),
                // A search hit is a wash, not an inversion: there can
                // be a dozen on screen and the cursor still has to be
                // the thing your eye finds first.
                Some(Emphasis::Search) => (plain, Some(rgb(co.warning).into())),
            };
            (
                range,
                gpui::HighlightStyle {
                    color: Some(fg),
                    background_color: bg,
                    ..decorated(kind)
                },
            )
        },
    ));
    // Clone the layout handle out before the element is moved; it is
    // what the mouse handlers below hit-test against.
    let layout = styled.layout().clone();
    let drag_layout = layout.clone();
    let click_text = text.clone();
    div()
        .debug_selector(move || format!("body-line-{ln}"))
        .child(styled)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |this: &mut GpuiView, ev: &gpui::MouseDownEvent, _w, cx| {
                    let byte = layout
                        .index_for_position(ev.position)
                        .unwrap_or_else(|i| i)
                        .min(click_text.len());
                    // Ctrl-click follows an org link, the way it does
                    // in every editor. A plain click still places the
                    // cursor — the text stays editable rather than
                    // turning into a page of hyperlinks.
                    if ev.modifiers.control
                        && let Some(link) = line_links(&click_text)
                            .into_iter()
                            .find(|l| l.range.contains(&byte))
                    {
                        this.follow_link(&link.target, cx);
                        cx.stop_propagation();
                        return;
                    }
                    let col = col_offset + col_for_byte(&click_text, byte);
                    if ev.click_count >= 2 {
                        this.app.body_double_click(ln, col);
                    } else {
                        this.app.body_click(ln, col);
                    }
                    cx.stop_propagation();
                    cx.notify();
                },
            ),
        )
        // G2: dragging with the left button held extends the charwise
        // VISUAL selection (BodyEditor::drag_to).
        .on_mouse_move(cx.listener(
            move |this: &mut GpuiView, ev: &gpui::MouseMoveEvent, _w, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    let col = col_offset + hit_col(&drag_layout, &text, ev.position);
                    this.app.body_drag(ln, col);
                    cx.notify();
                }
            },
        ))
}

/// A draggable scrollbar for the pane tracked by `handle`.
///
/// gpui ships no scrollbar widget, so this is one: a track carrying a
/// thumb sized and placed by [`thumb_geometry`], with click-and-drag
/// anywhere on the track mapped back through [`track_fraction`] and
/// [`scroll_for_track_fraction`] — the thumb centres on the pointer, so
/// it stays under the finger dragging it. A pane whose content fits
/// gets an empty track, so the gutter width never shifts under the
/// mouse.
///
/// The handle's own bounds are the track's: the bar is painted as the
/// scrolled pane's sibling with the same height.
///
/// The gestures read the handle *when the mouse arrives*, not when the
/// element is built. A pane's measurements only exist once it has been
/// laid out, so a bar built from them was a frame behind its own pane:
/// open the headline list and the bar beside it was inert — it had
/// been built while the pane still held the previous surface's content
/// — until some unrelated repaint armed it. The thumb is still drawn
/// from the build-time snapshot, which is only ever a frame stale and
/// corrects itself on the next one.
#[cfg(feature = "gpui")]
fn scrollbar(
    name: &'static str,
    co: Colors,
    handle: &gpui::ScrollHandle,
    cx: &Context<GpuiView>,
) -> gpui::Div {
    /// Keeps the thumb grabbable on a huge vault.
    const MIN_THUMB: f32 = 0.06;
    let jump = {
        let handle = handle.clone();
        move |y: gpui::Pixels| {
            let bounds = handle.bounds();
            let viewport = f32::from(bounds.size.height);
            let content = viewport + f32::from(handle.max_offset().height);
            // gpui scroll offsets run negative as content moves up.
            let scroll = -f32::from(handle.offset().y);
            // A pane whose content fits has nothing to drag.
            let Some(thumb) = thumb_geometry(viewport, content, scroll, MIN_THUMB) else {
                return;
            };
            let fraction = track_fraction(
                f32::from(y),
                f32::from(bounds.origin.y),
                viewport,
                thumb.height,
            );
            let offset = scroll_for_track_fraction(viewport, content, fraction);
            handle.set_offset(gpui::point(px(0.0), px(-offset)));
        }
    };
    let drag_jump = jump.clone();
    let track = div()
        .debug_selector(move || name.to_owned())
        .w(px(10.0))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(mix_u32(co.bg, co.panel, 160)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |_this: &mut GpuiView, ev: &gpui::MouseDownEvent, _w, cx| {
                    jump(ev.position.y);
                    cx.stop_propagation();
                    cx.notify();
                },
            ),
        )
        .on_mouse_move(cx.listener(
            move |_this: &mut GpuiView, ev: &gpui::MouseMoveEvent, _w, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    drag_jump(ev.position.y);
                    cx.notify();
                }
            },
        ));
    let bounds = handle.bounds();
    let viewport = f32::from(bounds.size.height);
    let content = viewport + f32::from(handle.max_offset().height);
    let scroll = -f32::from(handle.offset().y);
    // A pane whose content fits gets an empty track, so the gutter
    // width never shifts under the mouse — and no pointer cursor,
    // because there is nothing there to grab.
    let Some(thumb) = thumb_geometry(viewport, content, scroll, MIN_THUMB) else {
        return track;
    };
    track
        .cursor_pointer()
        .child(div().h(gpui::relative(thumb.top)))
        .child(
            div()
                .h(gpui::relative(thumb.height))
                .w_full()
                .rounded_sm()
                .bg(rgb(co.muted))
                .hover(move |s| s.bg(rgb(co.accent))),
        )
}

/// The window's font: the theme's stack as gpui wants it — one family
/// plus an ordered fallback list.
///
/// gpui's `font_family()` takes a single family name, and the window
/// used to hand it the theme's whole CSS-shaped stack. No font is called
/// `"Maple Mono NF, JetBrains Mono, ui-monospace, monospace"`, so the
/// lookup failed and every glyph in the app came from whatever the
/// platform substituted — which is why the shell never looked like the
/// font it declared.
///
/// One font for the whole window, mono: the outline's markers, the
/// editor gutter, the block cursor and org tables all want cells that
/// line up, and a Nerd Font covers the rail's glyphs.
#[cfg(feature = "gpui")]
#[must_use]
pub fn app_font(theme: closure_shell_core::Theme) -> gpui::Font {
    let stack = closure_shell_core::font_stack(theme.typography.mono_family);
    let mut names = stack.into_iter();
    // A theme that names no font is a broken theme, not a window with
    // no text in it.
    let family = names.next().unwrap_or("monospace");
    let fallbacks: Vec<String> = names.map(ToOwned::to_owned).collect();
    gpui::Font {
        fallbacks: (!fallbacks.is_empty()).then(|| gpui::FontFallbacks::from_fonts(fallbacks)),
        ..gpui::font(family)
    }
}

/// The weight, slant and rules a span kind carries, as gpui spells
/// them ([`span_decoration`] is the toolkit-free half).
///
/// Emphasis has to be weight and slant, not hue: `*bold*` drawn in a
/// different colour is not bold, it is a colour — and a paragraph with
/// six kinds of markup in it would become a colour chart rather than
/// prose.
#[cfg(feature = "gpui")]
fn decorated(kind: BodySpan) -> gpui::HighlightStyle {
    let d = span_decoration(kind);
    gpui::HighlightStyle {
        font_weight: d.bold.then_some(gpui::FontWeight::BOLD),
        font_style: d.italic.then_some(gpui::FontStyle::Italic),
        strikethrough: d.strike.then(|| gpui::StrikethroughStyle {
            thickness: px(1.0),
            color: None,
        }),
        underline: d.underline.then(|| gpui::UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        ..Default::default()
    }
}

/// Theme colour for a body-editor span kind. Shared by the editor
/// pane and the read-only detail preview so both read identically.
#[cfg(feature = "gpui")]
const fn span_color(co: Colors, kind: BodySpan) -> u32 {
    match kind {
        // Emphasis is weight and slant first (see [`span_decoration`]);
        // bold and italic keep the prose colour so a paragraph does
        // not turn into a colour chart.
        BodySpan::Plain | BodySpan::Bold | BodySpan::Italic | BodySpan::Underline => co.fg,
        // A tag run is de-emphasised for the same reason meta lines are:
        // it is bookkeeping beside the sentence, not the sentence.
        BodySpan::Meta | BodySpan::Comment | BodySpan::Strike | BodySpan::Tags => co.muted,
        // A drawer and an open TODO are both "unfinished business the
        // eye should catch first".
        BodySpan::Drawer | BodySpan::Todo => co.error,
        BodySpan::Keyword | BodySpan::Link => co.accent,
        // Literals and finished work read as settled.
        BodySpan::Literal | BodySpan::Done => co.success,
        BodySpan::Table => co.heading2,
        BodySpan::InlineCode | BodySpan::Verbatim | BodySpan::Example => co.code,
        BodySpan::Quote => co.heading3,
        // Org cycles its outline faces by level; the palette has three
        // heading colours, so depth 4 reads like depth 1 — which is
        // what org does too once it runs out of faces.
        BodySpan::Headline(level) => match level % 3 {
            1 => co.accent,
            2 => co.heading2,
            _ => co.heading3,
        },
        BodySpan::Priority => co.warning,
    }
}

/// Resolve a window-space mouse position to a char column in `text`
/// through gpui's text layout.
///
/// `index_for_position` returns `Err` with the nearest index when the
/// position falls outside the glyphs — a click in a line's empty tail
/// — which is exactly the "park at the line end" behaviour, so both
/// arms carry a usable offset.
#[cfg(feature = "gpui")]
fn hit_col(layout: &gpui::TextLayout, text: &str, position: gpui::Point<gpui::Pixels>) -> usize {
    let byte = layout.index_for_position(position).unwrap_or_else(|i| i);
    col_for_byte(text, byte)
}

/// A generic clickable list row (agenda / blocks / backlinks).
#[cfg(feature = "gpui")]
fn list_row(
    co: Colors,
    zoom: f32,
    selected: bool,
    text: String,
    listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .text_size(sz_at(13.0, zoom))
        .bg(rgb(if selected { co.selection } else { co.bg }))
        .hover(move |s| s.bg(rgb(if selected { co.selection } else { co.hover })))
        .on_mouse_down(MouseButton::Left, listener)
        .child(text)
}

/// The month's name, for the date picker's header.
#[must_use]
pub const fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

/// Give an element a right-click context menu for `target`.
///
/// The menu itself is [`closure_shell_core::context_menu`], which has
/// always known about three targets; the window only ever wired the
/// outline row, so a right-click in the editor or on a detail field
/// dismissed the menu it never opened. Propagation stops here so the
/// innermost target wins — the body's menu over the detail pane's.
#[cfg(feature = "gpui")]
fn with_menu<E: gpui::InteractiveElement>(
    el: E,
    target: closure_shell_core::ContextTarget,
    cx: &Context<GpuiView>,
) -> E {
    el.on_mouse_down(
        MouseButton::Right,
        cx.listener(
            move |this: &mut GpuiView, ev: &gpui::MouseDownEvent, _w, cx| {
                this.menu = Some((ev.position, target));
                cx.stop_propagation();
                cx.notify();
            },
        ),
    )
}

/// Wrap a detail field so a click begins the matching edit command.
#[cfg(feature = "gpui")]
fn clickable(
    co: Colors,
    inner: gpui::Div,
    command: &'static str,
    cx: &Context<GpuiView>,
) -> gpui::Div {
    div()
        .debug_selector(move || format!("field-{command}"))
        .rounded_sm()
        .px_1()
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(co.hover)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this: &mut GpuiView, _ev, _w, cx| this.click(command, cx)),
        )
        .child(inner)
}

/// Trailing file name of a vault path (the full path stays in the
/// detail pane; rows only need the short name).
#[cfg(feature = "gpui")]
fn short_path(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_owned()
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
    if let Some(s) = &d.scheduled {
        let _ = write!(meta, "SCHEDULED {s} ");
    }
    if let Some(s) = &d.deadline {
        let _ = write!(meta, "DEADLINE {s} ");
    }
    if meta.is_empty() {
        "+ todo".clone_into(&mut meta);
    }
    meta
}

#[cfg(feature = "gpui")]
impl Render for GpuiView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let co = Colors::of(&self.theme);
        let font = app_font(self.theme);
        // Every path that sets a status reaches the toast strip through
        // here, once per frame.
        self.absorb_status(cx);
        // A clock entry is stamped with the minute it was started, so
        // the window keeps the core's idea of now up to date.
        self.app.set_now(&closure_shell_core::now_local());
        self.refresh_title(window);
        self.reveal_cursors();
        // The editor sizes itself from its own measured height, which
        // is last frame's layout. When that moves — the buffer just
        // opened, the rail stepped out of the way, the window resized —
        // this frame paints a stale number of lines, so one more frame
        // is asked for. It settles immediately: the measurement stops
        // moving once the layout does.
        if self.app.surface_beneath().is_editor() {
            let view = self.body_view();
            // The kernel decides where the viewport sits and only the
            // window knows how tall it is, so the measurement is handed
            // over before anything asks to be framed — `C-l` and
            // `zz`/`zt`/`zb` need "the middle of the screen" to mean
            // this screen. Resolving the scroll here rather than in the
            // (borrow-only) paint is also what lets it be sticky:
            // scrolling by the minimum is measured from where the
            // viewport already was.
            self.app.set_body_viewport(view);
            self.app.body_scroll_follow(view);
            if self.painted_view.replace(view) != view {
                // `cx.notify()` inside a render is not another frame —
                // the window is already drawing. This asks for the next
                // one, which is when the new measurement exists.
                window.request_animation_frame();
            }
        }
        // The vault's files are the API, so something else writing them
        // — an Emacs on the same directory, a `git pull`, an inbound
        // sync round — has to reach the window. Armed once, from the
        // first frame.
        if self.reload_gen == 0 {
            self.arm_reload(cx);
        }

        let header = self.header_bar(co, cx);

        let context = self.context_row(co, cx);

        let body = div()
            .flex()
            .flex_row()
            .flex_grow()
            // A flex item's automatic minimum size is its *content*,
            // so this row refused to shrink below the height of the
            // panes inside it: a 300-line body made the right-hand
            // pane 6000px tall, and every measurement taken from it —
            // how many lines the editor paints, where the scrollbar
            // thumb goes, what a page-down moves by — was taken
            // against a viewport the size of the document. This is the
            // only item in the chain that needed saying: below it the
            // panes are row children, sized by `stretch`.
            .min_h(px(0.0));
        let body = self.panes(body, co, cx);

        let status = self.status_bar(co, cx);

        // The bindings panel opens on demand, and always while a chord
        // is in flight — that is the moment it is actually needed.
        let show_keys = self.app.which_key_open() || !self.app.pending_chord().is_empty();

        let mut root = div()
            .key_context("ClosureGpui")
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(Self::on_key))
            // A click anywhere outside an open menu dismisses it, the
            // way every desktop menu behaves.
            //
            // In the *capture* phase, because every handler that places
            // a caret has to stop propagation to keep the click from
            // travelling on — the body lines, the scrollbars. In the
            // bubble phase this never ran for them, so a right-click in
            // the editor left a menu hanging over the text no click
            // could put away. The menu itself `.occlude()`s, so a click
            // on an entry does not reach this hitbox at all: the entry
            // still gets to run its command.
            .capture_any_mouse_down(cx.listener(
                |this: &mut Self, ev: &gpui::MouseDownEvent, _w, cx| {
                    // Right-click is how a menu is *opened*; dismissing
                    // here would race the handler about to open one.
                    if ev.button == MouseButton::Left && this.menu.take().is_some() {
                        cx.notify();
                    }
                },
            ))
            // A row drag released anywhere but on a row — over the side
            // pane, in the empty space under the last row — never
            // reached a row's mouse-up handler, so the gesture stayed
            // armed: the next hover retargeted it and the next click
            // finished a move the user had abandoned.
            // Dragging the outline's edge. The move handler lives on
            // the root because the pointer leaves the 6px handle the
            // instant it starts moving.
            .on_mouse_move(
                cx.listener(|this: &mut Self, ev: &gpui::MouseMoveEvent, _w, cx| {
                    let Some(grab) = this.outline_drag else {
                        return;
                    };
                    if !ev.dragging() {
                        this.outline_drag = None;
                        return;
                    }
                    this.outline_w =
                        (f32::from(ev.position.x) - grab).clamp(OUTLINE_W_MIN, OUTLINE_W_MAX);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _ev, _w, cx| {
                    this.outline_drag = None;
                    if this.drag.source().is_some() {
                        this.drag.cancel();
                        cx.notify();
                    }
                }),
            )
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(rgb(co.bg))
            .text_color(rgb(co.fg))
            .font(font)
            .child(header)
            .child(context)
            // The tab strip (Q1-B5) appears the moment a second buffer
            // exists and not before: one tab is furniture.
            .children(self.tab_strip(co, cx))
            .child(body)
            .child(status);
        if show_keys {
            root = root.child(self.which_key_panel(co, cx));
        }
        root = root.child(self.footer(co, cx));
        if let Some(toasts) = self.toast_overlay(co) {
            root = root.child(toasts);
        }
        if let Some(menu) = self.context_menu_overlay(co, cx) {
            root = root.child(menu);
        }
        if let Some(palette) = self.palette_overlay(co, cx) {
            root = root.child(palette);
        }
        root
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
