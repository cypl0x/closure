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

#[cfg(feature = "gpui")]
use closure_store::Vault;

// The launcher state core is shell-agnostic and lives in
// closure-shell-core; the gpui crate re-exports it (GpuiApp/GpuiMode
// aliases preserve the historical names) and adds the gpui window.
pub use closure_shell_core::{
    App as GpuiApp, Detail, HeadlessAdapter, ModalApp, ModalSurface, Mode as GpuiMode, Row,
    Selection, Shell, ShellAdapter,
};

/// Marker for the capability matrix.
pub const GPUI_SHELL: &str = "gpui";

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
            GpuiMode::EditBody => format!("✎ body: {}▏", self.app.body_buffer()),
            GpuiMode::TagsEdit => format!("✎ tags: {}▏", self.app.tags_buffer()),
            GpuiMode::PropertyEdit => format!(
                "✎ prop: {}={}▏",
                self.app.property_key(),
                self.app.property_value()
            ),
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
