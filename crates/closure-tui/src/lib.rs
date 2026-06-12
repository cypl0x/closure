//! ratatui + crossterm shell for the closure kernel.
//!
//! Read-only vault browser: file list + headline tree, full-source
//! file view (=RET=, =j=/=k= scroll), fuzzy find-file (=/=) and
//! vault-wide headline search (=s=), which-key popup on pending chord
//! prefixes. All state transitions live in the terminal-free [`App`].

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use closure_core::Document;
use closure_input::{ChordTrie, TrieStep};
use closure_store::Vault;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use thiserror::Error;

/// Shell errors.
#[derive(Debug, Error)]
pub enum TuiError {
    /// IO failure setting up the terminal.
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Default browse-mode bindings. Multi-stroke chords exercise the
/// which-key popup (spec invariant I4: bindings drive the popup, no
/// hand-maintained table).
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("q", "quit"),
    ("ESC", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
];

/// Which input surface the shell is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Navigating the file list via chord bindings.
    Browse,
    /// Typing a fuzzy file query; strokes edit the query instead of
    /// firing chords.
    Search,
    /// Typing a fuzzy query over headline titles across the vault.
    SearchHeadlines,
    /// Reading the selected file's full org source; =j=/=k= scroll.
    FileView,
}

/// Elm-style application state for the terminal shell. Strokes go in
/// via [`Self::handle_stroke`]; rendering reads the accessors. No
/// terminal I/O lives here, which keeps every transition testable.
pub struct App {
    paths: Vec<PathBuf>,
    selected: Option<usize>,
    bindings: Vec<(String, String)>,
    trie: ChordTrie,
    pending: Vec<String>,
    popup: Option<Vec<String>>,
    quit: bool,
    mode: AppMode,
    query: String,
    result_cursor: usize,
    headlines: Vec<(PathBuf, String)>,
    sources: Vec<(PathBuf, String)>,
    scroll: usize,
}

