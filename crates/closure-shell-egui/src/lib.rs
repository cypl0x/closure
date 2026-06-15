//! Native desktop shell built on egui + eframe.
//!
//! I7: consumes the shell-agnostic [`closure_shell_core`] launcher
//! (which itself touches only `closure_core` / `closure_store` /
//! `closure_query`). The launcher state ([`App`], [`Shell`], browse/
//! filter/detail/palette/edit/modes) is shared with the gpui shell and
//! fully unit-tested without a window; this crate re-exports it and
//! adds the eframe window behind the opt-in `egui` cargo feature so the
//! default workspace stays hermetic (I10).
//!
//! Build gate: `nix develop -c just gui-egui`.
//! Launch:     `nix develop -c just run-egui /path/to/vault`
//!         (=  `cargo run -p closure-cli --features egui -- egui <vault>`).
//! G1 verified: builds clean + launches (eframe event loop runs, window
//! opens) on the dev host. The window is display-bound, so it is not part
//! of the hermetic gate — `just gui-egui` only guarantees it compiles.

#![forbid(unsafe_code)]

// Re-export the shared launcher core (one tested state machine across
// every GUI shell).
pub use closure_shell_core::{
    App, Detail, HeadlessAdapter, ModalApp, ModalSurface, Mode, Row, Selection, Shell, ShellAdapter,
};

/// Marker for the capability matrix.
pub const EGUI_SHELL: &str = "egui";

/// Launch fallback when the `egui` feature is disabled (default,
/// hermetic build). The kernel-side launcher core is always available;
/// the window requires `--features egui` + the system GL/X11 libs.
#[cfg(not(feature = "egui"))]
pub fn run(_vault_path: &std::path::Path) -> Result<(), String> {
    Err("egui shell not compiled: rebuild with `--features egui` \
         (pulls eframe + system GL/X11/wayland libs)."
        .to_owned())
}

// === eframe window over the shared launcher core ===
// Behind the `egui` feature. The state + behaviour live in
// closure-shell-core (App), fully unit-tested without a window; this
// adapter only translates egui input + draws. Parity with the gpui
// shell (I7/I8). The window itself needs a display to exercise.
#[cfg(feature = "egui")]
mod window {
    use closure_config::{Config, InputMode};
    use closure_shell_core::{App, Detail, ModalApp, ModalSurface, Mode, Row};
    use closure_store::Vault;
    use eframe::egui;

    use super::Shell;

    /// The active GUI surface. Notion-style launcher (type-to-filter,
    /// the default) or the keymap-driven modal surface backing the four
    /// keyboard editing modes (vim/emacs/doom/helix). Both surfaces
    /// expose `select`/`view_window`/`detail`/`status`/`key_hints`/
    /// `should_quit`/`selected`, so list + detail draw code is shared;
    /// only the header, the right pane, and key translation branch.
    enum Surface {
        /// Notion default: typing filters, slash opens the palette.
        Launcher(App),
        /// Keyboard modes: keys resolve against the active mode's keymap.
        Modal(ModalApp),
    }

    impl Surface {
        /// Pick the surface from the configured input mode: Notion ->
        /// launcher, every other mode -> the keymap-driven modal surface.
        fn for_mode(mode: InputMode) -> Self {
            match mode {
                InputMode::Notion => Self::Launcher(App::new()),
                other => Self::Modal(ModalApp::new(other)),
            }
        }

        const fn should_quit(&self) -> bool {
            match self {
                Self::Launcher(a) => a.should_quit(),
                Self::Modal(a) => a.should_quit(),
            }
        }
        const fn selected(&self) -> usize {
            match self {
                Self::Launcher(a) => a.selected(),
                Self::Modal(a) => a.selected(),
            }
        }
        fn select(&mut self, i: usize, shell: &Shell) {
            match self {
                Self::Launcher(a) => a.select(i, shell),
                Self::Modal(a) => a.select(i, shell),
            }
        }
        fn view_window(&self, shell: &Shell, page: usize) -> (usize, Vec<Row>) {
            match self {
                Self::Launcher(a) => a.view_window(shell, page),
                Self::Modal(a) => a.view_window(shell, page),
            }
        }
        fn detail(&self, shell: &Shell) -> Option<Detail> {
            match self {
                Self::Launcher(a) => a.detail(shell),
                Self::Modal(a) => a.detail(shell),
            }
        }
        fn status(&self) -> String {
            match self {
                Self::Launcher(a) => a.status().to_owned(),
                Self::Modal(a) => a.status().to_owned(),
            }
        }
        fn key_hints(&self) -> String {
            match self {
                Self::Launcher(a) => a.key_hints(),
                Self::Modal(a) => a.key_hints(),
            }
        }

