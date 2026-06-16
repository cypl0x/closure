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
    use closure_shell_core::{
        App, BodySegment, Detail, HighlightKind, ModalApp, ModalSurface, Mode, Row, highlight_spans,
        segment_body,
    };
    use closure_store::Vault;
    use eframe::egui;

    use super::Shell;

    /// Render `text` as a label that senses clicks when `enabled`;
    /// otherwise a plain read-only label. Returns the response so the
    /// caller can act on `.clicked()`. Backs the E4 click-to-edit fields.
    fn clickable(ui: &mut egui::Ui, text: egui::RichText, enabled: bool) -> egui::Response {
        if enabled {
            ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        } else {
            ui.label(text)
        }
    }

    /// Map a coarse [`HighlightKind`] to a colour. `Identifier`/`Plain`/
    /// `Punctuation` inherit the default text colour so plain prose
    /// (which the keyword highlighter tags as `Identifier`) reads
    /// normally; only keywords/literals/comments get colour.
    const fn kind_colour(kind: HighlightKind) -> Option<egui::Color32> {
        match kind {
            HighlightKind::Keyword => Some(egui::Color32::from_rgb(0xE5, 0xC0, 0x7B)),
            HighlightKind::Literal => Some(egui::Color32::from_rgb(0x98, 0xC3, 0x79)),
            HighlightKind::Comment => Some(egui::Color32::from_rgb(0x7F, 0x84, 0x8E)),
            HighlightKind::Identifier | HighlightKind::Punctuation | HighlightKind::Plain => None,
        }
    }

    /// Render one line of `source` (in language `lang`) as coloured
    /// inline segments via the tested [`highlight_spans`] helper.
    /// Keyword-coloured tokens stand out; everything else is default.
    fn highlighted_line(ui: &mut egui::Ui, source: &str, lang: &str) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if source.is_empty() {
                ui.label(" ");
                return;
            }
            for (text, kind) in highlight_spans(source, lang) {
                let mut rt = egui::RichText::new(text).monospace();
                if let Some(c) = kind_colour(kind) {
                    rt = rt.color(c);
                }
                ui.label(rt);
            }
        });
    }

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

        /// A read-only list overlay for the modal "list" surfaces
        /// (backlinks/agenda/blocks): `(title, rows)`. `None` when not on
        /// such a surface. The launcher never has these.
        fn list_overlay(&self, shell: &Shell) -> Option<(String, Vec<String>)> {
            let Self::Modal(a) = self else {
                return None;
            };
            match a.surface() {
                ModalSurface::Backlinks => Some((
                    "backlinks (Esc to return)".to_owned(),
                    a.backlink_rows(shell)
                        .into_iter()
                        .map(|(path, title)| format!("{title}    ({path})"))
                        .collect(),
                )),
                ModalSurface::Agenda => Some((
                    "agenda (Esc to return)".to_owned(),
                    a.agenda_rows(shell)
                        .into_iter()
                        .map(|(date, title, path)| format!("{date}  {title}    ({path})"))
                        .collect(),
                )),
                ModalSurface::Blocks => Some((
                    "code blocks (Esc to return)".to_owned(),
                    a.block_rows(shell)
                        .into_iter()
                        .map(|(path, lang, first)| format!("⟪{lang}⟫ {first}    ({path})"))
                        .collect(),
                )),
                _ => None,
            }
        }

        /// Activate row `i` on a modal list surface (jump to its
        /// headline/file). No-op on the launcher.
        fn jump_list_row(&mut self, shell: &Shell, i: usize) {
            if let Self::Modal(a) = self {
                a.jump_list_row(shell, i);
            }
        }

        /// Which-key state: the pending chord prefix + its completions
        /// (`(remaining, command)`). Empty prefix => no popup. Only the
        /// modal surface has multi-stroke chords; the launcher returns
        /// nothing (its slash palette is its which-key surface).
        fn whichkey(&self) -> (String, Vec<(String, String)>) {
            match self {
                Self::Modal(a) => (a.pending_chord(), a.completions()),
                Self::Launcher(_) => (String::new(), Vec::new()),
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
                    Mode::EditBody => "✎ edit body".to_owned(),
                    Mode::PropertyEdit => "＋ property".to_owned(),
                    Mode::TagsEdit => "🏷 tags".to_owned(),
                    Mode::Browse => format!("⌕ {}", a.query()),
                },
                Self::Modal(a) => match a.surface() {
                    ModalSurface::Capture => format!("＋ capture: {}", a.capture_buffer()),
                    ModalSurface::Search => format!("⌕ {}", a.query()),
                    ModalSurface::EditBody => "✎ edit body".to_owned(),
                    ModalSurface::Backlinks => "↩ backlinks".to_owned(),
                    ModalSurface::Agenda => "🗓 agenda".to_owned(),
                    ModalSurface::Blocks => "❮❯ code blocks".to_owned(),
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
        /// Enter the body editor (org-edit-special). Launcher only — the
        /// modal modes enter it with their keymap chord.
        fn begin_edit_body(&mut self, shell: &Shell) {
            if let Self::Launcher(a) = self {
                a.begin_edit_body(shell);
            }
        }
        /// Whether the body editor surface is active (either app).
        const fn editing_body(&self) -> bool {
            match self {
                Self::Launcher(a) => matches!(a.mode(), Mode::EditBody),
                Self::Modal(a) => matches!(a.surface(), ModalSurface::EditBody),
            }
        }
        /// Whether the property editor is active (launcher surface only;
        /// the modal modes don't edit properties yet).
        const fn editing_property(&self) -> bool {
            matches!(self, Self::Launcher(a) if matches!(a.mode(), Mode::PropertyEdit))
        }
        /// The launcher `App` when active, for property editing (which is
        /// launcher-only this cycle).
        const fn launcher_mut(&mut self) -> Option<&mut App> {
            match self {
                Self::Launcher(a) => Some(a),
                Self::Modal(_) => None,
            }
        }
        /// Multiline body buffer the `TextEdit` widget binds to.
        const fn body_buffer_mut(&mut self) -> &mut String {
            match self {
                Self::Launcher(a) => a.body_buffer_mut(),
                Self::Modal(a) => a.body_buffer_mut(),
            }
        }
        /// Commit the body buffer through the Vault (I8).
        fn commit_edit_body(&mut self, shell: &mut Shell) {
            match self {
                Self::Launcher(a) => a.commit_edit_body(shell),
                Self::Modal(a) => a.commit_edit_body(shell),
            }
        }
        /// Commit the property form (launcher only).
        fn commit_property(&mut self, shell: &mut Shell) {
            if let Self::Launcher(a) = self {
                a.commit_property(shell);
            }
        }
        /// Whether the tags editor is active (launcher surface only).
        const fn editing_tags(&self) -> bool {
            matches!(self, Self::Launcher(a) if matches!(a.mode(), Mode::TagsEdit))
        }
        /// Commit the tags form (launcher only).
        fn commit_tags(&mut self, shell: &mut Shell) {
            if let Self::Launcher(a) = self {
                a.commit_tags(shell);
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
            // While an editor surface is open the TextEdit widgets own
            // typing; feeding the same chars to on_key would
            // double-insert. Route only Esc (cancel) and, for the body
            // editor, C-Enter (commit) globally.
            if self.surface.editing_body()
                || self.surface.editing_property()
                || self.surface.editing_tags()
            {
                let body = self.surface.editing_body();
                for ev in events {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = ev
                    {
                        match key {
                            egui::Key::Escape => {
                                self.surface.on_named(&mut self.shell, "escape", false, false);
                            }
                            egui::Key::Enter if modifiers.ctrl && body => {
                                self.surface.commit_edit_body(&mut self.shell);
                            }
                            _ => {}
                        }
                    }
                }
                return;
            }
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
                        let edit = self.surface.chord_for("edit-body");
                        if ui.button(format!("✎ edit body ({edit})")).clicked() {
                            self.surface.begin_edit_body(&self.shell);
                        }
                        if ui.button("＋ property").clicked()
                            && let Some(a) = self.surface.launcher_mut()
                        {
                            a.begin_add_property(&self.shell);
                        }
                    });
                    // Field edits on the selection (TODO / priority / tags).
                    ui.horizontal(|ui| {
                        if ui.button("⟳ todo").clicked()
                            && let Some(a) = self.surface.launcher_mut()
                        {
                            a.cycle_todo(&mut self.shell);
                        }
                        for p in ['A', 'B', 'C'] {
                            if ui.button(format!("[#{p}]")).clicked()
                                && let Some(a) = self.surface.launcher_mut()
                            {
                                a.set_priority_cmd(&mut self.shell, Some(p));
                            }
                        }
                        if ui.button("[# ]").clicked()
                            && let Some(a) = self.surface.launcher_mut()
                        {
                            a.set_priority_cmd(&mut self.shell, None);
                        }
                        if ui.button("🏷 tags").clicked()
                            && let Some(a) = self.surface.launcher_mut()
                        {
                            a.begin_edit_tags(&self.shell);
                        }
                    });
                }
            });
            egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
                ui.label(self.surface.status());
                ui.small(self.surface.key_hints());
            });
            // which-key popup (vision: emacs which-key): only while a
            // multi-stroke chord prefix is pending on the modal surface.
            let (prefix, completions) = self.surface.whichkey();
            if !prefix.is_empty() && !completions.is_empty() {
                egui::TopBottomPanel::bottom("whichkey").show(ctx, |ui| {
                    ui.strong(format!("{prefix} →"));
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        for (rest, cmd) in &completions {
                            ui.label(format!("{prefix} {rest:8}  {cmd}"));
                        }
                    });
                });
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                // Read-only list surfaces (backlinks/agenda/blocks) take
                // the whole pane; otherwise the browse list + right pane.
                if self.render_list_overlay(ui) {
                    return;
                }
                ui.columns(2, |cols| {
                    self.list_pane(&mut cols[0]);
                    if self.surface.editing_body() {
                        self.editor_pane(&mut cols[1]);
                    } else if self.surface.editing_property() {
                        self.property_pane(&mut cols[1]);
                    } else if self.surface.editing_tags() {
                        self.tags_pane(&mut cols[1]);
                    } else {
                        self.right_pane(&mut cols[1]);
                    }
                });
            });
        }
    }

    impl EguiApp {
        /// Render a read-only list surface (backlinks/agenda/blocks) as a
        /// full-pane clickable list; returns true when it handled the
        /// pane. Click a row -> jump to its headline/file (R1).
        fn render_list_overlay(&mut self, ui: &mut egui::Ui) -> bool {
            let Some((title, rows)) = self.surface.list_overlay(&self.shell) else {
                return false;
            };
            ui.heading(title);
            let selected = self.surface.selected();
            let mut clicked: Option<usize> = None;
            egui::ScrollArea::vertical().id_salt("list-overlay").show(ui, |ui| {
                if rows.is_empty() {
                    ui.label("(none)");
                }
                for (i, row) in rows.into_iter().enumerate() {
                    if ui.selectable_label(i == selected, row).clicked() {
                        clicked = Some(i);
                    }
                }
            });
            if let Some(i) = clicked {
                self.surface.jump_list_row(&self.shell, i);
            }
            true
        }

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

        /// org-edit-special body editor: a multiline `TextEdit` bound to
        /// the shell-core body buffer + Save/Cancel. The widget owns
        /// typing; commit/cancel route through the shell-core methods.
        fn editor_pane(&mut self, ui: &mut egui::Ui) {
            ui.label("body (org-edit-special):");
            ui.add(
                egui::TextEdit::multiline(self.surface.body_buffer_mut())
                    .desired_rows(14)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            ui.horizontal(|ui| {
                if ui.button("💾 save (C-Enter)").clicked() {
                    self.surface.commit_edit_body(&mut self.shell);
                }
                if ui.button("✕ cancel (Esc)").clicked() {
                    self.surface.on_named(&mut self.shell, "escape", false, false);
                }
            });
        }

        /// Property editor form: key + value fields bound to the
        /// shell-core buffers + Save/Cancel. Launcher-only.
        fn property_pane(&mut self, ui: &mut egui::Ui) {
            ui.label("property (:KEY: value):");
            // Scope the launcher borrow to the field widgets so the
            // Save/Cancel buttons can re-borrow the surface.
            {
                let Some(app) = self.surface.launcher_mut() else {
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("key");
                    ui.add(egui::TextEdit::singleline(app.prop_key_mut()).desired_width(120.0));
                });
                ui.horizontal(|ui| {
                    ui.label("value");
                    ui.add(
                        egui::TextEdit::singleline(app.prop_value_mut())
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            ui.horizontal(|ui| {
                if ui.button("💾 save").clicked() {
                    self.surface.commit_property(&mut self.shell);
                }
                if ui.button("✕ cancel (Esc)").clicked() {
                    self.surface.on_named(&mut self.shell, "escape", false, false);
                }
            });
        }

        /// Tags editor: a space-separated text field + Save/Cancel.
        /// Launcher-only.
        fn tags_pane(&mut self, ui: &mut egui::Ui) {
            ui.label("tags (space-separated):");
            {
                let Some(app) = self.surface.launcher_mut() else {
                    return;
                };
                ui.add(
                    egui::TextEdit::singleline(app.tags_buffer_mut())
                        .desired_width(f32::INFINITY),
                );
            }
            ui.horizontal(|ui| {
                if ui.button("💾 save").clicked() {
                    self.surface.commit_tags(&mut self.shell);
                }
                if ui.button("✕ cancel (Esc)").clicked() {
                    self.surface.on_named(&mut self.shell, "escape", false, false);
                }
            });
        }

        fn right_pane(&mut self, ui: &mut egui::Ui) {
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
            // E4 inline edit: on the launcher surface every detail field
            // is a click-to-edit affordance, reusing the tested
            // begin_*/cycle/commit methods (no logic in the window).
            let launcher = self.surface.is_launcher();
            // Title -> rename.
            let title = clickable(ui, egui::RichText::new(&d.title).heading(), launcher);
            if title.clicked()
                && let Some(a) = self.surface.launcher_mut()
            {
                a.begin_rename(&self.shell);
            }
            ui.horizontal(|ui| {
                // TODO keyword -> cycle.
                let todo_txt = d
                    .todo
                    .as_deref()
                    .map_or_else(|| "·".to_owned(), |t| format!("TODO: {t}"));
                if clickable(ui, egui::RichText::new(todo_txt), launcher).clicked()
                    && let Some(a) = self.surface.launcher_mut()
                {
                    a.cycle_todo(&mut self.shell);
                }
                if let Some(p) = d.priority {
                    ui.label(format!("[#{p}]"));
                }
                // Tags -> edit.
                let tags_txt = if d.tags.is_empty() {
                    "🏷".to_owned()
                } else {
                    format!(":{}:", d.tags.join(":"))
                };
                if clickable(ui, egui::RichText::new(tags_txt), launcher).clicked()
                    && let Some(a) = self.surface.launcher_mut()
                {
                    a.begin_edit_tags(&self.shell);
                }
            });
            // Per-property edit affordance (launcher only): click "✎".
            for (k, v) in &d.properties {
                ui.horizontal(|ui| {
                    ui.label(format!(":{k}: {v}"));
                    if launcher && ui.small_button("✎").clicked() {
                        let key = k.clone();
                        if let Some(a) = self.surface.launcher_mut() {
                            a.begin_edit_property(&self.shell, &key);
                        }
                    }
                });
            }
            ui.separator();
            // Body header -> edit.
            if clickable(ui, egui::RichText::new("body:").strong(), launcher).clicked()
                && let Some(a) = self.surface.launcher_mut()
            {
                a.begin_edit_body(&self.shell);
            }
            // Body: prose plain, #+BEGIN_SRC blocks framed + monospace
            // with the language shown and keyword-highlighted (P2/P3).
            // Long bodies scroll.
            egui::ScrollArea::vertical()
                .id_salt("detail-body")
                .show(ui, |ui| {
                    for seg in segment_body(&d.body) {
                        match seg {
                            BodySegment::Prose(text) => {
                                for line in text.lines() {
                                    highlighted_line(ui, line, "plain");
                                }
                            }
                            BodySegment::Code { lang, text } => {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.small(format!("⟪ {lang} ⟫"));
                                    for line in text.lines() {
                                        highlighted_line(ui, line, &lang);
                                    }
                                });
                            }
                        }
                    }
                });
        }
    }
}

#[cfg(feature = "egui")]
pub use window::run;
