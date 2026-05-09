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
    /// Print headline tree with todo/priority/tags annotations.
    Tree {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the first N headlines (titles only).
    Head {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Number of headlines to show (default 10).
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Print the subtree source rooted at a block id (verbatim).
    Subtree {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the subtree root.
        id: String,
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
    /// Print every list group (consecutive `-`/`+`/`1.` items) in a file.
    Lists {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print unfinished checkbox items across the vault.
    Pending {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every named block (`#+BEGIN_QUOTE`, `#+BEGIN_EXAMPLE`, etc.).
    Blocks {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every `<<anchor>>` and `<<<radio>>>` target in a file.
    Anchors {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every link target found in a file.
    Links {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print headlines whose title/body links to `target` (in one file).
    LinksTo {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Target id (`id:<ULID>` or bare ULID).
        target: String,
    },
    /// Print every footnote (reference or inline definition) in a file.
    Footnotes {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every timestamp (active/inactive) in a file.
    Timestamps {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every progress cookie (`[N/M]`, `[N%]`) in a file.
    Cookies {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every macro invocation (`{{{name(args)}}}`) in a file.
    Macros {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every block id in a file with its title.
    Ids {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print code-block header args for every `#+BEGIN_SRC` block.
    BlockArgs {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every `#+KEY: value` preamble keyword line in a file.
    Keywords {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print properties drawer entries for every headline in a file.
    Properties {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Verify byte-exact roundtrip (parse+print) of a file (I1 check).
    Validate {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print per-file stats: headlines, depth, words, link count.
    StatsFile {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print top-level vault summary: files, headlines, words, top tag.
    VaultInfo {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Run the MCP stdio dispatcher (one command name per line; `LIST`
    /// to enumerate). Quits on EOF.
    Mcp,
    /// Print headlines that have no incoming `id:` links.
    Orphans {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print `id:` links whose targets do not resolve inside the vault.
    DeadLinks {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print the top-N headlines by incoming `id:` link count.
    Hubs {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Number of top hubs to print (default 10).
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Print every CLOCK entry inside :LOGBOOK: drawers in a file.
    Clock {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every command name registered in the default registry.
    Commands,
    /// Parse `#+BEGIN_SRC closure-cron` block of a file, print jobs.
    CronList {
        /// Path to a `*.org` file containing cron blocks.
        file: PathBuf,
    },
    /// Print jobs from a cron block that match the given wall-clock time.
    CronTick {
        /// Path to a `*.org` file containing cron blocks.
        file: PathBuf,
        /// Minute (0-59).
        minute: u8,
        /// Hour (0-23).
        hour: u8,
        /// Day-of-month (1-31).
        dom: u8,
        /// Month (1-12).
        month: u8,
        /// Day-of-week (0-6).
        dow: u8,
    },
    /// Print the closure-cli crate version.
    Version,
    /// Print the keybinding(s) registered for a command name.
    WhereIs {
        /// Command name (e.g. `rename-headline`).
        name: String,
    },
    /// Print all tags on a single headline.
    TagsOf {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Print every evaluator language alias the build supports.
    Languages,
    /// Print the maximum headline nesting depth in a file.
    Depth {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print preamble + body node counts for a file.
    Nodes {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every leaf headline (no children) in a file.
    Leaves {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every root-level headline (level 1) in a file.
    Roots {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print every archived headline (those carrying :ARCHIVE:) in a vault.
    Archived {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print headlines tagged with EVERY given tag (AND).
    Tagged {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Tags (positional, one or more).
        tags: Vec<String>,
    },
    /// Print headlines tagged with ANY given tag (OR).
    TaggedAny {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Tags (positional, one or more).
        tags: Vec<String>,
    },
    /// Print every COMMENT-prefixed headline in a vault.
    CommentList {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Find the first headline whose title matches (case-insensitive).
    FindTitle {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Title to match.
        title: String,
    },
    /// Find the headline with `:ID:` matching across the vault.
    FindId {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Block id to look up.
        id: String,
    },
    /// Print detailed information about a single headline.
    Info {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Print the index path (root, child, …) to a headline by id.
    PathOf {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Print body text of a single headline by id.
    Body {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
    },
    /// Print TODO keyword occurrence counts ranked descending.
    TodoCloud {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Append a line to a headline's `:LOGBOOK:` drawer.
    LogbookAppend {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the target headline.
        id: String,
        /// Single-line entry to append (no trailing newline).
        entry: String,
    },
    /// Print every `*.org` file path in a vault, sorted.
    Paths {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// List vault files sorted by mtime, most-recently-modified first.
    Recent {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Number of entries to show (default 20).
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print the FNV-1a source hash of an org file (cache-keying).
    Hash {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Emit a Graphviz `dot` document of every `id:` link in a vault.
    Graph {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print a deterministic "random" headline picked from the vault.
    /// Useful for daily-review prompts (`#+BEGIN: dynamic block` style).
    Random {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Seed (e.g. today's date as `YYYY-MM-DD`); same seed = same pick.
        seed: String,
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

#[allow(clippy::too_many_lines)]
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
        Cmd::Tree { file } => cmd_tree(file),
        Cmd::Head { file, limit } => cmd_head(file, *limit),
        Cmd::Subtree { file, id } => cmd_subtree(file, id),
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
        Cmd::Lists { file } => cmd_lists(file),
        Cmd::Pending { vault } => cmd_pending(vault),
        Cmd::Blocks { file } => cmd_blocks(file),
        Cmd::Anchors { file } => cmd_anchors(file),
        Cmd::Links { file } => cmd_links(file),
        Cmd::LinksTo { file, target } => cmd_links_to(file, target),
        Cmd::Footnotes { file } => cmd_footnotes(file),
        Cmd::Timestamps { file } => cmd_timestamps(file),
        Cmd::Cookies { file } => cmd_cookies(file),
        Cmd::Macros { file } => cmd_macros(file),
        Cmd::Ids { file } => cmd_ids(file),
        Cmd::BlockArgs { file } => cmd_block_args(file),
        Cmd::Keywords { file } => cmd_keywords(file),
        Cmd::Properties { file } => cmd_properties(file),
        Cmd::Validate { file } => cmd_validate(file),
        Cmd::StatsFile { file } => cmd_stats_file(file),
        Cmd::VaultInfo { vault } => cmd_vault_info(vault),
        Cmd::Mcp => cmd_mcp(),
        Cmd::Orphans { vault } => cmd_orphans(vault),
        Cmd::DeadLinks { vault } => cmd_dead_links(vault),
        Cmd::Hubs { vault, limit } => cmd_hubs(vault, *limit),
        Cmd::Clock { file } => cmd_clock(file),
        Cmd::Commands => cmd_commands(),
        Cmd::CronList { file } => cmd_cron_list(file),
        Cmd::CronTick {
            file,
            minute,
            hour,
            dom,
            month,
            dow,
        } => cmd_cron_tick(file, *minute, *hour, *dom, *month, *dow),
        Cmd::Version => cmd_version(),
        Cmd::WhereIs { name } => cmd_where_is(name),
        Cmd::TagsOf { file, id } => cmd_tags_of(file, id),
        Cmd::Languages => cmd_languages(),
        Cmd::LogbookAppend { file, id, entry } => cmd_logbook_append(file, id, entry),
        Cmd::Depth { file } => cmd_depth(file),
        Cmd::Nodes { file } => cmd_nodes(file),
        Cmd::Leaves { file } => cmd_leaves(file),
        Cmd::Roots { file } => cmd_roots(file),
        Cmd::Archived { vault } => cmd_archived(vault),
        Cmd::Tagged { vault, tags } => cmd_tagged(vault, tags),
        Cmd::TaggedAny { vault, tags } => cmd_tagged_any(vault, tags),
        Cmd::CommentList { vault } => cmd_comment_list(vault),
        Cmd::FindTitle { vault, title } => cmd_find_title(vault, title),
        Cmd::FindId { vault, id } => cmd_find_id(vault, id),
        Cmd::Info { file, id } => cmd_info(file, id),
        Cmd::PathOf { file, id } => cmd_path_of(file, id),
        Cmd::Body { file, id } => cmd_body(file, id),
        Cmd::TodoCloud { vault } => cmd_todo_cloud(vault),
        Cmd::Paths { vault } => cmd_paths(vault),
        Cmd::Recent { vault, limit } => cmd_recent(vault, *limit),
        Cmd::Hash { file } => cmd_hash(file),
        Cmd::Graph { vault } => cmd_graph(vault),
        Cmd::Random { vault, seed } => cmd_random(vault, seed),
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

fn cmd_timestamps(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for t in closure_org::find_timestamps(&src) {
        let kind = if t.active { "active" } else { "inactive" };
        println!("{kind}\t{}", t.content);
    }
    Ok(())
}

fn cmd_cookies(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for c in closure_org::find_cookies(&src) {
        match c {
            closure_org::CookieView::Count { done, total } => println!("[{done}/{total}]"),
            closure_org::CookieView::Percent(n) => println!("[{n}%]"),
        }
    }
    Ok(())
}

fn cmd_random(vault: &Path, seed: &str) -> Result<(), String> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut h = OFFSET;
    for b in seed.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    let all: Vec<_> = v.iter().flat_map(|(_, d)| d.all_headlines()).collect();
    if all.is_empty() {
        return Err("vault has no headlines".into());
    }
    #[allow(clippy::cast_possible_truncation)]
    let idx = (h as usize) % all.len();
    let pick = all[idx];
    println!("{}\t{}", pick.id(), pick.title());
    Ok(())
}

fn cmd_graph(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    println!("digraph closure {{");
    println!("  rankdir=LR;");
    for (_, doc) in v.iter() {
        for h in doc.all_headlines() {
            let label = h.title().replace('"', "\\\"");
            println!("  \"{}\" [label=\"{label}\"];", h.id());
        }
    }
    for (_, doc) in v.iter() {
        for h in doc.all_headlines() {
            for t in h.link_targets() {
                let target = t.strip_prefix("id:").unwrap_or(t);
                if v.find_by_id(&closure_core::BlockId::from_existing(target))
                    .is_some()
                {
                    println!("  \"{}\" -> \"{target}\";", h.id());
                }
            }
        }
    }
    println!("}}");
    Ok(())
}

fn cmd_hash(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    println!("{:016x}", doc.source_hash());
    Ok(())
}

fn cmd_recent(vault: &Path, limit: usize) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let mut paths_with_mtime: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    for p in v.paths() {
        if let Ok(md) = std::fs::metadata(&p)
            && let Ok(mtime) = md.modified()
        {
            paths_with_mtime.push((p, mtime));
        }
    }
    paths_with_mtime.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
    for (p, _) in paths_with_mtime.into_iter().take(limit) {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_paths(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.paths() {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_mcp() -> Result<(), String> {
    let registry = closure_core::default_registry();
    closure_mcp::run_stdio(&registry).map_err(|e| format!("{e}"))
}

fn cmd_dead_links(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            for raw in h.link_targets() {
                let Some(stripped) = raw.strip_prefix("id:") else {
                    continue;
                };
                if v.find_by_id(&closure_core::BlockId::from_existing(stripped))
                    .is_none()
                {
                    println!("{}\t{}\t{}", path.display(), h.id(), raw);
                }
            }
        }
    }
    Ok(())
}

fn cmd_todo_cloud(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (kw, n) in v.todo_counts() {
        println!("{n:>4}  {kw}");
    }
    Ok(())
}

fn cmd_body(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let h = doc
        .headline_by_id(&bid)
        .ok_or_else(|| "block id not found".to_owned())?;
    print!("{}", h.body_text());
    Ok(())
}

fn cmd_path_of(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let p = doc
        .path_of(&bid)
        .ok_or_else(|| "block id not found".to_owned())?;
    let s: Vec<String> = p.iter().map(usize::to_string).collect();
    println!("{}", s.join("/"));
    Ok(())
}

fn cmd_info(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let h = doc
        .headline_by_id(&bid)
        .ok_or_else(|| "block id not found".to_owned())?;
    println!("id:        {}", h.id());
    println!("title:     {}", h.title());
    println!("level:     {}", h.level());
    if let Some(t) = h.todo() {
        println!("todo:      {t}");
    }
    if let Some(p) = h.priority() {
        println!("priority:  {p}");
    }
    if !h.tags().is_empty() {
        println!("tags:      {}", h.tags().join(", "));
    }
    if let Some(s) = h.scheduled() {
        println!("scheduled: {s}");
    }
    if let Some(d) = h.deadline() {
        println!("deadline:  {d}");
    }
    if let Some(c) = h.closed() {
        println!("closed:    {c}");
    }
    Ok(())
}

fn cmd_find_id(vault: &Path, id: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let bid = closure_core::BlockId::from_existing(id);
    let (h, path) = v.find_by_id(&bid).ok_or_else(|| "not found".to_owned())?;
    println!("{}\t{}\t{}", path.display(), h.id(), h.title());
    Ok(())
}

fn cmd_find_title(vault: &Path, title: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let (h, path) = v.find_by_title(title).ok_or_else(|| "not found".to_owned())?;
    println!("{}\t{}\t{}", path.display(), h.id(), h.title());
    Ok(())
}

fn cmd_comment_list(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            if h.is_comment() {
                println!("{}\t{}\t{}", path.display(), h.id(), h.title());
            }
        }
    }
    Ok(())
}

fn cmd_tagged(vault: &Path, tags: &[String]) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    for m in closure_query::by_tags_all(&v, &refs) {
        println!(
            "{}\t{}\t{}",
            m.path.display(),
            m.headline.id(),
            m.headline.title()
        );
    }
    Ok(())
}

fn cmd_tagged_any(vault: &Path, tags: &[String]) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    for m in closure_query::by_tags_any(&v, &refs) {
        println!(
            "{}\t{}\t{}",
            m.path.display(),
            m.headline.id(),
            m.headline.title()
        );
    }
    Ok(())
}

fn cmd_archived(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            if h.tags().iter().any(|t| t == "ARCHIVE") {
                println!("{}\t{}\t{}", path.display(), h.id(), h.title());
            }
        }
    }
    Ok(())
}

fn cmd_roots(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for r in doc.roots() {
        println!("{}", r.title());
    }
    Ok(())
}

fn walk_leaves(h: &closure_org::Headline) {
    if h.is_leaf() {
        println!("{}", h.title());
    }
    for c in h.children() {
        walk_leaves(c);
    }
}

fn cmd_leaves(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for r in doc.roots() {
        walk_leaves(r);
    }
    Ok(())
}

fn cmd_nodes(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    println!("preamble: {}", doc.preamble_len());
    println!("total:    {}", doc.total_node_count());
    Ok(())
}

fn cmd_depth(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    println!("{}", doc.max_depth());
    Ok(())
}

fn cmd_logbook_append(path: &Path, id: &str, entry: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let mut indices: Vec<usize> = Vec::new();
    let bid = closure_core::BlockId::from_existing(id);
    walk_for_id(&doc, &bid, &mut indices, &mut Vec::new())
        .ok_or_else(|| "block id not found".to_owned())?;
    let new = closure_org::rewrite_headline_append_logbook(&doc, &indices, entry)
        .map_err(|e| format!("{e}"))?;
    fs::write(path, closure_org::print(&new)).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn walk_for_id(
    doc: &closure_org::OrgDoc,
    target: &closure_core::BlockId,
    out: &mut Vec<usize>,
    cursor: &mut Vec<usize>,
) -> Option<()> {
    fn walk(
        h: &closure_org::Headline,
        target: &str,
        cursor: &mut Vec<usize>,
        out: &mut Vec<usize>,
    ) -> bool {
        if h.properties().and_then(closure_org::Properties::id) == Some(target) {
            out.clone_from(cursor);
            return true;
        }
        for (i, c) in h.children().iter().enumerate() {
            cursor.push(i);
            if walk(c, target, cursor, out) {
                return true;
            }
            cursor.pop();
        }
        false
    }
    for (i, root) in doc.roots().iter().enumerate() {
        cursor.clear();
        cursor.push(i);
        if walk(root, target.as_str(), cursor, out) {
            return Some(());
        }
    }
    None
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_languages() -> Result<(), String> {
    for lang in closure_eval::known_languages() {
        println!("{lang}");
    }
    Ok(())
}

fn cmd_tags_of(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let bid = BlockId::from_existing(id);
    let h = doc
        .headline_by_id(&bid)
        .ok_or_else(|| "block id not found".to_owned())?;
    for t in h.tags() {
        println!("{t}");
    }
    Ok(())
}

fn cmd_where_is(name: &str) -> Result<(), String> {
    let registry = closure_core::default_registry();
    let cmd = registry
        .get(name)
        .ok_or_else(|| format!("unknown command: {name}"))?;
    for chord in cmd.keys() {
        println!("{chord}");
    }
    Ok(())
}

fn cmd_cron_tick(
    path: &Path,
    m: u8,
    h: u8,
    d: u8,
    mo: u8,
    dw: u8,
) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for n in doc.preamble() {
        if n.kind() == closure_org::NodeKind::CodeBlock
            && let Some(cb) = n.as_code_block()
            && cb.language == Some("closure-cron")
        {
            let jobs = closure_cron::parse_jobs(cb.content).map_err(|e| format!("{e}"))?;
            for j in closure_cron::jobs_matching(&jobs, m, h, d, mo, dw) {
                println!("{}", j.command);
            }
        }
    }
    Ok(())
}

fn cmd_cron_list(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for n in doc.preamble() {
        if n.kind() == closure_org::NodeKind::CodeBlock
            && let Some(cb) = n.as_code_block()
            && cb.language == Some("closure-cron")
        {
            for j in closure_cron::parse_jobs(cb.content).map_err(|e| format!("{e}"))? {
                println!("{}", j.command);
            }
        }
    }
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_version() -> Result<(), String> {
    println!("{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn cmd_commands() -> Result<(), String> {
    let registry = closure_core::default_registry();
    let mut names: Vec<&str> = registry.names().collect();
    names.sort_unstable();
    for n in names {
        println!("{n}");
    }
    Ok(())
}

fn cmd_clock(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for d in closure_org::find_drawers(&src) {
        if d.name != "LOGBOOK" {
            continue;
        }
        for e in closure_org::parse_logbook(d.content) {
            if e.kind == closure_org::LogbookKind::Clock
                && let Some(w) = e.when
            {
                println!("{w}");
            }
        }
    }
    Ok(())
}

fn cmd_hubs(vault: &Path, limit: usize) -> Result<(), String> {
    use std::collections::HashMap;
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let graph = v.link_graph();
    let mut counts: HashMap<closure_core::BlockId, usize> = HashMap::new();
    for targets in graph.values() {
        for t in targets {
            *counts.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (id, n) in ranked.into_iter().take(limit) {
        let title = v
            .find_by_id(&id)
            .map_or_else(|| "?".to_owned(), |(h, _)| h.title().to_owned());
        println!("{n}\t{id}\t{title}");
    }
    Ok(())
}

fn cmd_orphans(vault: &Path) -> Result<(), String> {
    use std::collections::HashSet;
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let graph = v.link_graph();
    let mut targeted: HashSet<closure_core::BlockId> = HashSet::new();
    for targets in graph.values() {
        for t in targets {
            targeted.insert(t.clone());
        }
    }
    for (_, doc) in v.iter() {
        for h in doc.all_headlines() {
            if !targeted.contains(h.id()) {
                println!("{}\t{}", h.id(), h.title());
            }
        }
    }
    Ok(())
}

fn cmd_vault_info(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    println!("root:       {}", v.root().display());
    println!("files:      {}", v.len());
    println!("headlines:  {}", v.headline_count());
    println!("words:      {}", v.word_count());
    if let Some((tag, n)) = v.tag_counts().into_iter().next() {
        println!("top tag:    {tag} ({n})");
    }
    if let Some((kw, n)) = v.todo_counts().into_iter().next() {
        println!("top todo:   {kw} ({n})");
    }
    Ok(())
}

fn cmd_stats_file(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let org = doc.org();
    let total_links: usize = doc
        .all_headlines()
        .map(|h| h.link_targets().len())
        .sum();
    println!("file:       {}", path.display());
    println!("headlines:  {}", org.headline_count());
    println!("max depth:  {}", org.max_depth());
    println!("words:      {}", doc.word_count());
    println!("chars:      {}", doc.char_count());
    println!("links:      {total_links}");
    println!("hash:       {:016x}", doc.source_hash());
    Ok(())
}

fn cmd_validate(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("parse: {e}"))?;
    let printed = closure_org::print(&doc);
    if printed == src {
        println!("ok: roundtrip byte-exact ({} bytes)", src.len());
        Ok(())
    } else {
        Err(format!(
            "roundtrip mismatch: {} bytes in, {} bytes out",
            src.len(),
            printed.len()
        ))
    }
}

fn cmd_properties(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    for h in doc.all_headlines() {
        if h.properties().is_empty() {
            continue;
        }
        println!("{} ({})", h.title(), h.id());
        for (k, v) in h.properties() {
            println!("  :{k}: {v}");
        }
    }
    Ok(())
}

fn cmd_keywords(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for (k, v) in doc.all_keywords() {
        println!("{k}\t{v}");
    }
    Ok(())
}

fn cmd_block_args(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let mut idx = 0usize;
    for n in doc.preamble() {
        if n.kind() == closure_org::NodeKind::CodeBlock
            && let Some(cb) = n.as_code_block()
        {
            let lang = cb.language.unwrap_or("?");
            let args_str = cb.args.unwrap_or("");
            println!("block #{idx} lang={lang}");
            for (k, v) in closure_org::parse_block_args(args_str) {
                println!("  {k} {v}");
            }
            idx += 1;
        }
    }
    Ok(())
}

fn cmd_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    for h in doc.all_headlines() {
        println!("{}\t{}", h.id(), h.title());
    }
    Ok(())
}

fn cmd_macros(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for m in closure_org::find_macros(&src) {
        if m.args.is_empty() {
            println!("{{{{{{{}}}}}}}", m.name);
        } else {
            println!("{{{{{{{}({})}}}}}}", m.name, m.args.join(","));
        }
    }
    Ok(())
}

fn cmd_links_to(path: &Path, target: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for h in doc.find_link_sources(target) {
        println!("{}", h.title());
    }
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

fn cmd_anchors(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for a in closure_org::find_anchor_targets(&src) {
        let kind = if a.is_radio { "radio" } else { "anchor" };
        println!("{kind}\t{}", a.name);
    }
    Ok(())
}

fn cmd_blocks(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for b in closure_org::find_named_blocks(&src) {
        let lines = b.content.lines().count();
        println!("#+BEGIN_{}  {} lines", b.name, lines);
    }
    Ok(())
}

fn cmd_pending(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for li in doc.org().unfinished_checkboxes() {
            println!("{}\t{}", path.display(), li.content);
        }
    }
    Ok(())
}

fn cmd_lists(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for (i, g) in doc.lists().iter().enumerate() {
        println!("list #{i} ({} items)", g.items.len());
        for item in &g.items {
            let cb = match item.checkbox {
                Some(closure_org::Checkbox::Checked) => "[X] ",
                Some(closure_org::Checkbox::Unchecked) => "[ ] ",
                Some(closure_org::Checkbox::Partial) => "[-] ",
                None => "",
            };
            println!("  {cb}{}", item.content);
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
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let (chars, words, lines, headlines) = doc.wc();
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

fn cmd_subtree(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let mut indices: Vec<usize> = Vec::new();
    let bid = closure_core::BlockId::from_existing(id);
    walk_for_id(&doc, &bid, &mut indices, &mut Vec::new())
        .ok_or_else(|| "block id not found".to_owned())?;
    let mut node = doc.roots().get(indices[0]).ok_or("no such root")?;
    for &i in &indices[1..] {
        node = node.children().get(i).ok_or("no such child")?;
    }
    print!("{}", node.subtree_source());
    Ok(())
}

fn cmd_head(path: &Path, limit: usize) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    for h in doc.all_headlines().take(limit) {
        println!("{}\t{}", h.id(), h.title());
    }
    Ok(())
}

fn cmd_tree(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    for h in doc.all_headlines() {
        let indent = "  ".repeat(usize::from(h.level()).saturating_sub(1));
        let stars = "*".repeat(usize::from(h.level()));
        let mut prefix = String::new();
        if let Some(t) = h.todo() {
            prefix.push_str(t);
            prefix.push(' ');
        }
        if let Some(p) = h.priority() {
            use std::fmt::Write as _;
            let _ = write!(prefix, "[#{p}] ");
        }
        let tags = if h.tags().is_empty() {
            String::new()
        } else {
            format!("  :{}:", h.tags().join(":"))
        };
        let mark = if h.is_comment() {
            " (COMMENT)"
        } else if h.tags().iter().any(|t| t == "ARCHIVE") {
            " (archived)"
        } else {
            ""
        };
        println!("{indent}{stars} {prefix}{}{tags}{mark}", h.title());
    }
    Ok(())
}

fn cmd_tag_cloud(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (tag, n) in v.tag_counts() {
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
    println!("# priority_levels = A, B, C");
    println!("# tag_inheritance = true");
    println!("# agenda_files = inbox.org, projects.org");
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
    println!("words:     {}", v.word_count());
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