impl App {
    /// Build an app over `paths` with the default browse bindings.
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self::with_bindings(paths, DEFAULT_BINDINGS)
    }

    /// Build an app over `paths` with caller-supplied
    /// `(chord, command)` bindings, replacing the defaults entirely.
    #[must_use]
    pub fn with_bindings(paths: Vec<PathBuf>, bindings: &[(&str, &str)]) -> Self {
        let selected = if paths.is_empty() { None } else { Some(0) };
        Self {
            paths,
            selected,
            bindings: bindings
                .iter()
                .map(|(c, n)| ((*c).to_owned(), (*n).to_owned()))
                .collect(),
            trie: ChordTrie::build(bindings),
            pending: Vec::new(),
            popup: None,
            quit: false,
            mode: AppMode::Browse,
            query: String::new(),
            result_cursor: 0,
            headlines: Vec::new(),
            sources: Vec::new(),
            scroll: 0,
        }
    }

    /// Provide the `(file, org source)` records shown by the file
    /// view. Typically harvested from the vault once at startup.
    pub fn set_sources(&mut self, sources: Vec<(PathBuf, String)>) {
        self.sources = sources;
    }

    /// Source of the file open in the view, `None` outside
    /// [`AppMode::FileView`].
    #[must_use]
    pub fn view_source(&self) -> Option<&str> {
        if self.mode != AppMode::FileView {
            return None;
        }
        self.selected_path().and_then(|sel| {
            self.sources
                .iter()
                .find(|(p, _)| p.as_path() == sel)
                .map(|(_, src)| src.as_str())
        })
    }

    /// Scroll offset (top visible line) of the file view.
    #[must_use]
    pub const fn scroll(&self) -> usize {
        self.scroll
    }

    /// Provide the `(file, headline title)` records searched by
    /// headline mode. Typically harvested from the vault once at
    /// startup.
    pub fn set_headlines(&mut self, headlines: Vec<(PathBuf, String)>) {
        self.headlines = headlines;
    }

    /// Headlines matching the live query, best fuzzy score first.
    #[must_use]
    pub fn headline_results(&self) -> Vec<(&Path, &str)> {
        let mut scored: Vec<(usize, u32)> = self
            .headlines
            .iter()
            .enumerate()
            .filter_map(|(i, (_, t))| closure_query::fuzzy_score(&self.query, t).map(|sc| (i, sc)))
            .collect();
        scored.sort_by_key(|&(_, sc)| std::cmp::Reverse(sc));
        scored
            .iter()
            .map(|&(i, _)| {
                let (p, t) = &self.headlines[i];
                (p.as_path(), t.as_str())
            })
            .collect()
    }

    /// Index of the highlighted row in [`Self::results`].
    #[must_use]
    pub const fn result_cursor(&self) -> usize {
        self.result_cursor
    }

    /// Current input surface.
    #[must_use]
    pub const fn mode(&self) -> AppMode {
        self.mode
    }

    /// The live fuzzy query (empty outside search mode).
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Paths matching the live query, best fuzzy score first. With an
    /// empty query every path is returned in display order.
    #[must_use]
    pub fn results(&self) -> Vec<&Path> {
        let names: Vec<String> = self.paths.iter().map(|p| p.display().to_string()).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        closure_query::fuzzy_filter(&self.query, &name_refs)
            .iter()
            .filter_map(|(name, _)| {
                self.paths
                    .iter()
                    .find(|p| p.display().to_string() == *name)
                    .map(PathBuf::as_path)
            })
            .collect()
    }

    /// The browsable file paths, in display order.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Index of the selected file, if any.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Path of the selected file, if any.
    #[must_use]
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected
            .and_then(|i| self.paths.get(i))
            .map(PathBuf::as_path)
    }

    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// The strokes of the in-progress chord, space-joined; empty when
    /// no chord is pending.
    #[must_use]
    pub fn pending_chord(&self) -> String {
        self.pending.join(" ")
    }

    /// Which-key popup lines (`chord → command`) while a chord prefix
    /// is pending, `None` otherwise.
    #[must_use]
    pub fn popup_lines(&self) -> Option<&[String]> {
        self.popup.as_deref()
    }

    /// Feed one key stroke into the active surface: query editing in
    /// search mode, the chord trie otherwise.
    pub fn handle_stroke(&mut self, stroke: &str) {
        if matches!(self.mode, AppMode::Search | AppMode::SearchHeadlines) {
            self.handle_search_stroke(stroke);
            return;
        }
        if self.mode == AppMode::FileView {
            self.handle_view_stroke(stroke);
            return;
        }
        match self.trie.step(stroke) {
            TrieStep::Resolved(cmd) => {
                self.pending.clear();
                self.popup = None;
                self.apply_command(&cmd);
            }
            TrieStep::Pending(_) => {
                self.pending.push(stroke.to_owned());
                let prefix = self.pending_chord();
                let mut lines: Vec<String> = self
                    .bindings
                    .iter()
                    .filter(|(chord, _)| chord.starts_with(&prefix) && chord.as_str() != prefix)
                    .map(|(chord, cmd)| {
                        let rest = chord[prefix.len()..].trim_start();
                        format!("{rest} → {cmd}")
                    })
                    .collect();
                lines.sort();
                self.popup = Some(lines);
            }
            TrieStep::Unbound => {
                self.pending.clear();
                self.popup = None;
            }
        }
    }

    fn handle_view_stroke(&mut self, stroke: &str) {
        match stroke {
            "j" | "<down>" => {
                let last = self
                    .view_source()
                    .map_or(0, |src| src.lines().count().saturating_sub(1));
                self.scroll = (self.scroll + 1).min(last);
            }
            "k" | "<up>" => self.scroll = self.scroll.saturating_sub(1),
            "ESC" | "q" | "h" | "DEL" => self.mode = AppMode::Browse,
            _ => {}
        }
    }

    fn handle_search_stroke(&mut self, stroke: &str) {
        match stroke {
            "ESC" => {
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "RET" => {
                let pick = if self.mode == AppMode::SearchHeadlines {
                    self.headline_results()
                        .get(self.result_cursor)
                        .map(|(p, _)| p.to_path_buf())
                } else {
                    self.results()
                        .get(self.result_cursor)
                        .copied()
                        .map(Path::to_path_buf)
                };
                let idx = pick.and_then(|pb| self.paths.iter().position(|p| *p == pb));
                if idx.is_some() {
                    self.selected = idx;
                }
                self.mode = AppMode::Browse;
                self.query.clear();
                self.result_cursor = 0;
            }
            "<down>" => {
                let len = if self.mode == AppMode::SearchHeadlines {
                    self.headline_results().len()
                } else {
                    self.results().len()
                };
                self.result_cursor = (self.result_cursor + 1).min(len.saturating_sub(1));
            }
            "<up>" => {
                self.result_cursor = self.result_cursor.saturating_sub(1);
            }
            "DEL" => {
                self.query.pop();
                self.result_cursor = 0;
            }
            "SPC" => {
                self.query.push(' ');
                self.result_cursor = 0;
            }
            s => {
                let mut chars = s.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    self.query.push(c);
                    self.result_cursor = 0;
                }
            }
        }
    }

    fn apply_command(&mut self, cmd: &str) {
        let last = self.paths.len().checked_sub(1);
        match cmd {
            "next-file" => {
                if let (Some(i), Some(last)) = (self.selected, last) {
                    self.selected = Some((i + 1).min(last));
                }
            }
            "prev-file" => {
                if let Some(i) = self.selected {
                    self.selected = Some(i.saturating_sub(1));
                }
            }
            "first-file" if self.selected.is_some() => {
                self.selected = Some(0);
            }
            "last-file" => {
                self.selected = last;
            }
            "quit" => self.quit = true,
            "search-start" => {
                self.mode = AppMode::Search;
                self.query.clear();
            }
            "search-headline-start" => {
                self.mode = AppMode::SearchHeadlines;
                self.query.clear();
            }
            "open-file" => {
                let has_source = self
                    .selected_path()
                    .is_some_and(|sel| self.sources.iter().any(|(p, _)| p.as_path() == sel));
                if has_source {
                    self.mode = AppMode::FileView;
                    self.scroll = 0;
                }
            }
            _ => {}
        }
    }
}