        /// Header line, surface-specific (shows the active sub-mode and
        /// any in-progress buffer).
        fn header(&self) -> String {
            match self {
                Self::Launcher(a) => match a.mode() {
                    Mode::Capture => format!("＋ capture: {}", a.capture_buffer()),
                    Mode::AddSibling => format!("＋ add: {}", a.capture_buffer()),
                    Mode::Rename => format!("✎ rename: {}", a.capture_buffer()),
                    Mode::Palette => format!("❯ command: {}", a.capture_buffer()),
                    Mode::Browse => format!("⌕ {}", a.query()),
                },
                Self::Modal(a) => match a.surface() {
                    ModalSurface::Capture => format!("＋ capture: {}", a.capture_buffer()),
                    ModalSurface::Search => format!("⌕ {}", a.query()),
                    ModalSurface::Browse => format!("[{:?}] browse", a.input_mode()),
                },
            }
        }

        /// True when the launcher palette pane should render.
        fn palette_open(&self) -> bool {
            matches!(self, Self::Launcher(a) if a.mode() == Mode::Palette)
        }
        fn palette_rows(&self) -> (usize, Vec<(String, String)>) {
            match self {
                Self::Launcher(a) => (a.palette_cursor(), a.palette_results()),
                Self::Modal(_) => (0, Vec::new()),
            }
        }

        /// Notion mouse affordances. Only the launcher surface reacts;
        /// the keyboard modal modes are driven by keys, so these no-op
        /// there (the ＋ button + double-click are Notion-flavor, G3).
        fn begin_add_sibling(&mut self, shell: &Shell) {
            if let Self::Launcher(a) = self {
                a.begin_add_sibling(shell);
            }
        }
        fn begin_capture(&mut self) {
            if let Self::Launcher(a) = self {
                a.begin_capture();
            }
        }
        fn begin_rename(&mut self, shell: &Shell) {
            if let Self::Launcher(a) = self {
                a.begin_rename(shell);
            }
        }
        /// Whether the mouse affordances apply (launcher surface only).
        const fn is_launcher(&self) -> bool {
            matches!(self, Self::Launcher(_))
        }
        /// The real chord bound to `command` in the active mode, for
        /// labelling a button (G4: never a hardcoded chord). `""` when
        /// unbound or on the modal surface.
        fn chord_for(&self, command: &str) -> &'static str {
            match self {
                Self::Launcher(a) => a.chord_for(command).unwrap_or(""),
                Self::Modal(_) => "",
            }
        }

