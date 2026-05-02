//! ratatui + crossterm shell for the closure kernel.
//!
//! The current shell is a read-only vault browser: it shows the file
//! list on the left and the headlines of the selected file on the
//! right. Modal editing, command palette, and which-key land in later
//! milestones.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

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
