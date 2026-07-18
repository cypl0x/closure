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

/// Per-char click cells for the body pane: each char paired with its
/// char column. The seam that gives mouse clicks in-word precision.
#[must_use]
pub fn char_cells(text: &str, start_col: usize) -> Vec<(String, usize)> {
    text.chars()
        .enumerate()
        .map(|(i, c)| (c.to_string(), start_col + i))
        .collect()
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
    use closure_tree_sitter::{HighlightKind, Highlighter as _, KeywordHighlighter};
    let mut out = Vec::new();
    let mut in_src: Option<KeywordHighlighter> = None;
    for line in body.split('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#+") {
            let lower = trimmed.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("#+begin_src") {
                in_src = Some(KeywordHighlighter::for_language(rest.trim()));
            } else if lower.starts_with("#+end_src") {
                in_src = None;
            }
            out.push(vec![(BodySpan::Meta, line.to_owned())]);
        } else if let Some(hl) = &in_src {
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
        } else if trimmed.starts_with(':')
            && (trimmed.ends_with(':')
                || trimmed
                    .split_once(' ')
                    .is_some_and(|(k, _)| k.ends_with(':')))
        {
            out.push(vec![(BodySpan::Drawer, line.to_owned())]);
        } else {
            out.push(vec![(BodySpan::Plain, line.to_owned())]);
        }
    }
    out
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

