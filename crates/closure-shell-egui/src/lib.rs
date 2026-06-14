//! Native desktop shell built on egui + eframe.
//!
//! I7: consumes the shell-agnostic [`closure_shell_core`] launcher
//! (which itself touches only `closure_core` / `closure_store` /
//! `closure_query`). The launcher state ([`App`], [`Shell`], browse/
//! filter/detail/palette/edit/modes) is shared with the gpui shell and
//! fully unit-tested without a window; this crate re-exports it and
//! adds the eframe window behind the opt-in `egui` cargo feature so the
//! default workspace stays hermetic (I10).

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
    use closure_shell_core::{App, Mode};
    use closure_store::Vault;
    use eframe::egui;

    use super::Shell;

    /// Launch the egui desktop window against the vault at `path`.
    ///
    /// # Errors
    ///
    /// Vault open or eframe runtime failures as a string.
    pub fn run(path: &std::path::Path) -> Result<(), String> {
        let vault = Vault::open(path).map_err(|e| format!("{e}"))?;
        let app = EguiApp {
            shell: Shell::new(vault),
            app: App::new(),
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
        app: App,
    }

    impl EguiApp {
        fn handle_input(&mut self, ctx: &egui::Context) {
            let events = ctx.input(|i| i.events.clone());
            for ev in events {
                match ev {
                    egui::Event::Text(s) => {
                        for c in s.chars() {
                            self.app
                                .on_key(&mut self.shell, &c.to_string(), false, Some(c));
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
                            self.app.on_key(&mut self.shell, &token, true, None);
                        } else {
                            let token = match key {
                                egui::Key::Enter => "enter",
                                egui::Key::Escape => "escape",
                                egui::Key::Backspace => "backspace",
                                egui::Key::ArrowDown => "down",
                                egui::Key::ArrowUp => "up",
                                _ => continue,
                            };
                            self.app.on_key(&mut self.shell, token, false, None);
                        }
                    }
                    _ => {}
                }
            }
        }

        fn header(&self) -> String {
            match self.app.mode() {
                Mode::Capture => format!("＋ capture: {}", self.app.capture_buffer()),
                Mode::AddSibling => format!("＋ add: {}", self.app.capture_buffer()),
                Mode::Rename => format!("✎ rename: {}", self.app.capture_buffer()),
                Mode::Palette => format!("❯ command: {}", self.app.capture_buffer()),
                Mode::Browse => format!("⌕ {}", self.app.query()),
            }
        }
    }

    impl eframe::App for EguiApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.handle_input(ctx);
            if self.app.should_quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                ui.heading("closure · egui");
                ui.label(self.header());
            });
            egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
                ui.label(self.app.status().to_owned());
                ui.small(self.app.key_hints());
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
            let (offset, rows) = self.app.view_window(&self.shell, PAGE);
            let selected = self.app.selected();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (vis, row) in rows.into_iter().enumerate() {
                    let i = offset + vis;
                    let indent = "  ".repeat(usize::from(row.level).saturating_sub(1));
                    let todo = row.todo.map_or_else(String::new, |t| format!("{t} "));
                    let label = format!("{indent}{todo}{}", row.title);
                    if ui.selectable_label(i == selected, label).clicked() {
                        self.app.select(i, &self.shell);
                    }
                }
            });
        }

        fn right_pane(&self, ui: &mut egui::Ui) {
            if self.app.mode() == Mode::Palette {
                let cursor = self.app.palette_cursor();
                for (i, (name, keyhint)) in self.app.palette_results().into_iter().enumerate() {
                    let _ = ui.selectable_label(i == cursor, format!("{name:24} {keyhint}"));
                }
                return;
            }
            let Some(d) = self.app.detail(&self.shell) else {
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