        /// A printable character (no ctrl). Routes to the matching app.
        fn on_text(&mut self, shell: &mut Shell, c: char) {
            let token = c.to_string();
            match self {
                Self::Launcher(a) => a.on_key(shell, &token, false, Some(c)),
                Self::Modal(a) => a.on_key(shell, &token, false, false, Some(c)),
            }
        }
        /// A named/modified key (arrows, enter, esc, backspace, ctrl/alt
        /// chords).
        fn on_named(&mut self, shell: &mut Shell, token: &str, ctrl: bool, alt: bool) {
            match self {
                Self::Launcher(a) => a.on_key(shell, token, ctrl, None),
                Self::Modal(a) => a.on_key(shell, token, ctrl, alt, None),
            }
        }
    }

    /// Launch the egui desktop window against the vault at `path`. The
    /// surface follows the vault's configured `input_mode` (Notion by
    /// default, or the configured keyboard mode).
    ///
    /// # Errors
    ///
    /// Vault open or eframe runtime failures as a string.
    pub fn run(path: &std::path::Path) -> Result<(), String> {
        let vault = Vault::open(path).map_err(|e| format!("{e}"))?;
        // Notion default; honour `<vault>/config.org` input_mode if set.
        let mode = Config::from_path(&path.join("config.org"))
            .map_or(InputMode::Notion, |c| c.input_mode);
        let app = EguiApp {
            shell: Shell::new(vault),
            surface: Surface::for_mode(mode),
        };
        eframe::run_native(
            "closure · egui",
            eframe::NativeOptions::default(),
            Box::new(|_cc| Ok(Box::new(app))),
        )
        .map_err(|e| format!("{e}"))
    }

    struct EguiApp {
        shell: Shell,
        surface: Surface,
    }

    impl EguiApp {
        fn handle_input(&mut self, ctx: &egui::Context) {
            let events = ctx.input(|i| i.events.clone());
            for ev in events {
                match ev {
                    egui::Event::Text(s) => {
                        for c in s.chars() {
                            self.surface.on_text(&mut self.shell, c);
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if modifiers.ctrl {
                            let token = key.name().to_ascii_lowercase();
                            self.surface
                                .on_named(&mut self.shell, &token, true, modifiers.alt);
                        } else {
                            let token = match key {
                                egui::Key::Enter => "enter",
                                egui::Key::Escape => "escape",
                                egui::Key::Backspace => "backspace",
                                egui::Key::ArrowDown => "down",
                                egui::Key::ArrowUp => "up",
                                _ => continue,
                            };
                            self.surface
                                .on_named(&mut self.shell, token, false, modifiers.alt);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    impl eframe::App for EguiApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.handle_input(ctx);
            if self.surface.should_quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                ui.heading("closure · egui");
                ui.label(self.surface.header());
                // Notion-flavor mouse affordances (G3): clickable ＋ to
                // capture / add a sibling. Keyboard-mode surfaces hide
                // them (driven by keys instead).
                if self.surface.is_launcher() {
                    let cap = self.surface.chord_for("capture-start");
                    let add = self.surface.chord_for("add-sibling");
                    ui.horizontal(|ui| {
                        if ui.button(format!("＋ capture ({cap})")).clicked() {
                            self.surface.begin_capture();
                        }
                        if ui.button(format!("＋ add sibling ({add})")).clicked() {
                            self.surface.begin_add_sibling(&self.shell);
                        }
                    });
                }
            });
            egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
                ui.label(self.surface.status());
                ui.small(self.surface.key_hints());
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.columns(2, |cols| {
                    self.list_pane(&mut cols[0]);
                    self.right_pane(&mut cols[1]);
                });
            });
        }
    }

    impl EguiApp {
        fn list_pane(&mut self, ui: &mut egui::Ui) {
            const PAGE: usize = 40;
            let (offset, rows) = self.surface.view_window(&self.shell, PAGE);
            let selected = self.surface.selected();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (vis, row) in rows.into_iter().enumerate() {
                    let i = offset + vis;
                    let indent = "  ".repeat(usize::from(row.level).saturating_sub(1));
                    let todo = row.todo.map_or_else(String::new, |t| format!("{t} "));
                    let label = format!("{indent}{todo}{}", row.title);
                    let resp = ui.selectable_label(i == selected, label);
                    if resp.clicked() {
                        self.surface.select(i, &self.shell);
                    }
                    // Double-click to edit the title (Notion-flavor, G3).
                    if resp.double_clicked() {
                        self.surface.select(i, &self.shell);
                        self.surface.begin_rename(&self.shell);
                    }
                }
            });
        }

        fn right_pane(&self, ui: &mut egui::Ui) {
            if self.surface.palette_open() {
                let (cursor, results) = self.surface.palette_rows();
                for (i, (name, keyhint)) in results.into_iter().enumerate() {
                    let _ = ui.selectable_label(i == cursor, format!("{name:24} {keyhint}"));
                }
                return;
            }
            let Some(d) = self.surface.detail(&self.shell) else {
                ui.label("no selection");
                return;
            };
            ui.heading(&d.title);
            if let Some(t) = &d.todo {
                ui.label(format!("TODO: {t}"));
            }
            for (k, v) in &d.properties {
                ui.label(format!(":{k}: {v}"));
            }
            ui.separator();
            ui.label(&d.body);
        }
    }
}

#[cfg(feature = "egui")]
pub use window::run;