/// Launch the gpui desktop window against the vault at `vault_path`.
/// Blocks until the window closes.
///
/// # Errors
///
/// Returns the vault open error as a string; window/runtime failures
/// surface through gpui's own panics on the UI thread.
#[cfg(feature = "gpui")]
pub fn run(vault_path: &Path) -> Result<(), String> {
    let vault = Vault::open(vault_path).map_err(|e| format!("{e}"))?;
    let theme = resolve_theme(vault_path);
    let input_mode = resolve_input_mode(vault_path);
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1080.0), px(720.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| GpuiView {
                    shell: Shell::new(vault),
                    app: ModalApp::new(input_mode),
                    theme,
                    focus_handle: cx.focus_handle(),
                    feedback: closure_shell_core::Feedback::default(),
                    last_status: String::new(),
                    popup_gen: 0,
                    drag: closure_shell_core::DragReorder::default(),
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

/// Lines the body-editor pane paints per frame (G5 wheel viewport).
#[cfg(feature = "gpui")]
const BODY_VIEW: usize = 40;

/// gpui view: owns the kernel-side [`Shell`] and the pure [`ModalApp`]
/// editor state, plus a focus handle so the root receives key events.
#[cfg(feature = "gpui")]
struct GpuiView {
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
        self.app
            .on_key(&mut self.shell, &ks.key, m.control, m.alt, text);
        self.absorb_status();
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

    /// Feed a changed status line through [`status_toast`] into the
    /// shared feedback queue (the toast strip's only source).
    fn absorb_status(&mut self) {
        let status = self.app.status().to_owned();
        if status != self.last_status {
            if let Some((level, text)) = status_toast(&status) {
                self.feedback.notify(level, text);
            }
            self.last_status = status;
        }
    }

    /// Run a command from a mouse affordance (which-key chip, detail
    /// field, header button) — the same dispatch the chords use (I8).
    fn click(&mut self, command: &str, cx: &mut Context<Self>) {
        self.app.run(&mut self.shell, command);
        self.absorb_status();
        if self.app.should_quit() {
            cx.quit();
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let dy = match ev.delta {
            ScrollDelta::Lines(l) => l.y,
            ScrollDelta::Pixels(p) => f32::from(p.y) / 20.0,
        };
        // Wheel moves the viewport, not the cursor (L4); selection
        // movement reclaims the view.
        #[allow(clippy::cast_possible_truncation)]
        let steps = dy.abs().ceil().min(1000.0) as i32;
        let delta = if dy < 0.0 { steps } else { -steps };
        self.app.scroll_by(delta, &self.shell, 40);
        cx.notify();
    }

    /// G5: wheel over the body-editor pane scrolls its own viewport
    /// (`body_scroll_by`), not the outline; same delta convention as
    /// [`Self::on_scroll`].
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
        self.app.body_scroll_by(delta, BODY_VIEW);
        cx.stop_propagation();
        cx.notify();
    }

    /// Context line describing the active surface (with the live input
    /// buffer + caret for the typing surfaces).
    fn context_line(&self) -> String {
        let n = self.app.rows(&self.shell).len();
        match self.app.surface() {
            ModalSurface::Browse => format!("{n} headline(s)"),
            ModalSurface::Search => self.app.search_context(&self.shell),
            ModalSurface::Capture => format!("＋ capture: {}▏", self.app.capture_buffer()),
            ModalSurface::Rename => format!("✎ rename: {}▏", self.app.field_buffer()),
            ModalSurface::AddSibling => format!("＋ add: {}▏", self.app.field_buffer()),
            ModalSurface::TagsEdit => format!("✎ tags: {}▏", self.app.field_buffer()),
            ModalSurface::PropertyEdit => format!("✎ prop: {}▏", self.app.field_buffer()),
            ModalSurface::Palette => format!("❯ {}▏", self.app.field_buffer()),
            ModalSurface::EditBody => "✎ body — C-Enter save, Esc cancel".to_owned(),
            ModalSurface::Backlinks => "backlinks — Esc back".to_owned(),
            ModalSurface::Agenda => "agenda — RET jump, Esc back".to_owned(),
            ModalSurface::Blocks => "src blocks — RET jump, Esc back".to_owned(),
            ModalSurface::UndoHistory => "undo history — Esc back".to_owned(),
        }
    }

    /// The left outline list (Browse/Search and the typing surfaces
    /// that keep the tree visible).
    fn rows_pane(&self, co: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        const PAGE: usize = 40;
        let (offset, rows) = self.app.view_window(&self.shell, PAGE);
        let selected = self.app.selected();
        div()
            .flex()
            .flex_col()
            .w(px(420.0))
            .min_w(px(300.0))
            .border_r_1()
            .border_color(rgb(co.border))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .children(rows.into_iter().enumerate().map(|(vis, row)| {
                let i = offset + vis;
                let folded = closure_shell_core::is_row_folded(&self.shell, &row.id);
                let is_sel = i == selected;
                let indent = f32::from(row.level.saturating_sub(1)) * 14.0;
                let (todo_col, glyph) = match row.todo.as_deref() {
                    Some("DONE" | "CANCELLED" | "KILL") => (co.success, "●"),
                    Some(_) => (co.error, "○"),
                    None => (co.muted, "·"),
                };
                let mut line = div()
                    .flex()
                    .items_center()
                    .px_2()
                    .py_1()
                    .text_size(px(14.0))
                    .cursor_pointer()
                    .bg(rgb(if is_sel { co.selection } else { co.bg }))
                    .hover(move |s| s.bg(rgb(if is_sel { co.selection } else { co.hover })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev, _w, cx| {
                            this.app.select(i, &this.shell);
                            // G3: a held press starts a potential row drag.
                            this.drag.begin(i);
                            cx.notify();
                        }),
                    )
                    // G3: dragging across rows retargets the drop slot…
                    .on_mouse_move(cx.listener(
                        move |this, ev: &gpui::MouseMoveEvent, _w, cx| {
                            if ev.pressed_button == Some(MouseButton::Left) {
                                this.drag.over(i);
                                cx.notify();
                            }
                        },
                    ))
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
                if is_sel {
                    line = line.border_l_2().border_color(rgb(co.accent));
                }
                line = line.child(div().w(px(indent)));
                // Fold arrow: ▸ folded / ▾ unfolded; click toggles.
                line = line.child(
                    div()
                        .w(px(18.0))
                        .text_color(rgb(if folded { co.accent } else { co.muted }))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev, _w, cx| {
                                this.app.select(i, &this.shell);
                                this.app.run(&mut this.shell, "toggle-fold");
                                cx.notify();
                            }),
                        )
                        .child(if folded { "▸" } else { "▾" }),
                );
                // Status glyph: same click target as the TODO chip.
                line = line.child(
                    div()
                        .w(px(18.0))
                        .text_color(rgb(todo_col))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev, _w, cx| {
                                this.app.select(i, &this.shell);
                                this.app.run(&mut this.shell, "toggle-todo");
                                cx.notify();
                            }),
                        )
                        .child(glyph.to_owned()),
                );
                if let Some(todo) = &row.todo {
                    // Clickable TODO chip: click toggles the state, the
                    // same registry command `t` runs (I8).
                    line = line.child(
                        div()
                            .mr_2()
                            .px_1()
                            .rounded_sm()
                            .text_color(rgb(todo_col))
                            .text_size(px(11.0))
                            .cursor_pointer()
                            .hover(move |s| s.bg(rgb(co.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev, _w, cx| {
                                    this.app.select(i, &this.shell);
                                    this.app.run(&mut this.shell, "toggle-todo");
                                    cx.notify();
                                }),
                            )
                            .child(todo.clone()),
                    );
                }
                line.child(
                    div()
                        .text_color(rgb(co.outline(row.level)))
                        .child(row.title.clone()),
                )
                .child(div().flex_grow())
                .child(
                    div()
                        .text_color(rgb(co.muted))
                        .text_size(px(10.0))
                        .child(short_path(&row.path)),
                )
            }))
    }

    /// Right-hand pane: detail (clickable fields), palette, a list
    /// surface, or the body editor — driven by the active surface.
    fn side_pane(&self, co: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = div()
            .flex()
            .flex_col()
            .flex_grow()
            .px_4()
            .py_3()
            .gap_2()
            .bg(rgb(co.bg));
        match self.app.surface() {
            ModalSurface::Palette => pane.child(self.palette_pane(co, cx)),
            ModalSurface::Agenda => pane.child(self.agenda_pane(co, cx)),
            ModalSurface::UndoHistory => pane.children(
                self.app
                    .undo_history_rows(&self.shell)
                    .into_iter()
                    .map(|r| {
                        div()
                            .flex()
                            .px_2()
                            .py_1()
                            .child(
                                div()
                                    .w(px(f32::from(u16::try_from(r.depth).unwrap_or(u16::MAX))
                                        * 14.0))
                                    .child(""),
                            )
                            .child(
                                div()
                                    .text_color(rgb(if r.is_current {
                                        co.accent
                                    } else {
                                        co.muted
                                    }))
                                    .child(format!(
                                        "{} {}",
                                        if r.is_current { "●" } else { "○" },
                                        r.label
                                    )),
                            )
                    }),
            ),
            ModalSurface::Blocks => pane.children(
                self.app
                    .block_rows(&self.shell)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (path, lang, first))| {
                        list_row(
                            co,
                            i == self.app.selected(),
                            format!("{lang:8} {first}  — {}", short_path(&path)),
                            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                this.app.jump_list_row(&this.shell, i);
                                cx.notify();
                            }),
                        )
                    }),
            ),
            ModalSurface::Backlinks => pane.children(
                self.app
                    .backlink_rows(&self.shell)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (_id, title))| {
                        list_row(
                            co,
                            i == self.app.selected(),
                            format!("⟵ {title}"),
                            cx.listener(move |this: &mut Self, _ev, _w, cx| {
                                this.app.backlink_click(&this.shell, i);
                                cx.notify();
                            }),
                        )
                    }),
            ),
            ModalSurface::EditBody => pane.child(self.editor_pane(co, cx)),
            _ => self.detail_pane(pane, co, cx),
        }
    }

    /// The org-edit-special editor pane: syntax-highlighted lines
    /// ([`highlight_body`]), a real caret at the editor cursor, the
    /// vim mode chip (doom spaceline colours: INSERT green / NORMAL
    /// blue), and the C-n completion popup.
    fn editor_pane(&self, co: Colors, cx: &mut Context<Self>) -> gpui::Div {
        use closure_shell_core::EditorMode;
        let scroll_start = self.app.body_scroll_start(BODY_VIEW);
        let (cur_line, cur_col) = self.app.body_cursor();
        let mode = self.app.body_mode();
        // doom spaceline colours: insert green, normal blue, visual grey-violet.
        let (mode_txt, mode_col) = match mode {
            EditorMode::Insert => ("INSERT", co.success),
            EditorMode::Normal => ("NORMAL", co.accent),
            EditorMode::Visual => ("VISUAL", co.heading3),
            EditorMode::VisualLine => ("V·LINE", co.heading2),
        };
        let span_color = |k: BodySpan| match k {
            BodySpan::Plain => co.fg,
            BodySpan::Meta => co.muted,
            BodySpan::Drawer => co.error,
            BodySpan::Keyword => co.accent,
            BodySpan::Literal => co.success,
            BodySpan::Comment => co.muted,
        };
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .rounded_sm()
                    .bg(rgb(mode_col))
                    .text_color(rgb(co.bg))
                    .text_size(px(11.0))
                    .child(mode_txt),
            )
            .child(
                div()
                    .text_color(rgb(co.muted))
                    .text_size(px(11.0))
                    .child(match mode {
                        EditorMode::Insert => {
                            "type · TAB tempo (<s…) · C-n complete · C-a/e/k/y readline · Esc → NORMAL"
                        }
                        EditorMode::Normal => {
                            "h j k l 0 $ move · i a o insert · x dd yy p · v visual · Esc cancel"
                        }
                        EditorMode::Visual | EditorMode::VisualLine => {
                            "motions extend · y yank · d delete · Esc → NORMAL"
                        }
                    }),
            );
        let mut body = div()
            .flex()
            .flex_col()
            .flex_grow()
            .p_2()
            .bg(rgb(co.panel))
            .rounded_md()
            .text_size(px(13.0))
            .on_scroll_wheel(cx.listener(Self::on_body_scroll));
        let selection = self.app.body_selection();
        let mut line_start = 0usize;
        for (ln, spans) in highlight_body(self.app.body_buffer())
            .into_iter()
            .enumerate()
        {
            let line_len: usize = spans.iter().map(|(_, s)| s.len()).sum();
            // G5: only the wheel-scrolled window of lines is painted;
            // byte offsets still accumulate for the skipped lines.
            if !(scroll_start..scroll_start + BODY_VIEW).contains(&ln) {
                line_start += line_len + 1;
                continue;
            }
            // L5: line-number gutter, current line accented.
            let mut row = div().flex().min_h(px(18.0)).child(
                div()
                    .w(px(34.0))
                    .mr_2()
                    .text_size(px(11.0))
                    .text_color(rgb(if ln == cur_line { co.accent } else { co.muted }))
                    .child(format!("{:>3}", ln + 1)),
            );
            if let Some((lo, hi)) = selection {
                // VISUAL: paint the selected byte range exactly; the
                // selection is the position indicator here, no caret.
                // All split points are char boundaries (cursor, anchor
                // and span edges always are).
                let mut at = line_start;
                for (kind, text) in spans {
                    let end = at + text.len();
                    let cut_lo = lo.clamp(at, end) - at;
                    let cut_hi = hi.clamp(at, end) - at;
                    for (piece, selected) in [
                        (&text[..cut_lo], false),
                        (&text[cut_lo..cut_hi], true),
                        (&text[cut_hi..], false),
                    ] {
                        if piece.is_empty() {
                            continue;
                        }
                        let d = div()
                            .text_color(rgb(span_color(kind)))
                            .child(piece.to_owned());
                        row = row.child(if selected { d.bg(rgb(co.selection)) } else { d });
                    }
                    at = end;
                }
            } else if ln == cur_line {
                // Split the spans at the caret column and paint a bar.
                let mut remaining = cur_col;
                let mut placed = false;
                for (kind, text) in spans {
                    let chars = text.chars().count();
                    if !placed && remaining <= chars {
                        let pre: String = text.chars().take(remaining).collect();
                        let post: String = text.chars().skip(remaining).collect();
                        row = row
                            .child(div().text_color(rgb(span_color(kind))).child(pre))
                            .child(div().w(px(2.0)).bg(rgb(co.code)))
                            .child(div().text_color(rgb(span_color(kind))).child(post));
                        placed = true;
                    } else {
                        row = row.child(div().text_color(rgb(span_color(kind))).child(text));
                        if !placed {
                            remaining = remaining.saturating_sub(chars);
                        }
                    }
                }
                if !placed {
                    row = row.child(div().w(px(2.0)).bg(rgb(co.code)));
                }
                row = row.bg(rgb(mix_u32(co.panel, co.selection, 96)));
            } else {
                // G1: per-char click targets — every char is its own
                // cell carrying its char column, so a click (or double
                // click) lands the cursor on the exact glyph, in-word
                // included (the mouse path into BodyEditor).
                let mut col = 0usize;
                for (kind, text) in spans {
                    let n = text.chars().count();
                    for (piece, chunk_col) in char_cells(&text, col) {
                        row = row.child(
                            div()
                                .text_color(rgb(span_color(kind)))
                                .child(piece)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this: &mut Self,
                                              ev: &gpui::MouseDownEvent,
                                              _w,
                                              cx| {
                                            if ev.click_count >= 2 {
                                                this.app.body_double_click(ln, chunk_col);
                                            } else {
                                                this.app.body_click(ln, chunk_col);
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        },
                                    ),
                                )
                                // G2: drag with the left button held
                                // extends the charwise VISUAL selection
                                // (BodyEditor::drag_to via body_drag).
                                .on_mouse_move(cx.listener(
                                    move |this: &mut Self,
                                          ev: &gpui::MouseMoveEvent,
                                          _w,
                                          cx| {
                                        if ev.pressed_button == Some(MouseButton::Left) {
                                            this.app.body_drag(ln, chunk_col);
                                            cx.notify();
                                        }
                                    },
                                )),
                        );
                    }
                    col += n;
                }
            }
            // Fallback: a click on the empty tail of any line parks the
            // cursor at that line's end.
            row = row.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut Self, ev: &gpui::MouseDownEvent, _w, cx| {
                    if ev.click_count < 2 {
                        this.app.body_click(ln, usize::MAX / 2);
                        cx.notify();
                    }
                }),
            );
            body = body.child(row);
            line_start += line_len + 1;
        }
        let mut pane = div()
            .flex()
            .flex_col()
            .flex_grow()
            .gap_2()
            .child(header)
            .child(body);
        let items = self.app.body_completion_items();
        if !items.is_empty() {
            let ix = self.app.body_completion_ix().unwrap_or(0);
            pane = pane.child(
                div()
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
                            .text_size(px(12.0))
                            .bg(if i == ix {
                                rgb(co.selection)
                            } else {
                                rgb(co.bg)
                            })
                            .text_color(rgb(if i == ix { co.fg } else { co.muted }))
                            .child(item.clone())
                    })),
            );
        }
        pane
    }

    /// Command palette entries with the cursor row highlighted; every
    /// row clickable (runs the entry, I8).
    fn palette_pane(&self, co: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor = self.app.palette_cursor();
        div().flex().flex_col().gap_0().children(
            self.app
                .palette_entries()
                .into_iter()
                .enumerate()
                .map(|(i, e)| {
                    let is_cur = i == cursor;
                    div()
                        .flex()
                        .items_center()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(rgb(if is_cur { co.selection } else { co.bg }))
                        .hover(move |s| s.bg(rgb(if is_cur { co.selection } else { co.hover })))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev, _w, cx| {
                                this.app.palette_click(&mut this.shell, i);
                                if this.app.should_quit() {
                                    cx.quit();
                                }
                                cx.notify();
                            }),
                        )
                        .child(div().w(px(140.0)).text_color(rgb(co.fg)).child(e.label))
                        .child(
                            div()
                                .flex_grow()
                                .text_color(rgb(co.muted))
                                .text_size(px(11.0))
                                .child(e.description),
                        )
                        .child(
                            div()
                                .text_color(rgb(co.accent))
                                .text_size(px(11.0))
                                .child(e.action.chord().to_owned()),
                        )
                }),
        )
    }

    /// Agenda pane: rows grouped under date headers, SCHEDULED accent /
    /// DEADLINE error kind chips, the today group accented, overdue red.
    /// Row click jumps like Enter (`jump_list_row`).
    fn agenda_pane(&self, co: Colors, cx: &mut Context<Self>) -> impl IntoElement {
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
                        .text_size(px(11.0))
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
                            .text_size(px(10.0))
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
    fn detail_pane(&self, pane: gpui::Div, co: Colors, cx: &mut Context<Self>) -> gpui::Div {
        let Some(d) = self.app.detail(&self.shell) else {
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
                .text_size(px(12.0))
                .child(meta_line(&d)),
            "toggle-todo",
            cx,
        ))
        .child(clickable(
            co,
            div()
                .text_color(rgb(co.warning))
                .text_size(px(12.0))
                .child(tags),
            "edit-tags",
            cx,
        ))
        .child(clickable(
            co,
            div()
                .text_color(rgb(co.error))
                .text_size(px(11.0))
                .child(props),
            "edit-property",
            cx,
        ))
        .child(
            div()
                .text_color(rgb(co.muted))
                .text_size(px(10.0))
                .child(d.path.clone()),
        )
        .child(clickable(
            co,
            {
                // C3: the read-only body preview reuses the editor's
                // highlight_body spans (same colours as edit mode).
                let mut body_el = div()
                    .mt_2()
                    .flex_grow()
                    .flex()
                    .flex_col()
                    .text_color(rgb(co.fg))
                    .text_size(px(13.0));
                if d.body.is_empty() {
                    body_el = body_el.child("+ body".to_owned());
                } else {
                    // Same palette as the editor pane's span_color.
                    let span_color = |k: BodySpan| match k {
                        BodySpan::Plain => co.fg,
                        BodySpan::Meta => co.muted,
                        BodySpan::Drawer => co.error,
                        BodySpan::Keyword => co.accent,
                        BodySpan::Literal => co.success,
                        BodySpan::Comment => co.muted,
                    };
                    for spans in highlight_body(&d.body) {
                        let mut line = div().flex().min_h(px(17.0));
                        for (kind, text) in spans {
                            line = line.child(div().text_color(rgb(span_color(kind))).child(text));
                        }
                        body_el = body_el.child(line);
                    }
                }
                body_el
            },
            "edit-body",
            cx,
        ))
    }

    /// Footer: pending-chord completions (which-key popup) when a
    /// chord is in flight, otherwise the full clickable binding bar.
    fn footer(&self, co: Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.app.pending_chord();
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(rgb(co.panel))
            .text_size(px(11.0));
        if pending.is_empty() {
            // Doom-style which-key: one column per palette section,
            // group title on top, chord-sorted entries beneath (I4 —
            // the same which_key_groups data every shell reads).
            bar.items_start()
                .child(
                    div()
                        .px_1()
                        .text_color(rgb(co.accent))
                        .child(format!("[{:?}]", self.app.input_mode())),
                )
                .children(
                    self.app
                        .which_key_groups()
                        .into_iter()
                        .map(|(title, entries)| {
                            let mut col = div()
                                .flex()
                                .flex_col()
                                .px_2()
                                .child(div().text_color(rgb(co.heading2)).child(title));
                            for (chord, cmd) in entries {
                                let run = cmd.clone();
                                col = col.child(
                                    div()
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
                                            div()
                                                .w(px(56.0))
                                                .text_color(rgb(co.accent))
                                                .child(chord),
                                        )
                                        .child(div().text_color(rgb(co.muted)).child(cmd)),
                                );
                            }
                            col
                        }),
                )
        } else {
            bar.child(
                div()
                    .px_1()
                    .text_color(rgb(co.warning))
                    .child(format!("{pending} ‸")),
            )
            .children(self.app.completions().into_iter().map(|(rest, cmd)| {
                div()
                    .flex()
                    .px_1()
                    .child(div().text_color(rgb(co.accent)).child(rest))
                    .child(div().text_color(rgb(co.muted)).child(format!(" → {cmd}")))
            }))
        }
    }
}