/// Translate a terminal key event into a chord stroke in Emacs/doom
/// notation (`j`, `G`, `SPC`, `C-c`, `M-x`, `RET`, …). Returns `None`
/// for keys the shell does not map.
#[must_use]
pub fn stroke_of(ev: &crossterm::event::KeyEvent) -> Option<String> {
    use crossterm::event::KeyModifiers;
    let base = match ev.code {
        KeyCode::Char(' ') => "SPC".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "ESC".to_owned(),
        KeyCode::Enter => "RET".to_owned(),
        KeyCode::Tab => "TAB".to_owned(),
        KeyCode::Backspace => "DEL".to_owned(),
        KeyCode::Up => "<up>".to_owned(),
        KeyCode::Down => "<down>".to_owned(),
        _ => return None,
    };
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(format!("C-{base}"));
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        return Some(format!("M-{base}"));
    }
    Some(base)
}

/// Render the headline tree of `doc` as indented text lines:
/// `indent * TODO [#P] title :tags:    [id]`.
#[must_use]
pub fn headline_lines(doc: &Document) -> String {
    let mut s = String::new();
    for h in doc.all_headlines() {
        let indent = "  ".repeat(usize::from(h.level()).saturating_sub(1));
        let mut prefix = String::new();
        if let Some(t) = h.todo() {
            prefix.push_str(t);
            prefix.push(' ');
        }
        if let Some(p) = h.priority() {
            let _ = write!(prefix, "[#{p}] ");
        }
        let tags = if h.tags().is_empty() {
            String::new()
        } else {
            format!(" :{}:", h.tags().join(":"))
        };
        let _ = writeln!(
            s,
            "{indent}* {prefix}{title}{tags}    [{id}]",
            title = h.title(),
            id = h.id()
        );
    }
    s
}

