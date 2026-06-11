//! ratatui + crossterm shell for the closure kernel.
//!
//! The current shell is a read-only vault browser: it shows the file
//! list on the left and the headlines of the selected file on the
//! right. Modal editing, command palette, and which-key land in later
//! milestones.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    ("g g", "first-file"),
    ("G", "last-file"),
    ("q", "quit"),
];

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
        }
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

    /// Feed one key stroke into the chord trie and apply whatever it
    /// resolves to.
    pub fn handle_stroke(&mut self, stroke: &str) {
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
                    .filter(|(chord, _)| {
                        chord.starts_with(&prefix) && chord.as_str() != prefix
                    })
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
            _ => {}
        }
    }
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
    let paths: Vec<PathBuf> = vault.paths();
    let mut file_state = ListState::default();
    if !paths.is_empty() {
        file_state.select(Some(0));
    }

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(area);

            let file_items: Vec<ListItem<'_>> = paths
                .iter()
                .map(|p| ListItem::new(p.display().to_string()))
                .collect();
            let files = List::new(file_items)
                .block(Block::default().title("files").borders(Borders::ALL))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            f.render_stateful_widget(files, chunks[0], &mut file_state);

            let body_text = file_state
                .selected()
                .and_then(|i| paths.get(i))
                .and_then(|p| vault.document(p))
                .map_or_else(String::new, |doc| {
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
                });
            let body = Paragraph::new(body_text)
                .block(Block::default().title("headlines").borders(Borders::ALL));
            f.render_widget(body, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = file_state.selected().unwrap_or(0);
                    if !paths.is_empty() && i + 1 < paths.len() {
                        file_state.select(Some(i + 1));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = file_state.selected().unwrap_or(0);
                    if i > 0 {
                        file_state.select(Some(i - 1));
                    }
                }
                _ => {}
            }
        }
    }
}