/// A generic clickable list row (agenda / blocks / backlinks).
#[cfg(feature = "gpui")]
fn list_row(
    co: Colors,
    selected: bool,
    text: String,
    listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .text_size(px(13.0))
        .bg(rgb(if selected { co.selection } else { co.bg }))
        .hover(move |s| s.bg(rgb(if selected { co.selection } else { co.hover })))
        .on_mouse_down(MouseButton::Left, listener)
        .child(text)
}

/// Wrap a detail field so a click begins the matching edit command.
#[cfg(feature = "gpui")]
fn clickable(
    co: Colors,
    inner: gpui::Div,
    command: &'static str,
    cx: &mut Context<GpuiView>,
) -> gpui::Div {
    div()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let co = Colors::of(&self.theme);
        let mono = self.theme.typography.mono_family.to_owned();

        let header = div()
            .flex()
            .items_center()
            .px_3()
            .py_1()
            .gap_2()
            .child(div().text_color(rgb(co.accent)).text_lg().child("closure"))
            .child(
                // Mode chip — click cycles the input mode.
                div()
                    .px_2()
                    .rounded_md()
                    .bg(rgb(co.panel))
                    .text_size(px(11.0))
                    .text_color(rgb(co.warning))
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _ev, _w, cx| this.click("cycle-mode", cx)),
                    )
                    .child(format!("{:?}", self.app.input_mode())),
            )
            .child(div().flex_grow())
            .child(
                // Notion "+" — click captures (same command as `c`).
                div()
                    .px_2()
                    .rounded_md()
                    .bg(rgb(co.panel))
                    .text_size(px(11.0))
                    .text_color(rgb(co.success))
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _ev, _w, cx| {
                            this.click("capture-start", cx);
                        }),
                    )
                    .child("＋ capture"),
            )
            .child(
                div()
                    .px_2()
                    .rounded_md()
                    .bg(rgb(co.panel))
                    .text_size(px(11.0))
                    .text_color(rgb(co.accent))
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(co.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _ev, _w, cx| this.click("palette", cx)),
                    )
                    .child("❯ palette"),
            );

        let context = div()
            .px_3()
            .py_1()
            .bg(rgb(co.panel))
            .text_color(rgb(co.fg))
            .text_size(px(12.0))
            .child(self.context_line());

        let body = div()
            .flex()
            .flex_row()
            .flex_grow()
            .child(self.rows_pane(co, cx))
            .child(self.side_pane(co, cx));

        let status = div()
            .px_3()
            .py_1()
            .bg(rgb(co.panel))
            .text_color(rgb(co.muted))
            .text_size(px(11.0))
            .child(self.app.status().to_owned());

        // Toast strip: the newest three feedback items, severity-coloured.
        let toasts = div().flex().gap_2().px_3().children(
            self.feedback
                .items()
                .iter()
                .rev()
                .take(3)
                .map(|item| {
                    use closure_shell_core::FeedbackKind as K;
                    let col = match item.kind {
                        K::Error => co.error,
                        K::Warning => co.warning,
                        K::Success => co.success,
                        K::Info | K::Progress(_) => co.accent,
                    };
                    div()
                        .px_2()
                        .rounded_md()
                        .bg(rgb(mix_u32(co.bg, col, 48)))
                        .border_1()
                        .border_color(rgb(col))
                        .text_color(rgb(col))
                        .text_size(px(11.0))
                        .child(format!("⚑ {}", item.text))
                })
                .collect::<Vec<_>>(),
        );

        div()
            .key_context("ClosureGpui")
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(co.bg))
            .text_color(rgb(co.fg))
            .font_family(mono)
            .child(header)
            .child(context)
            .child(toasts)
            .child(body)
            .child(status)
            .child(self.footer(co, cx))
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