/// Run the TUI against an already-loaded vault. Returns when the user
/// quits via `q` or `Esc`.
pub fn run(vault: &Vault) -> Result<(), TuiError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, vault);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    vault: &Vault,
) -> Result<(), TuiError> {
    let mut app = App::new(vault.paths());
    let mut headlines: Vec<(PathBuf, String)> = Vec::new();
    for (path, doc) in vault.iter() {
        for h in doc.all_headlines() {
            headlines.push((path.to_path_buf(), h.title().to_owned()));
        }
    }
    app.set_headlines(headlines);
    let sources: Vec<(PathBuf, String)> = vault
        .iter()
        .map(|(path, doc)| (path.to_path_buf(), doc.source()))
        .collect();
    app.set_sources(sources);

    loop {
        terminal.draw(|f| draw(f, &app, vault))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(stroke) = stroke_of(&key)
        {
            app.handle_stroke(&stroke);
            if app.should_quit() {
                return Ok(());
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame<'_>, app: &App, vault: &Vault) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let file_items: Vec<ListItem<'_>> = app
        .paths()
        .iter()
        .map(|p| ListItem::new(p.display().to_string()))
        .collect();
    let mut file_state = ListState::default();
    file_state.select(app.selected_index());
    let files = List::new(file_items)
        .block(Block::default().title("files").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(files, chunks[0], &mut file_state);

    if app.mode() == AppMode::FileView {
        let src = app.view_source().unwrap_or_default().to_owned();
        let title = app
            .selected_path()
            .map_or_else(String::new, |p| p.display().to_string());
        let offset = u16::try_from(app.scroll()).unwrap_or(u16::MAX);
        let view = Paragraph::new(src)
            .scroll((offset, 0))
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(view, chunks[1]);
    } else {
        let body_text = app
            .selected_path()
            .and_then(|p| vault.document(p))
            .map_or_else(String::new, headline_lines);
        let body = Paragraph::new(body_text)
            .block(Block::default().title("headlines").borders(Borders::ALL));
        f.render_widget(body, chunks[1]);
    }

    if matches!(app.mode(), AppMode::Search | AppMode::SearchHeadlines) {
        let title = if app.mode() == AppMode::Search {
            format!("find file: {}", app.query())
        } else {
            format!("find headline: {}", app.query())
        };
        let height = area.height / 2;
        let search_area = ratatui::layout::Rect {
            x: area.x,
            y: area.height.saturating_sub(height),
            width: area.width,
            height,
        };
        let items: Vec<ListItem<'_>> = if app.mode() == AppMode::Search {
            app.results()
                .iter()
                .map(|p| ListItem::new(p.display().to_string()))
                .collect()
        } else {
            app.headline_results()
                .iter()
                .map(|(p, t)| ListItem::new(format!("{t}    ({})", p.display())))
                .collect()
        };
        let mut state = ListState::default();
        state.select(Some(app.result_cursor()));
        let pane = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        f.render_widget(ratatui::widgets::Clear, search_area);
        f.render_stateful_widget(pane, search_area, &mut state);
    }

    if let Some(lines) = app.popup_lines() {
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let popup_area = ratatui::layout::Rect {
            x: area.x,
            y: area.height.saturating_sub(height.saturating_add(2)),
            width: area.width,
            height: height.saturating_add(2).min(area.height),
        };
        let title = format!("which-key: {}", app.pending_chord());
        let popup = Paragraph::new(lines.join("\n"))
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(ratatui::widgets::Clear, popup_area);
        f.render_widget(popup, popup_area);
    }
}
