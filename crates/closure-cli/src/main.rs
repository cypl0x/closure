//! `closure` command-line entrypoint.
//!
//! Subcommands:
//! * `parse <file>` — print a short summary of the parsed document.
//! * `fmt <file>` — roundtrip the file through parse/print and write
//!   back byte-exact (I1).
//! * `check <vault>` — validate that every file in the vault roundtrips
//!   byte-exact.
//! * `query <vault>` — filter headlines across a vault by tag, todo,
//!   title substring, or level.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use closure_config::InputMode;
use closure_core::{
    AddSibling, BlockId, Command, Demote, Document, EnsureId, MoveSubtree, Promote, Registry,
    RemoveSubtree, RenameHeadline, SetBody, SetPlanning, SetPriority, SetProperty, SetTags,
    SetTodo, ToggleArchive, ToggleComment,
};
use closure_eval::{Backend, ShellBackend, backend_for};
use closure_input::Dispatcher;
use closure_org::{NodeKind, parse};
use closure_store::Vault;

#[derive(Parser, Debug)]
#[command(version, about = "closure: a local-first PKM kernel")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Launch the TUI shell against a vault.
    Tui {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Parse a single org file and print a summary.
    Parse {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Parse then print a single file; writes back byte-exact source.
    Fmt {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Validate that every file under a vault roundtrips byte-exact.
    Check {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Query headlines across a vault.
    Query {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Filter by tag.
        #[arg(long)]
        tag: Option<String>,
        /// Filter by TODO keyword.
        #[arg(long)]
        todo: Option<String>,
        /// Filter by title substring.
        #[arg(long)]
        title: Option<String>,
        /// Filter by nesting level.
        #[arg(long)]
        level: Option<u8>,
    },
    /// Print which-key bindings for the default registry.
    Whichkey {
        /// Optional prefix filter (e.g. `C-c`).
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Evaluate every shell code block in the given file and print its
    /// output.
    Eval {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Write `#+RESULTS:` blocks back to the source file after
        /// each evaluated code block.
        #[arg(long)]
        write: bool,
    },
    /// Print headlines that link to the given block id.
    Backlinks {
        /// Path to the vault directory.
        vault: PathBuf,
        /// ULID of the target block.
        id: String,
    },
    /// Ensure every headline in the given file has a persisted `:ID:`
    /// property. Writes a fresh drawer when absent.
    Id {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print a Notion-style table view (id, level, title, todo) for
    /// every headline in the vault.
    Db {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Serve the vault on a localhost HTTP port (read-only).
    Serve {
        /// Path to the vault directory.
        vault: PathBuf,
        /// `host:port` to bind. Defaults to `127.0.0.1:7878`.
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: String,
    },
    /// Sync the vault via git: push (commit and push) or pull
    /// (`git pull --rebase`).
    Sync {
        /// Path to the vault directory.
        vault: PathBuf,
        /// One of `push` or `pull`.
        #[arg(long, default_value = "push")]
        op: String,
        /// Optional commit message (push only).
        #[arg(long)]
        message: Option<String>,
    },
    /// Rename the headline with the given block id.
    Rename {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// New title text.
        title: String,
    },
    /// Set or clear the TODO keyword on the headline with the given id.
    SetTodo {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// Keyword to set (empty string clears).
        keyword: String,
    },
    /// Watch the vault for `*.org` file changes and stream events.
    Watch {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Set or clear the `[#X]` priority on a headline.
    SetPriority {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// Priority letter (`A`..`Z`); empty string clears.
        priority: String,
    },
    /// Replace the trailing tag list on a headline.
    SetTags {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// Comma-separated tags (empty clears).
        tags: String,
    },
    /// Promote (decrease level) the headline with the given id.
    Promote {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Demote (increase level) the headline with the given id.
    Demote {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Insert a new sibling headline after the given id.
    AddSibling {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the headline this sibling sits after.
        after_id: String,
        /// Title for the new headline.
        title: String,
    },
    /// Remove the subtree rooted at the given headline id (header,
    /// body, drawer, and descendants).
    Remove {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Print summary statistics for a vault.
    Stats {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Move the subtree of `id` to immediately after `after_id`.
    Move {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the headline being moved.
        id: String,
        /// Block id of the new predecessor.
        after_id: String,
    },
    /// Replace a headline's body wholesale.
    SetBody {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// New body text (use empty string to clear).
        body: String,
    },
    /// Full-text search across the vault (title + body).
    Search {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Substring to look for (case-sensitive).
        needle: String,
    },
    /// Load a closure-config block from an org file and print the
    /// resolved settings.
    Config {
        /// Path to a `*.org` file containing a `#+BEGIN_SRC
        /// closure-config` block.
        path: PathBuf,
    },
    /// Print the 10 spec invariants closure enforces.
    Spec,
    /// Print a sample `#+BEGIN_SRC closure-config` block.
    DefaultConfig,
    /// Create a new `*.org` file under a vault with one headline.
    New {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Relative path of the new file (e.g. `notes/today.org`).
        path: PathBuf,
        /// Title of the initial headline.
        title: String,
    },
    /// Send a prompt to an Anthropic-compatible endpoint.
    /// Reads `ANTHROPIC_API_KEY` from the environment.
    Ask {
        /// Prompt text to send.
        prompt: String,
        /// Model id (defaults to `claude-sonnet-4-6`).
        #[arg(long, default_value = "claude-sonnet-4-6")]
        model: String,
    },
    /// Print tag occurrence counts in descending order.
    TagCloud {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print just the headline outline of an org file (no metadata).
    Outline {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Delete a `*.org` file from a vault.
    DeleteFile {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Path to the file to delete.
        file: PathBuf,
    },
    /// Rename a `*.org` file inside a vault.
    RenameFile {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Current path of the file.
        from: PathBuf,
        /// New relative path inside the vault.
        to: PathBuf,
    },
    /// Print every TODO headline across the vault, sorted by
    /// priority (A first) then by file path.
    Agenda {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every distinct tag in the vault (sorted).
    Tags {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every distinct TODO keyword in the vault (sorted).
    Todos {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Count words / characters / headlines in an org file.
    Wc {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Toggle the `:ARCHIVE:` tag on a headline.
    Archive {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// List every `:NAME:` ... `:END:` drawer in a file.
    Drawers {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every `|...|` table in a file with row/col counts.
    Tables {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every link target found in a file.
    Links {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every footnote (reference or inline definition) in a file.
    Footnotes {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Toggle the checkbox on the Nth preamble list item.
    ToggleCheckbox {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Zero-based list item index in the preamble.
        index: usize,
    },
    /// Toggle the `COMMENT` keyword prefix on a headline.
    Comment {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Set a `:KEY: value` property on a headline.
    SetProperty {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// Property key (e.g. `EFFORT`).
        key: String,
        /// Property value.
        value: String,
    },
    /// Print document-level keywords (TITLE/AUTHOR/DATE/FILETAGS).
    Meta {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Set or clear the planning line (SCHEDULED/DEADLINE/CLOSED).
    SetPlanning {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// SCHEDULED timestamp (e.g. `<2026-04-25 Sat>`); empty to clear.
        #[arg(long, default_value = "")]
        scheduled: String,
        /// DEADLINE timestamp; empty to clear.
        #[arg(long, default_value = "")]
        deadline: String,
        /// CLOSED timestamp; empty to clear.
        #[arg(long, default_value = "")]
        closed: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli.cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: &Cmd) -> Result<(), String> {
    match cmd {
        Cmd::Tui { vault } => cmd_tui(vault),
        Cmd::Parse { file } => cmd_parse(file),
        Cmd::Fmt { file } => cmd_fmt(file),
        Cmd::Check { vault } => cmd_check(vault),
        Cmd::Query {
            vault,
            tag,
            todo,
            title,
            level,
        } => cmd_query(
            vault,
            tag.as_deref(),
            todo.as_deref(),
            title.as_deref(),
            *level,
        ),
        Cmd::Whichkey { prefix } => cmd_whichkey(prefix.as_deref()),
        Cmd::Eval { file, write } => cmd_eval(file, *write),
        Cmd::Backlinks { vault, id } => cmd_backlinks(vault, id),
        Cmd::Id { file } => cmd_id(file),
        Cmd::Db { vault } => cmd_db(vault),
        Cmd::Serve { vault, addr } => cmd_serve(vault, addr),
        Cmd::Sync { vault, op, message } => cmd_sync(vault, op, message.as_deref()),
        Cmd::Rename { file, id, title } => cmd_rename(file, id, title),
        Cmd::SetTodo { file, id, keyword } => cmd_set_todo(file, id, keyword),
        Cmd::Watch { vault } => cmd_watch(vault),
        Cmd::SetPriority { file, id, priority } => cmd_set_priority(file, id, priority),
        Cmd::SetTags { file, id, tags } => cmd_set_tags(file, id, tags),
        Cmd::Promote { file, id } => cmd_promote(file, id),
        Cmd::Demote { file, id } => cmd_demote(file, id),
        Cmd::AddSibling {
            file,
            after_id,
            title,
        } => cmd_add_sibling(file, after_id, title),
        Cmd::Remove { file, id } => cmd_remove(file, id),
        Cmd::Stats { vault } => cmd_stats(vault),
        Cmd::Move { file, id, after_id } => cmd_move(file, id, after_id),
        Cmd::SetBody { file, id, body } => cmd_set_body(file, id, body),
        Cmd::Search { vault, needle } => cmd_search(vault, needle),
        Cmd::Config { path } => cmd_config(path),
        Cmd::Spec => cmd_spec(),
        Cmd::DefaultConfig => cmd_default_config(),
        Cmd::New { vault, path, title } => cmd_new(vault, path, title),
        Cmd::Ask { prompt, model } => cmd_ask(prompt, model),
        Cmd::TagCloud { vault } => cmd_tag_cloud(vault),
        Cmd::Outline { file } => cmd_outline(file),
        Cmd::DeleteFile { vault, file } => cmd_delete_file(vault, file),
        Cmd::RenameFile { vault, from, to } => cmd_rename_file(vault, from, to),
        Cmd::Agenda { vault } => cmd_agenda(vault),
        Cmd::Tags { vault } => cmd_tags(vault),
        Cmd::Todos { vault } => cmd_todos(vault),
        Cmd::SetPlanning {
            file,
            id,
            scheduled,
            deadline,
            closed,
        } => cmd_set_planning(file, id, scheduled, deadline, closed),
        Cmd::Meta { file } => cmd_meta(file),
        Cmd::Wc { file } => cmd_wc(file),
        Cmd::Archive { file, id } => cmd_archive(file, id),
        Cmd::Comment { file, id } => cmd_comment(file, id),
        Cmd::ToggleCheckbox { file, index } => cmd_toggle_checkbox(file, *index),
        Cmd::Drawers { file } => cmd_drawers(file),
        Cmd::Tables { file } => cmd_tables(file),
        Cmd::Links { file } => cmd_links(file),
        Cmd::Footnotes { file } => cmd_footnotes(file),
        Cmd::SetProperty {
            file,
            id,
            key,
            value,
        } => cmd_set_property(file, id, key, value),
    }
}

fn cmd_set_property(path: &Path, id: &str, key: &str, value: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let cmd = SetProperty::new(BlockId::from_existing(id), key.to_owned(), value.to_owned());
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_archive(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let cmd = ToggleArchive::new(BlockId::from_existing(id));
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_links(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for l in closure_org::find_links(&src) {
        match l.description {
            Some(d) => println!("{}\t{d}", l.target),
            None => println!("{}", l.target),
        }
    }
    Ok(())
}

fn cmd_footnotes(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for f in closure_org::find_footnotes(&src) {
        match f.definition {
            Some(d) => println!("[fn:{}]\t{d}", f.name),
            None => println!("[fn:{}]", f.name),
        }
    }
    Ok(())
}

fn cmd_tables(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for (i, t) in doc.tables().iter().enumerate() {
        let cols = t.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        println!("table #{i}: {} rows, {} cols", t.rows.len(), cols);
        for r in &t.rows {
            if r.is_separator {
                println!("  |---|");
            } else {
                println!("  | {} |", r.cells.join(" | "));
            }
        }
    }
    Ok(())
}

fn cmd_drawers(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for d in closure_org::find_drawers(&src) {
        let bytes = d.content.len();
        let lines = d.content.lines().count();
        println!(":{}:  {} lines, {} bytes", d.name, lines, bytes);
    }
    Ok(())
}

fn cmd_toggle_checkbox(path: &Path, index: usize) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let new = closure_org::rewrite_toggle_checkbox(&doc, index).map_err(|e| format!("{e}"))?;
    fs::write(path, closure_org::print(&new)).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_comment(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let cmd = ToggleComment::new(BlockId::from_existing(id));
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_wc(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let chars = src.chars().count();
    let words = src.split_whitespace().count();
    let lines = src.lines().count();
    let headlines = doc.all_headlines().count();
    println!("chars: {chars}");
    println!("words: {words}");
    println!("lines: {lines}");
    println!("headlines: {headlines}");
    Ok(())
}

fn cmd_meta(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(t) = doc.title() {
        println!("title: {t}");
    }
    if let Some(a) = doc.author() {
        println!("author: {a}");
    }
    if let Some(d) = doc.date() {
        println!("date: {d}");
    }
    let tags = doc.filetags();
    if !tags.is_empty() {
        println!("filetags: {}", tags.join(", "));
    }
    Ok(())
}

fn cmd_set_planning(
    path: &Path,
    id: &str,
    scheduled: &str,
    deadline: &str,
    closed: &str,
) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let s = (!scheduled.is_empty()).then(|| scheduled.to_owned());
    let d = (!deadline.is_empty()).then(|| deadline.to_owned());
    let c = (!closed.is_empty()).then(|| closed.to_owned());
    let cmd = SetPlanning::new(BlockId::from_existing(id), s, d, c);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_tags(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for t in v.all_tags() {
        println!("{t}");
    }
    Ok(())
}

fn cmd_todos(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for t in v.all_todos() {
        println!("{t}");
    }
    Ok(())
}

fn cmd_agenda(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut items: Vec<(char, String, String, String, String)> = Vec::new();
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            if h.tags().iter().any(|t| t == "ARCHIVE") || h.is_comment() {
                continue;
            }
            if h.todo().is_some() {
                let mut planning = String::new();
                if let Some(s) = h.scheduled() {
                    planning.push_str(" SCHEDULED:");
                    planning.push_str(s);
                }
                if let Some(d) = h.deadline() {
                    planning.push_str(" DEADLINE:");
                    planning.push_str(d);
                }
                items.push((
                    h.priority().unwrap_or('Z'),
                    path.display().to_string(),
                    h.todo().unwrap_or("").to_owned(),
                    h.title().to_owned(),
                    planning,
                ));
            }
        }
    }
    items.sort();
    for (prio, path, todo, title, planning) in items {
        let prio_marker = if prio == 'Z' {
            "    ".to_owned()
        } else {
            format!("[#{prio}]")
        };
        println!("{prio_marker} {todo:5} {title}{planning}  ({path})");
    }
    Ok(())
}

fn cmd_delete_file(vault: &Path, file: &Path) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    v.delete_file(file).map_err(|e| format!("{e}"))?;
    Ok(())
}

fn cmd_rename_file(vault: &Path, from: &Path, to: &Path) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let new_path = v.rename_file(from, to).map_err(|e| format!("{e}"))?;
    println!("{}", new_path.display());
    Ok(())
}

fn cmd_outline(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    for h in doc.all_headlines() {
        let indent = "  ".repeat(usize::from(h.level()).saturating_sub(1));
        let stars = "*".repeat(usize::from(h.level()));
        println!("{indent}{stars} {}", h.title());
    }
    Ok(())
}

fn cmd_tag_cloud(vault: &Path) -> Result<(), String> {
    use std::collections::BTreeMap;
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, doc) in v.iter() {
        for h in doc.all_headlines() {
            for t in h.tags() {
                *counts.entry(t.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut pairs: Vec<(&String, &usize)> = counts.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (tag, n) in pairs {
        println!("{n:>4}  {tag}");
    }
    Ok(())
}

fn cmd_ask(prompt: &str, model: &str) -> Result<(), String> {
    use closure_llm::Provider;
    let key =
        std::env::var("ANTHROPIC_API_KEY").map_err(|_| "ANTHROPIC_API_KEY not set".to_owned())?;
    let provider = closure_llm::anthropic(&key, model);
    let response = provider.complete(prompt).map_err(|e| format!("{e}"))?;
    println!("{response}");
    Ok(())
}

fn cmd_new(vault: &Path, path: &Path, title: &str) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let id = BlockId::fresh();
    let source = format!("* {title}\n:PROPERTIES:\n:ID: {id}\n:END:\n");
    let abs = v.create_file(path, &source).map_err(|e| format!("{e}"))?;
    println!("{} ({})", abs.display(), id);
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_default_config() -> Result<(), String> {
    println!("#+TITLE: closure config");
    println!();
    println!("#+BEGIN_SRC closure-config");
    println!("input_mode = doom");
    println!("theme = default");
    println!("# default_vault = ~/notes");
    println!("# todo_keywords = TODO, DOING, DONE");
    println!("#+END_SRC");
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_spec() -> Result<(), String> {
    let lines = [
        "I1  byte-exact roundtrip on the golden corpus",
        "I2  stable BlockId (ULID) survives parse/print/CRDT merges",
        "I3  every mutation undoable via Edit + branching UndoTree",
        "I4  every command carries a keybinding (whichkey reads registry)",
        "I5  no panics in kernel crates (forbid unsafe, deny unwrap/expect, fuzz)",
        "I6  determinism for parse/print/queries",
        "I7  shells consume closure-core only; spans pub(crate) firewall",
        "I8  command-registry is the only side-effect surface",
        "I9  config validation at load, not at use (typed schema)",
        "I10 deterministic / hermetic / reproducible builds (nix flake check)",
    ];
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

fn cmd_config(path: &Path) -> Result<(), String> {
    let cfg = closure_config::Config::from_path(path).map_err(|e| format!("{e}"))?;
    println!("input_mode:    {:?}", cfg.input_mode);
    println!("theme:         {}", cfg.theme);
    if let Some(v) = &cfg.default_vault {
        println!("default_vault: {}", v.display());
    }
    println!("todo_keywords: {}", cfg.todo_keywords.join(", "));
    Ok(())
}

fn cmd_search(vault: &Path, needle: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for m in closure_query::full_text(&v, needle) {
        println!(
            "{}:{}:{}",
            m.path.display(),
            m.headline.level(),
            m.headline.title()
        );
    }
    Ok(())
}

fn cmd_set_body(path: &Path, id: &str, body: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let cmd = SetBody::new(BlockId::from_existing(id), body.to_owned());
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_move(path: &Path, id: &str, after_id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let cmd = MoveSubtree::new(BlockId::from_existing(id), BlockId::from_existing(after_id));
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_stats(vault: &Path) -> Result<(), String> {
    use std::collections::BTreeMap;
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut by_level: BTreeMap<u8, usize> = BTreeMap::new();
    let mut by_todo: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_tag: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    for (_, doc) in v.iter() {
        for h in doc.all_headlines() {
            total += 1;
            *by_level.entry(h.level()).or_insert(0) += 1;
            if let Some(t) = h.todo() {
                *by_todo.entry(t.to_owned()).or_insert(0) += 1;
            }
            for tag in h.tags() {
                *by_tag.entry(tag.clone()).or_insert(0) += 1;
            }
        }
    }
    println!("files:     {}", v.len());
    println!("headlines: {total}");
    println!("by level:");
    for (lvl, n) in &by_level {
        println!("  L{lvl}: {n}");
    }
    if !by_todo.is_empty() {
        println!("by todo:");
        for (k, n) in &by_todo {
            println!("  {k}: {n}");
        }
    }
    if !by_tag.is_empty() {
        println!("by tag:");
        for (k, n) in &by_tag {
            println!("  {k}: {n}");
        }
    }
    Ok(())
}

fn cmd_remove(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let cmd = RemoveSubtree::new(bid);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_add_sibling(path: &Path, after_id: &str, title: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(after_id);
    let cmd = AddSibling::new(bid, title.to_owned());
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_promote(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let cmd = Promote::new(bid);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_demote(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let cmd = Demote::new(bid);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_set_priority(path: &Path, id: &str, priority: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let new = priority.chars().next();
    let cmd = SetPriority::new(bid, new);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_set_tags(path: &Path, id: &str, tags: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let new: Vec<String> = if tags.is_empty() {
        Vec::new()
    } else {
        tags.split(',').map(|s| s.trim().to_owned()).collect()
    };
    let cmd = SetTags::new(bid, new);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_watch(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let watcher = v.watch().map_err(|e| format!("{e}"))?;
    eprintln!("closure watch: streaming events from {}", vault.display());
    loop {
        let event = watcher.recv().map_err(|e| format!("{e}"))?;
        println!("{:?} {}", event.kind, event.path.display());
    }
}

fn cmd_rename(path: &Path, id: &str, title: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let cmd = RenameHeadline::new(bid, title.to_owned());
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_set_todo(path: &Path, id: &str, keyword: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let new = if keyword.is_empty() {
        None
    } else {
        Some(keyword.to_owned())
    };
    let cmd = SetTodo::new(bid, new);
    Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_sync(vault: &Path, op: &str, message: Option<&str>) -> Result<(), String> {
    use closure_sync::Transport;
    let mut t = closure_sync::GitTransport::new(vault.to_path_buf());
    if let Some(m) = message {
        m.clone_into(&mut t.commit_message);
    }
    match op {
        "push" => t.push().map_err(|e| format!("{e}")),
        "pull" => t.pull().map_err(|e| format!("{e}")),
        other => Err(format!("unknown sync op: {other}")),
    }
}

fn cmd_db(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let view = closure_query::DatabaseView::default_view(closure_query::all_headlines(&v));
    println!("{}", view.columns.join("\t"));
    for row in view.cells() {
        println!("{}", row.join("\t"));
    }
    Ok(())
}

fn cmd_serve(vault: &Path, addr: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    eprintln!("closure serve: listening on http://{addr}");
    closure_shell_web::serve(&v, addr).map_err(|e| format!("{e}"))
}

fn cmd_backlinks(vault: &Path, id: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    for m in closure_query::backlinks(&v, &bid) {
        println!(
            "{}:{}:{}",
            m.path.display(),
            m.headline.level(),
            m.headline.title()
        );
    }
    Ok(())
}

fn cmd_id(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let ids: Vec<BlockId> = doc.all_block_ids();
    for id in ids {
        let cmd = EnsureId::new(id);
        Command::apply(&cmd, &mut doc).map_err(|e| format!("{e}"))?;
    }
    fs::write(path, doc.source()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn default_registry() -> Registry {
    closure_core::default_registry()
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_whichkey(prefix: Option<&str>) -> Result<(), String> {
    let reg = default_registry();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let out = prefix.map_or_else(
        || closure_whichkey::render(&disp),
        |p| closure_whichkey::render_prefix(&disp, p),
    );
    print!("{out}");
    Ok(())
}

fn cmd_eval(path: &Path, write: bool) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = parse(&src).map_err(|e| format!("{e}"))?;
    let mut ran = 0usize;
    let mut results: Vec<(usize, String)> = Vec::new();
    for n in doc.preamble() {
        if n.kind() != NodeKind::CodeBlock {
            continue;
        }
        let Some(cb) = n.as_code_block() else {
            continue;
        };
        let backend: Box<dyn Backend> = if let Some(lang) = cb.language {
            if let Some(b) = backend_for(lang) {
                b
            } else {
                eprintln!("---- block #{ran} skipped (no backend for `{lang}`) ----");
                ran += 1;
                continue;
            }
        } else {
            Box::new(ShellBackend)
        };
        let out = backend.eval(cb.content).map_err(|e| format!("{e}"))?;
        println!(
            "---- block #{ran} {lang} exit={} ----",
            out.exit,
            lang = cb.language.unwrap_or("shell")
        );
        if !out.stdout.is_empty() {
            print!("{}", out.stdout);
        }
        if !out.stderr.is_empty() {
            eprint!("{}", out.stderr);
        }
        if write {
            results.push((ran, out.stdout.clone()));
        }
        ran += 1;
    }
    if ran == 0 {
        eprintln!("no code blocks found");
        return Ok(());
    }
    if write {
        let mut current = doc;
        for (idx, output) in results {
            current = closure_org::rewrite_attach_results_to_code_block(&current, idx, &output)
                .map_err(|e| format!("attach results: {e}"))?;
        }
        fs::write(path, closure_org::print(&current)).map_err(|e| format!("write: {e}"))?;
    }
    Ok(())
}

fn cmd_tui(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_tui::run(&v).map_err(|e| format!("{e}"))
}

fn cmd_parse(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let n_roots = doc.roots().len();
    let n_headlines = doc.all_headlines().count();
    println!(
        "{}: {n_roots} root(s), {n_headlines} headline(s)",
        path.display()
    );
    for h in doc.all_headlines() {
        let indent = "  ".repeat(usize::from(h.level()) - 1);
        println!("{indent}* [{id}] {title}", id = h.id(), title = h.title());
    }
    Ok(())
}

fn cmd_fmt(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let out = doc.source();
    if out != src {
        return Err(format!("{}: roundtrip mismatch", path.display()));
    }
    fs::write(path, out).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn cmd_check(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut failures = 0usize;
    for (path, doc) in v.iter() {
        let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if doc.source() != src {
            failures += 1;
            eprintln!("FAIL: {}", path.display());
        }
    }
    if failures > 0 {
        return Err(format!("{failures} file(s) failed roundtrip"));
    }
    println!("OK: {} file(s) roundtripped", v.len());
    Ok(())
}

fn cmd_query(
    vault: &Path,
    tag: Option<&str>,
    todo: Option<&str>,
    title: Option<&str>,
    level: Option<u8>,
) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut matches = closure_query::all_headlines(&v);
    if let Some(t) = tag {
        matches.retain(|m| m.headline.tags().iter().any(|x| x == t));
    }
    if let Some(t) = todo {
        matches.retain(|m| m.headline.todo() == Some(t));
    }
    if let Some(t) = title {
        matches.retain(|m| m.headline.title().contains(t));
    }
    if let Some(l) = level {
        matches.retain(|m| m.headline.level() == l);
    }
    for m in &matches {
        println!(
            "{}:{}:{}",
            m.path.display(),
            m.headline.level(),
            m.headline.title()
        );
    }
    Ok(())
}
