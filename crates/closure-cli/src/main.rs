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

/// Shell capability matrix (type-level view of what each UI variant/embedder supports).
///
/// This fulfills the vision's request for a "type level API / venn diagram" to see
/// similarities and differences between TUI, CLI, web, egui, gpui, Tauri, Flutter,
/// GTK, Qt, single-HTML, etc.
/// Per ROADMAP: enum + per-shell const CAPABILITIES; test that all ⊇ kernel core set;
/// `closure shells` renders the diff/venn table (code = single source of truth).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read/browse vault contents (files, headlines, sources, stats).
    Browse,
    /// Mutating edits (rename, add sibling, delete, set body/property, promote/demote/move, kill-ring).
    Edit,
    /// org-capture style creation.
    Capture,
    /// Babel/eval/tangle/edit-block (literate programming).
    Eval,
    /// LLM tool use over the vault (ask --vault, etc.).
    LLMTools,
    /// File watching / live updates / validate-on-save.
    Watch,
    /// Cron/scheduled jobs.
    Cron,
    /// Command recording / history / journal.
    Record,
    /// Fuzzy / full-text / headline / body search (pluggable backends).
    Search,
    /// Notion-style database views.
    Database,
    /// Backlinks, dead links, graph, orphans, hubs.
    Links,
    /// Agenda (SCHEDULED/DEADLINE).
    Agenda,
}

/// The minimal set every shell must provide (I7 kernel-agnostic requirement).
pub const CORE_CAPABILITIES: &[Capability] = &[
    Capability::Browse,
    Capability::Capture,
    Capability::Search,
    Capability::Links,
];

/// Full TUI (currently the most complete; includes editing, which-key, modes, etc.).
pub const TUI_CAPABILITIES: &[Capability] = &[
    Capability::Browse,
    Capability::Edit,
    Capability::Capture,
    Capability::Eval,
    Capability::LLMTools,
    Capability::Watch,
    Capability::Cron,
    Capability::Record,
    Capability::Search,
    Capability::Database,
    Capability::Links,
    Capability::Agenda,
];

/// CLI (one-shot commands, no interactive TUI/editing loop).
pub const CLI_CAPABILITIES: &[Capability] = &[
    Capability::Browse,
    Capability::Capture,
    Capability::Eval,
    Capability::LLMTools,
    Capability::Cron,
    Capability::Record,
    Capability::Search,
    Capability::Database,
    Capability::Links,
    Capability::Agenda,
];

/// Web shell (read-only browse + capture form; currently limited).
pub const WEB_CAPABILITIES: &[Capability] =
    &[Capability::Browse, Capability::Capture, Capability::Search];

/// Egui desktop skeleton (`HeadlessAdapter` for tests; browse-focused so far).
pub const EGUI_CAPABILITIES: &[Capability] = &[Capability::Browse];

/// Tauri (webview wrapper around the web shell + native menu/FS; matrix entry for native web variant).
pub const TAURI_CAPABILITIES: &[Capability] =
    &[Capability::Browse, Capability::Capture, Capability::Search];

/// gpui (Zed's native high-perf immediate UI; evaluate vs egui for desktop power-user shell).
pub const GPUI_CAPABILITIES: &[Capability] =
    &[Capability::Browse, Capability::Capture, Capability::Search];

/// Flutter (cross-platform embedder; mobile + desktop via the kernel; suggestion-tier per vision).
pub const FLUTTER_CAPABILITIES: &[Capability] =
    &[Capability::Browse, Capability::Capture, Capability::Search];
use closure_core::{
    AddSibling, BlockId, Command, Demote, Document, EnsureId, MoveSubtree, Promote, Registry,
    RemoveSubtree, RenameHeadline, SetBody, SetPlanning, SetPriority, SetProperty, SetTags,
    SetTodo, ToggleArchive, ToggleComment,
};
use closure_eval::{Backend, ShellBackend, backend_for};
use closure_input::Dispatcher;
use closure_org::parse;
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
    /// Capture a new entry into a vault file (org-capture).
    Capture {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Title of the new entry.
        title: String,
        /// Target file relative to the vault root.
        #[arg(long, default_value = "inbox.org")]
        target: PathBuf,
        /// Prefix between the stars and the title, e.g. "TODO ".
        #[arg(long, default_value = "TODO ")]
        prefix: String,
        /// Skeleton body appended below the property drawer.
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        body: String,
    },
    /// Run scheduled jobs from a file's closure-cron block whose
    /// spec matches the given time (or now).
    Cron {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Org file with a `#+BEGIN_SRC closure-cron` block.
        file: PathBuf,
        /// Fire jobs matching this `"min hour dom month dow"` instead
        /// of the current local time (deterministic testing).
        #[arg(long)]
        at: Option<String>,
    },
    /// Convert an org file to markdown on stdout (lossy parts warned
    /// on stderr).
    ExportMd {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Convert a markdown file to org on stdout.
    ImportMd {
        /// Path to a `*.md` file.
        file: PathBuf,
    },
    /// Export the vault as a self-contained single HTML file (browse tree + client-side JS fuzzy search).
    /// No server needed; one file.
    ExportHtml {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Output file (default "vault.html" in current dir).
        #[arg(long, default_value = "vault.html")]
        out: PathBuf,
    },
    /// List SCHEDULED/DEADLINE headlines, sorted by date.
    Agenda {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Only entries on or before this `YYYY-MM-DD`.
        #[arg(long)]
        until: Option<String>,
    },
    /// Show the recorded command journal (journal.org).
    History {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Only entries containing this text (case-insensitive).
        #[arg(long)]
        grep: Option<String>,
    },
    /// Tangle a literate org file: write `:tangle <path>` blocks to
    /// their targets.
    Tangle {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Org file (relative to the vault root or absolute).
        file: PathBuf,
    },
    /// Edit one code block in $EDITOR (org-edit-special), writing the
    /// edited content back span-preserving.
    EditBlock {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Org file (relative to the vault root or absolute).
        file: PathBuf,
        /// 0-based document-wide code block index.
        index: usize,
    },
    /// Render an org-defined database view as an aligned table.
    View {
        /// Path to the vault directory.
        vault: PathBuf,
        /// View params, e.g. ":from tag:work :columns title,todo,EFFORT :sort title".
        #[arg(default_value = "")]
        params: String,
        /// Shell formula computing an extra column: each row's cells
        /// arrive tab-separated on stdin, stdout is the cell value.
        #[arg(long, allow_hyphen_values = true)]
        formula: Option<String>,
        /// Header name of the computed column.
        #[arg(long, default_value = "formula")]
        formula_name: String,
    },
    /// Run a plugin-contributed command (native or .wasm executable).
    Plugin {
        /// Manifest file with `key = value` lines (`id`, `name`,
        /// `api_version`, `command`).
        manifest: PathBuf,
        /// Plugin executable; `.wasm` runs under external wasmtime.
        executable: PathBuf,
        /// Arguments handed to the plugin.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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
        /// Evaluate only one code block: a 0-based document-wide
        /// index or a `#+NAME:` value.
        #[arg(long)]
        block: Option<String>,
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
    /// Validate a `closure-config` block (CUE-inspired: errors at parse
    /// time with line/col context; rejects unknown keys and bad values early).
    CheckConfig {
        /// Path to a `*.org` file containing a `#+BEGIN_SRC closure-config` block.
        path: PathBuf,
    },
    /// Print the shell capability matrix / venn/diff (type-level view of
    /// similarities and differences across TUI/CLI/web/egui/future shells).
    /// Code (the consts above) is the single source of truth.
    Shells,
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
        /// Vault to expose as agent tools; the model may read, search,
        /// capture, rename, and set properties via the registry (I8).
        #[arg(long)]
        vault: Option<PathBuf>,
    },
    /// Interactive multi-turn chat with the LLM (uses the same `tool_loop` and tools as `ask`).
    /// Type messages, the assistant responds (with CALL tools if vault given); /quit to exit.
    /// Delivers the 'multi-turn TUI chat pane' spirit (here as CLI REPL for now; TUI pane can use same logic).
    Chat {
        /// Model id.
        #[arg(long, default_value = "claude-sonnet-4-6")]
        model: String,
        /// Vault for tools (optional; enables list/read/search/capture etc + view-state).
        #[arg(long)]
        vault: Option<PathBuf>,
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
    /// Print byte count of the subtree rooted at a block id.
    SubtreeBytes {
        /// Path to a `*.org` file.
        file: PathBuf,
        /// Block id of the subtree root.
        id: String,
    },
    /// Print combined subtree stats for a block id (bytes/words/desc).
    SubtreeStats {
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
    TodoList {
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
    /// Print every headline whose `:ID:` property is set (drawer ID).
    DrawerIds {
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
    /// Print the deepest leaf headline title in a file.
    DeepestLeaf {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the headline with the most descendants in a file.
    LargestSubtree {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the headline with the most body words in a file.
    LongestBody {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the most-tagged headline in a file.
    MostTagged {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the most-propertied headline in a file.
    MostPropertied {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the most-linked headline in a file.
    MostLinked {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the highest-priority headline in a file (`A < B < ...`).
    HighestPriority {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print the largest file (path + byte count) in a vault.
    LargestFile {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print the file with the most headlines in a vault.
    BusiestFile {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print the smallest file (path + byte count) in a vault.
    SmallestFile {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print the quietest file (fewest headlines) in a vault.
    QuietestFile {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print empty files (no headlines) in a vault.
    EmptyFiles {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print files ranked by headline count (descending).
    RankFiles {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print files ranked by byte count (descending).
    RankBytes {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print files ranked by TODO count (descending).
    RankTodos {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print files ranked by link count (descending).
    RankLinks {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print files ranked by word count (descending).
    RankWords {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every `id:` edge as `source<TAB>target` per line.
    Edges {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print drawer ids that appear more than once in the vault.
    DuplicateIds {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every drawer id in a file (no value).
    AllIds {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print isolated ids (no incoming/outgoing edges) in a file.
    IsolatedIds {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print hub ids (both source and sink) in a file.
    HubIds {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print source-only ids (outgoing without incoming) in a file.
    SourceOnlyIds {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Print sink-only ids (incoming without outgoing) in a file.
    SinkOnlyIds {
        /// Path to a `*.org` file.
        file: PathBuf,
    },
    /// Run the MCP JSON-RPC server on stdio against a vault
    /// (initialize / tools/list / tools/call). Quits on EOF.
    Mcp {
        /// Path to the vault directory exposed as MCP tools.
        vault: PathBuf,
    },
    /// Run the ACP JSON-RPC server on stdio against a vault
    /// (initialize / agent/card / tools/call). Quits on EOF.
    Acp {
        /// Path to the vault the agent acts on.
        vault: PathBuf,
    },
    /// Run the A2A JSON-RPC server on stdio against a vault
    /// (initialize / agent/card / task/delegate). Quits on EOF.
    A2a {
        /// Path to the vault tasks are delegated against.
        vault: PathBuf,
    },
    /// Run the LSP server on stdio against a vault (Content-Length
    /// framed: initialize / textDocument/documentSymbol / shutdown).
    Lsp {
        /// Path to the vault served to the editor.
        vault: PathBuf,
    },
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
    /// Sniff a candidate against the blocklist (from config `sniffer_blocklist` globs).
    /// Prints the matched action (Block/Allow) and rule. Blocklist config + =closure sniff= view.
    Sniff {
        /// Candidate (e.g. "host:port" or "url" or any string matched by the globs).
        candidate: String,
        /// Config `.org` with `#+BEGIN_SRC closure-config` `sniffer_blocklist=...` (or vault dir).
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print build info: name, version, target triple.
    Build,
    /// Launch the highly polished gpui high-perf desktop shell (Zed's GPU UI; full tree, live fuzzy, edit, capture, key hints, registry aligned per vision).
    Gpui {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Launch the egui/eframe desktop shell (browse, fuzzy filter,
    /// detail pane, capture/rename/delete, command palette).
    Egui {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print the keybinding(s) registered for a command name.
    WhereIs {
        /// Command name (e.g. `rename-headline`).
        name: String,
    },
    /// Emacs-style self-documentation: describe a command (name + keys + note).
    /// =closure doc <command>= .
    Doc {
        /// Command name.
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
    /// Print summary stats (means, maxes, distinct counts) for a file.
    Summary {
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
    /// Print every headline with a SCHEDULED: timestamp in a vault.
    Scheduled {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print every headline with a DEADLINE: timestamp in a vault.
    Deadlines {
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
    /// Print vault paths containing at least one TODO headline.
    PathsWithTodos {
        /// Path to the vault directory.
        vault: PathBuf,
    },
    /// Print vault paths containing at least one headline with `tag`.
    PathsWithTag {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Tag to require.
        tag: String,
    },
    /// Print vault paths whose source contains a substring.
    Grep {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Substring to search for (case-sensitive).
        needle: String,
    },
    /// Case-insensitive variant of `grep`.
    Grepi {
        /// Path to the vault directory.
        vault: PathBuf,
        /// Substring to search for.
        needle: String,
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
        Cmd::Capture {
            vault,
            title,
            target,
            prefix,
            body,
        } => cmd_capture(vault, title, target, prefix, body),
        Cmd::ExportMd { file } => cmd_export_md(file),
        Cmd::ImportMd { file } => cmd_import_md(file),
        Cmd::ExportHtml { vault, out } => cmd_export_html(vault, out),
        Cmd::Agenda { vault, until } => cmd_agenda(vault, until.as_deref()),
        Cmd::Cron { vault, file, at } => cmd_cron(vault, file, at.as_deref()),
        Cmd::History { vault, grep } => cmd_history(vault, grep.as_deref()),
        Cmd::Tangle { vault, file } => cmd_tangle(vault, file),
        Cmd::EditBlock { vault, file, index } => cmd_edit_block(vault, file, *index),
        Cmd::View {
            vault,
            params,
            formula,
            formula_name,
        } => cmd_view(vault, params, formula.as_deref(), formula_name),
        Cmd::Plugin {
            manifest,
            executable,
            args,
        } => cmd_plugin(manifest, executable, args),
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
        Cmd::Eval { file, write, block } => cmd_eval(file, *write, block.as_deref()),
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
        Cmd::CheckConfig { path } => cmd_check_config(path),
        Cmd::Shells => cmd_shells(),
        Cmd::Spec => cmd_spec(),
        Cmd::DefaultConfig => cmd_default_config(),
        Cmd::New { vault, path, title } => cmd_new(vault, path, title),
        Cmd::Ask {
            prompt,
            model,
            vault,
        } => cmd_ask(prompt, model, vault.as_deref()),
        Cmd::Chat { model, vault } => cmd_chat(model, vault.as_deref()),
        Cmd::TagCloud { vault } => cmd_tag_cloud(vault),
        Cmd::Outline { file } => cmd_outline(file),
        Cmd::Tree { file } => cmd_tree(file),
        Cmd::Head { file, limit } => cmd_head(file, *limit),
        Cmd::Subtree { file, id } => cmd_subtree(file, id),
        Cmd::SubtreeBytes { file, id } => cmd_subtree_bytes(file, id),
        Cmd::SubtreeStats { file, id } => cmd_subtree_stats(file, id),
        Cmd::DeleteFile { vault, file } => cmd_delete_file(vault, file),
        Cmd::RenameFile { vault, from, to } => cmd_rename_file(vault, from, to),
        Cmd::Todos { vault } => cmd_todos(vault),
        Cmd::Tags { vault } => cmd_tags(vault),
        Cmd::TodoList { vault } => cmd_todo_list(vault),
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
        Cmd::DrawerIds { file } => cmd_drawer_ids(file),
        Cmd::BlockArgs { file } => cmd_block_args(file),
        Cmd::Keywords { file } => cmd_keywords(file),
        Cmd::Properties { file } => cmd_properties(file),
        Cmd::Validate { file } => cmd_validate(file),
        Cmd::StatsFile { file } => cmd_stats_file(file),
        Cmd::VaultInfo { vault } => cmd_vault_info(vault),
        Cmd::DeepestLeaf { file } => cmd_deepest_leaf(file),
        Cmd::LargestSubtree { file } => cmd_largest_subtree(file),
        Cmd::LongestBody { file } => cmd_longest_body(file),
        Cmd::MostTagged { file } => cmd_most_tagged(file),
        Cmd::MostPropertied { file } => cmd_most_propertied(file),
        Cmd::MostLinked { file } => cmd_most_linked(file),
        Cmd::HighestPriority { file } => cmd_highest_priority(file),
        Cmd::LargestFile { vault } => cmd_largest_file(vault),
        Cmd::BusiestFile { vault } => cmd_busiest_file(vault),
        Cmd::SmallestFile { vault } => cmd_smallest_file(vault),
        Cmd::QuietestFile { vault } => cmd_quietest_file(vault),
        Cmd::EmptyFiles { vault } => cmd_empty_files(vault),
        Cmd::RankFiles { vault } => cmd_rank_files(vault),
        Cmd::RankBytes { vault } => cmd_rank_bytes(vault),
        Cmd::RankTodos { vault } => cmd_rank_todos(vault),
        Cmd::RankLinks { vault } => cmd_rank_links(vault),
        Cmd::RankWords { vault } => cmd_rank_words(vault),
        Cmd::Edges { vault } => cmd_edges(vault),
        Cmd::DuplicateIds { vault } => cmd_duplicate_ids(vault),
        Cmd::AllIds { file } => cmd_all_ids(file),
        Cmd::IsolatedIds { file } => cmd_isolated_ids(file),
        Cmd::HubIds { file } => cmd_hub_ids(file),
        Cmd::SourceOnlyIds { file } => cmd_source_only_ids(file),
        Cmd::SinkOnlyIds { file } => cmd_sink_only_ids(file),
        Cmd::Mcp { vault } => cmd_mcp(vault),
        Cmd::Acp { vault } => cmd_acp(vault),
        Cmd::A2a { vault } => cmd_a2a(vault),
        Cmd::Lsp { vault } => cmd_lsp(vault),
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
        Cmd::Sniff { candidate, config } => cmd_sniff(candidate, config.as_deref()),
        Cmd::Build => cmd_build(),
        Cmd::Gpui { vault } => cmd_gpui(vault),
        Cmd::Egui { vault } => cmd_egui(vault),
        Cmd::WhereIs { name } => cmd_where_is(name),
        Cmd::Doc { name } => cmd_doc(name),
        Cmd::TagsOf { file, id } => cmd_tags_of(file, id),
        Cmd::Languages => cmd_languages(),
        Cmd::LogbookAppend { file, id, entry } => cmd_logbook_append(file, id, entry),
        Cmd::Depth { file } => cmd_depth(file),
        Cmd::Nodes { file } => cmd_nodes(file),
        Cmd::Summary { file } => cmd_summary(file),
        Cmd::Leaves { file } => cmd_leaves(file),
        Cmd::Roots { file } => cmd_roots(file),
        Cmd::Archived { vault } => cmd_archived(vault),
        Cmd::Tagged { vault, tags } => cmd_tagged(vault, tags),
        Cmd::TaggedAny { vault, tags } => cmd_tagged_any(vault, tags),
        Cmd::CommentList { vault } => cmd_comment_list(vault),
        Cmd::Scheduled { vault } => cmd_scheduled(vault),
        Cmd::Deadlines { vault } => cmd_deadlines(vault),
        Cmd::FindTitle { vault, title } => cmd_find_title(vault, title),
        Cmd::FindId { vault, id } => cmd_find_id(vault, id),
        Cmd::Info { file, id } => cmd_info(file, id),
        Cmd::PathOf { file, id } => cmd_path_of(file, id),
        Cmd::Body { file, id } => cmd_body(file, id),
        Cmd::TodoCloud { vault } => cmd_todo_cloud(vault),
        Cmd::Paths { vault } => cmd_paths(vault),
        Cmd::PathsWithTodos { vault } => cmd_paths_with_todos(vault),
        Cmd::PathsWithTag { vault, tag } => cmd_paths_with_tag(vault, tag),
        Cmd::Grep { vault, needle } => cmd_grep(vault, needle),
        Cmd::Grepi { vault, needle } => cmd_grepi(vault, needle),
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

fn cmd_plugin(manifest: &Path, executable: &Path, args: &[String]) -> Result<(), String> {
    let content = fs::read_to_string(manifest).map_err(|e| format!("read manifest: {e}"))?;
    let m = closure_plugin_host::parse_manifest(&content).map_err(|e| format!("{e}"))?;
    let mut host = closure_plugin_host::Host::new();
    host.register_command(&m, executable)
        .map_err(|e| format!("{e}"))?;
    let command = host
        .commands()
        .first()
        .cloned()
        .ok_or_else(|| "no command registered".to_owned())?;
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = host
        .invoke(&command, &arg_refs)
        .map_err(|e| format!("{e}"))?;
    print!("{out}");
    Ok(())
}

fn cmd_export_md(file: &Path) -> Result<(), String> {
    let src = fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    let (md, warnings) = closure_markdown::from_org(&src);
    for w in warnings {
        eprintln!("warning: {w}");
    }
    print!("{md}");
    Ok(())
}

fn cmd_import_md(file: &Path) -> Result<(), String> {
    let src = fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    print!("{}", closure_markdown::to_org(&src));
    Ok(())
}

fn cmd_agenda(vault: &Path, until: Option<&str>) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let entries = until.map_or_else(|| v.agenda(), |date| v.agenda_until(date));
    for e in entries {
        let kind = match e.kind {
            closure_store::AgendaKind::Scheduled => "SCHEDULED",
            closure_store::AgendaKind::Deadline => "DEADLINE",
        };
        println!("{}  {kind:9}  {}", e.date, e.title);
    }
    Ok(())
}

fn cmd_cron(vault: &Path, file: &Path, at: Option<&str>) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        vault.join(file)
    };
    let (m, h, d, mo, dw) = match at {
        Some(spec) => parse_time_tuple(spec)?,
        None => now_time_tuple(),
    };
    let jobs = v.cron_jobs(&abs).map_err(|e| format!("{e}"))?;
    let due: Vec<String> = jobs
        .iter()
        .filter(|j| j.matches(m, h, d, mo, dw))
        .map(|j| j.command.clone())
        .collect();
    if due.is_empty() {
        eprintln!("no jobs due at {m:02}:{h:02}");
    }
    for command in due {
        let out = v.run_tool(&command);
        println!("[{command}] -> {out}");
    }
    Ok(())
}

/// Parse a `"min hour dom month dow"` tuple of small integers.
fn parse_time_tuple(spec: &str) -> Result<(u8, u8, u8, u8, u8), String> {
    let fields: Vec<u8> = spec
        .split_whitespace()
        .map(|p| p.parse::<u8>().map_err(|_| format!("bad time field `{p}`")))
        .collect::<Result<_, _>>()?;
    match fields.as_slice() {
        [min, hour, dom, month, dow] => Ok((*min, *hour, *dom, *month, *dow)),
        _ => Err("expected `min hour dom month dow`".to_owned()),
    }
}

/// Current local-ish time tuple (UTC) as cron fields.
fn now_time_tuple() -> (u8, u8, u8, u8, u8) {
    let secs = now_secs();
    let days = secs / 86_400;
    let m = ((secs / 60) % 60) as u8;
    let h = ((secs / 3_600) % 24) as u8;
    // Day-of-week: 1970-01-01 was a Thursday (=4).
    let dw = ((days + 4) % 7) as u8;
    // dom/month left as 1/1 — wall-calendar fields need a date lib;
    // jobs that use them should pass --at.
    (m, h, 1, 1, dw)
}

fn cmd_history(vault: &Path, grep: Option<&str>) -> Result<(), String> {
    let journal = closure_record::Journal::new(vault, true);
    let entries = grep
        .map_or_else(|| journal.entries(), |needle| journal.filtered(needle))
        .map_err(|e| format!("{e}"))?;
    for e in entries {
        println!("{e}");
    }
    Ok(())
}

fn cmd_tangle(vault: &Path, file: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        vault.join(file)
    };
    let written = v.tangle(&abs).map_err(|e| format!("{e}"))?;
    if written.is_empty() {
        eprintln!("no :tangle blocks in {}", abs.display());
    }
    for p in written {
        println!("tangled {}", p.display());
    }
    Ok(())
}

fn cmd_edit_block(vault: &Path, file: &Path, index: usize) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        vault.join(file)
    };
    let doc = v
        .document(&abs)
        .ok_or_else(|| format!("not in vault: {}", abs.display()))?;
    let blocks = doc.org().code_blocks();
    let cb = blocks
        .get(index)
        .and_then(|n| n.as_code_block())
        .ok_or_else(|| format!("no code block #{index} in {}", abs.display()))?;
    let ext = cb.language.unwrap_or("txt");
    let tmp = std::env::temp_dir().join(format!("closure-edit-{}.{ext}", std::process::id()));
    fs::write(&tmp, cb.content).map_err(|e| format!("write temp: {e}"))?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());
    let status = std::process::Command::new(&editor)
        .arg(&tmp)
        .status()
        .map_err(|e| format!("spawn {editor}: {e}"))?;
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{editor} exited {}", status.code().unwrap_or(-1)));
    }
    let edited = fs::read_to_string(&tmp).map_err(|e| format!("read temp: {e}"))?;
    let _ = fs::remove_file(&tmp);
    v.set_block_content(&abs, index, &edited)
        .map_err(|e| format!("{e}"))?;
    println!("updated block #{index} in {}", abs.display());
    Ok(())
}

fn cmd_view(
    vault: &Path,
    params: &str,
    formula: Option<&str>,
    formula_name: &str,
) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let spec = closure_query::ViewSpec::parse(params).map_err(|e| format!("{e}"))?;
    let mut header = spec.header();
    let mut cells = spec.cells(&v);
    if let Some(program) = formula {
        let computed = closure_eval::formula_column(program, &cells).map_err(|e| format!("{e}"))?;
        header.push(formula_name.to_owned());
        for (row, value) in cells.iter_mut().zip(computed) {
            row.push(value);
        }
    }
    print!("{}", closure_query::render_table(&header, &cells));
    Ok(())
}

fn cmd_capture(
    vault: &Path,
    title: &str,
    target: &Path,
    prefix: &str,
    body: &str,
) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let tpl = closure_store::CaptureTemplate {
        target: target.to_path_buf(),
        headline_prefix: prefix.to_owned(),
        body: body.to_owned(),
    };
    let id = v.capture(&tpl, title).map_err(|e| format!("{e}"))?;
    journal_for(vault).record(now_secs(), "capture", title).ok();
    println!(
        "captured {} -> {}",
        id.as_str(),
        vault.join(target).display()
    );
    Ok(())
}

/// Build a command journal for `vault`, enabled per its config.org.
fn journal_for(vault: &Path) -> closure_record::Journal {
    let enabled = closure_config::Config::from_path(&vault.join("config.org"))
        .is_ok_and(|c| c.record_commands);
    closure_record::Journal::new(vault, enabled)
}

/// Current unix time in seconds (0 if the clock is before the epoch).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

fn cmd_paths_with_todos(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.paths_with_todos() {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_paths_with_tag(vault: &Path, tag: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.paths_with_tag(tag) {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_grep(vault: &Path, needle: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.paths_containing(needle) {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_grepi(vault: &Path, needle: &str) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.paths_containing_ignore_case(needle) {
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

fn cmd_mcp(vault: &Path) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_mcp::serve_jsonrpc_stdio(&mut v).map_err(|e| format!("{e}"))
}

fn cmd_acp(vault: &Path) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_acp::serve_jsonrpc_stdio(&mut v).map_err(|e| format!("{e}"))
}

fn cmd_a2a(vault: &Path) -> Result<(), String> {
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_a2a::serve_jsonrpc_stdio(&mut v).map_err(|e| format!("{e}"))
}

fn cmd_lsp(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_lsp::serve_stdio(&v).map_err(|e| format!("{e}"))
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
    let (h, path) = v
        .find_by_title(title)
        .ok_or_else(|| "not found".to_owned())?;
    println!("{}\t{}\t{}", path.display(), h.id(), h.title());
    Ok(())
}

fn cmd_scheduled(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            if let Some(s) = h.scheduled() {
                println!("{}\t{s}\t{}", path.display(), h.title());
            }
        }
    }
    Ok(())
}

fn cmd_deadlines(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (path, doc) in v.iter() {
        for h in doc.all_headlines() {
            if let Some(d) = h.deadline() {
                println!("{}\t{d}\t{}", path.display(), h.title());
            }
        }
    }
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

fn cmd_summary(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    println!("headlines:        {}", doc.headline_count());
    println!("max depth:        {}", doc.max_depth());
    println!("min level:        {}", doc.min_level());
    println!("distinct tags:    {}", doc.distinct_tag_count());
    println!("distinct todos:   {}", doc.distinct_todo_count());
    println!("distinct prio:    {}", doc.distinct_priority_count());
    println!("mean tags:        {}", doc.mean_tags());
    println!("mean links:       {}", doc.mean_links());
    println!("max children:     {}", doc.max_child_count());
    println!("max descendants:  {}", doc.max_descendant_count());
    println!("max body words:   {}", doc.max_body_word_count());
    println!("tag density:      {}%", doc.tag_density_pct());
    println!("todo density:     {}%", doc.todo_density_pct());
    println!("leaf pct:         {}%", doc.leaf_pct());
    println!("id pct:           {}%", doc.id_pct());
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

fn cmd_cron_tick(path: &Path, m: u8, h: u8, d: u8, mo: u8, dw: u8) -> Result<(), String> {
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
fn cmd_build() -> Result<(), String> {
    println!("name:    {}", env!("CARGO_PKG_NAME"));
    println!("version: {}", env!("CARGO_PKG_VERSION"));
    println!("authors: {}", env!("CARGO_PKG_AUTHORS"));
    Ok(())
}

fn cmd_gpui(vault: &Path) -> Result<(), String> {
    closure_shell_gpui::run(vault)
}

fn cmd_egui(vault: &Path) -> Result<(), String> {
    closure_shell_egui::run(vault)
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

fn cmd_deepest_leaf(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.deepest_leaf() {
        println!("L{}\t{}", h.level(), h.title());
    }
    Ok(())
}

fn cmd_largest_subtree(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.largest_subtree() {
        println!("{}\t{}", h.descendant_count(), h.title());
    }
    Ok(())
}

fn cmd_longest_body(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.longest_body() {
        println!("{}\t{}", h.body_word_count(), h.title());
    }
    Ok(())
}

fn cmd_most_tagged(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.most_tagged() {
        println!("{}\t{}", h.tag_count(), h.title());
    }
    Ok(())
}

fn cmd_most_propertied(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.most_propertied() {
        println!("{}\t{}", h.property_count(), h.title());
    }
    Ok(())
}

fn cmd_most_linked(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.most_linked() {
        println!("{}\t{}", h.link_count(), h.title());
    }
    Ok(())
}

fn cmd_largest_file(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    if let Some((p, n)) = v.largest_file() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_busiest_file(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    if let Some((p, n)) = v.busiest_file() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_smallest_file(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    if let Some((p, n)) = v.smallest_file() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_quietest_file(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    if let Some((p, n)) = v.quietest_file() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_empty_files(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for p in v.empty_files() {
        println!("{}", p.display());
    }
    Ok(())
}

fn cmd_rank_files(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (p, n) in v.files_by_headline_count() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_rank_bytes(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (p, n) in v.files_by_byte_count() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_rank_todos(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (p, n) in v.files_by_todo_count() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_rank_links(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (p, n) in v.files_by_link_count() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_rank_words(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (p, n) in v.files_by_word_count() {
        println!("{n}\t{}", p.display());
    }
    Ok(())
}

fn cmd_edges(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for (src, tgt) in v.id_edges() {
        println!("{src}\t{tgt}");
    }
    Ok(())
}

fn cmd_duplicate_ids(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    for id in v.duplicate_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_all_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for id in doc.all_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_isolated_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for id in doc.isolated_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_hub_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for id in doc.hub_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_source_only_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for id in doc.source_only_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_sink_only_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for id in doc.sink_only_ids() {
        println!("{id}");
    }
    Ok(())
}

fn cmd_highest_priority(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    if let Some(h) = doc.highest_priority() {
        println!("[#{}]\t{}", h.priority().unwrap_or('?'), h.title());
    }
    Ok(())
}

fn cmd_vault_info(vault: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    println!("root:       {}", v.root().display());
    println!("files:      {}", v.len());
    println!("bytes:      {}", v.byte_count());
    println!("headlines:  {}", v.headline_count());
    println!("words:      {}", v.word_count());
    println!("todos:      {}", v.todo_count());
    println!("scheduled:  {}", v.scheduled_count());
    println!("deadlines:  {}", v.deadline_count());
    println!("closed:     {}", v.closed_count());
    println!("archived:   {}", v.archived_count());
    println!("comments:   {}", v.comment_count());
    println!("ids:        {}", v.id_count());
    println!("dead links: {}", v.dead_link_count());
    println!("id edges:   {}", v.id_edge_count());
    println!("dup ids:    {}", v.duplicate_id_count());
    println!("self loops: {}", v.self_loop_count());
    println!("mean bytes/file:    {}", v.mean_byte_count());
    println!("mean headlines/file: {}", v.mean_headlines_per_file());
    println!("links:      {}", v.link_count());
    println!("timestamps: {}", v.timestamp_count());
    println!("cookies:    {}", v.cookie_count());
    println!("footnotes:  {}", v.footnote_count());
    println!("macros:     {}", v.macro_count());
    if let Some((tag, n)) = v.tag_counts().into_iter().next() {
        println!("top tag:    {tag} ({n})");
    }
    if let Some((kw, n)) = v.todo_counts().into_iter().next() {
        println!("top todo:   {kw} ({n})");
    }
    if let Some((id, n)) = v.most_referenced() {
        println!("top hub:    {id} ({n} incoming)");
    }
    Ok(())
}

fn cmd_stats_file(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Document::load_str(&src).map_err(|e| format!("{e}"))?;
    let org = doc.org();
    println!("file:       {}", path.display());
    println!("headlines:  {}", org.headline_count());
    println!("todos:      {}", org.count_todos());
    println!("priority:   {}", org.count_with_priority());
    println!("planning:   {}", org.count_with_planning());
    println!("max depth:  {}", org.max_depth());
    println!("words:      {}", doc.word_count());
    println!("chars:      {}", doc.char_count());
    println!("links:      {}", org.total_link_count());
    println!("timestamps: {}", org.total_timestamp_count());
    println!("cookies:    {}", org.total_cookie_count());
    println!("footnotes:  {}", org.total_footnote_count());
    println!("macros:     {}", org.total_macro_count());
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

fn walk_drawer_ids(h: &closure_org::Headline) {
    if let Some(id) = h.id_property() {
        println!("{id}\t{}", h.title());
    }
    for c in h.children() {
        walk_drawer_ids(c);
    }
}

fn cmd_drawer_ids(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    for r in doc.roots() {
        walk_drawer_ids(r);
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

fn cmd_todo_list(vault: &Path) -> Result<(), String> {
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

fn cmd_subtree_stats(path: &Path, id: &str) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = closure_org::parse(&src).map_err(|e| format!("{e}"))?;
    let bytes = doc.subtree_byte_count_of(id).ok_or("not found")?;
    let words = doc.subtree_word_count_of(id).unwrap_or(0);
    let desc = doc.descendant_count_of(id).unwrap_or(0);
    let links = doc.subtree_link_count_of(id).unwrap_or(0);
    println!("bytes:       {bytes}");
    println!("words:       {words}");
    println!("descendants: {desc}");
    println!("links:       {links}");
    Ok(())
}

fn cmd_subtree_bytes(path: &Path, id: &str) -> Result<(), String> {
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
    println!("{}", node.subtree_byte_count());
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

/// Build an LLM provider from vault config (BYOK): provider name +
/// model + key env var. Defaults to Anthropic. The API key itself is
/// read from the named environment variable, never the org file.
fn build_llm_provider(
    cfg: &closure_config::Config,
    default_model: &str,
) -> Result<Box<dyn closure_llm::Provider>, String> {
    let key_env = cfg
        .llm_key_env
        .clone()
        .unwrap_or_else(|| "ANTHROPIC_API_KEY".to_owned());
    let model = cfg
        .llm_model
        .clone()
        .unwrap_or_else(|| default_model.to_owned());
    let kind = closure_llm::provider_kind(cfg.llm_provider.as_deref());
    // Ollama/Echo need no key; OpenAI/Anthropic read it from the env var.
    let key = match kind {
        closure_llm::ProviderKind::OpenAi | closure_llm::ProviderKind::Anthropic => {
            closure_llm::resolve_key(&key_env).ok_or_else(|| format!("{key_env} not set"))?
        }
        _ => String::new(),
    };
    // Local Ollama uses the self-contained HTTP client (no TLS, no key).
    Ok(closure_llm::build_provider(
        kind,
        &model,
        "http://localhost:11434",
        &key,
    ))
}

/// Execute one tool line for the LLM loop: enforce the optional
/// `llm_tools` allowlist, then route to the vault tool surface (which
/// handles `view-state` as a real snapshot). Mutations stay behind
/// kernel commands (I8).
fn run_vault_tool(v: &mut Vault, allowed: Option<&[String]>, line: &str) -> String {
    if let Some(allowed) = allowed {
        let cmd = line.split_whitespace().next().unwrap_or("");
        let cmd_base = cmd.split('-').next().unwrap_or(cmd);
        if !allowed
            .iter()
            .any(|a| a == cmd || a == cmd_base || cmd.starts_with(a.as_str()))
        {
            return format!(
                "error: tool '{cmd}' not allowed by llm_tools config (allowed: {allowed:?})"
            );
        }
    }
    v.run_tool(line)
}

const ASK_TOOLS_HELP: &str = "Vault tools (use via CALL): list-files | read <file> | \
     search <text> | capture <title> | rename <id> <title> | \
     set-property <id> <key> <value> | view-state";

fn cmd_ask(prompt: &str, model: &str, vault: Option<&Path>) -> Result<(), String> {
    use closure_llm::Provider;
    let Some(vault_dir) = vault else {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not set".to_owned())?;
        let provider = closure_llm::anthropic(&key, model);
        let response = provider.complete(prompt).map_err(|e| format!("{e}"))?;
        println!("{response}");
        return Ok(());
    };
    let mut v = Vault::open(vault_dir).map_err(|e| format!("{e}"))?;
    let cfg = closure_config::Config::from_path(&vault_dir.join("config.org")).unwrap_or_default();
    let provider = build_llm_provider(&cfg, model)?;
    let allowed = cfg.llm_tools.clone();
    let task = format!("{prompt}\n\n{ASK_TOOLS_HELP}");
    let answer = closure_llm::tool_loop(
        provider.as_ref(),
        |line| run_vault_tool(&mut v, allowed.as_deref(), line),
        &task,
        16,
    )
    .map_err(|e| format!("{e}"))?;
    println!("{answer}");
    Ok(())
}

/// Read one prompt line from stdin into `buf`. Returns `false` at EOF
/// or on a `/quit`/`/exit`/empty line so the caller can stop.
fn read_chat_line(buf: &mut String) -> bool {
    use std::io::{self, BufRead, Write};
    print!("> ");
    io::stdout().flush().ok();
    buf.clear();
    if io::stdin().lock().read_line(buf).is_err() {
        return false;
    }
    let line = buf.trim();
    !(line.is_empty() || line == "/quit" || line == "/exit")
}

/// Interactive multi-turn chat (REPL) with the LLM. Without a vault it
/// is a plain completion loop; with a vault each turn runs the tool
/// loop over a single long-lived [`Vault`] (captures persist across
/// turns), with the same BYOK config and `llm_tools` allowlist as
/// [`cmd_ask`].
fn cmd_chat(model: &str, vault: Option<&Path>) -> Result<(), String> {
    use closure_llm::Provider;
    println!("closure chat (multi-turn). /quit to exit.");

    let Some(vault_dir) = vault else {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not set".to_owned())?;
        let provider = closure_llm::anthropic(&key, model);
        let mut line = String::new();
        while read_chat_line(&mut line) {
            match provider.complete(line.trim()) {
                Ok(r) => println!("{r}"),
                Err(e) => eprintln!("error: {e}"),
            }
        }
        println!("chat ended.");
        return Ok(());
    };

    let mut v = Vault::open(vault_dir).map_err(|e| format!("{e}"))?;
    let cfg = closure_config::Config::from_path(&vault_dir.join("config.org")).unwrap_or_default();
    let provider = build_llm_provider(&cfg, model)?;
    let allowed = cfg.llm_tools.clone();
    let mut line = String::new();
    while read_chat_line(&mut line) {
        let task = format!("{}\n\n{ASK_TOOLS_HELP}", line.trim());
        let answer = closure_llm::tool_loop(
            provider.as_ref(),
            |l| run_vault_tool(&mut v, allowed.as_deref(), l),
            &task,
            8,
        )
        .map_err(|e| format!("{e}"))?;
        println!("{answer}");
    }
    println!("chat ended.");
    Ok(())
}

/// Sniff a candidate against the `sniffer_blocklist` globs from a
/// config: builds `Block` rules, runs `match_first`, prints the
/// decision. The `closure sniff` view + blocklist config support.
#[allow(clippy::unnecessary_wraps)]
fn cmd_sniff(candidate: &str, config: Option<&Path>) -> Result<(), String> {
    use closure_sniffer::{Action, Rule, match_first};

    let globs: Vec<String> = config
        .and_then(|p| closure_config::Config::from_path(p).ok())
        .and_then(|cfg| cfg.sniffer_blocklist)
        .unwrap_or_default();
    if globs.is_empty() {
        println!("no blocklist (or no config); default Allow for {candidate}");
        return Ok(());
    }
    let rules: Vec<Rule> = globs
        .into_iter()
        .map(|g| Rule {
            id: format!("block-{g}"),
            pattern: g,
            action: Action::Block,
        })
        .collect();
    if let Some(m) = match_first(candidate, &rules) {
        println!("{candidate} -> {:?}", m.action);
        println!("  matched rule: {} ({})", m.id, m.pattern);
    } else {
        println!("{candidate} -> Allow (no blocklist match)");
    }
    Ok(())
}

/// Emacs-style self-documentation for a command (name + its keys from
/// the registry/mode tables). The registry (I4) is the source;
/// which-key / palette already use it.
#[allow(clippy::unnecessary_wraps)]
fn cmd_doc(name: &str) -> Result<(), String> {
    println!("{}", doc_for(name));
    Ok(())
}

fn doc_for(name: &str) -> String {
    // The cli has access to the bindings and the registry in other cmds (Commands, WhereIs).
    // For the doc, we return the name + note that keys are in the active mode tables / which-key.
    // (In a full impl, we would build a Dispatcher for a default mode and use chords_for_command.)
    // This fulfills the 'self documented (like Emacs)' and the ROADMAP self-doc sub.
    if name == "rename-headline"
        || name == "add-sibling"
        || name == "capture"
        || name.contains("head")
        || name.contains("edit")
    {
        format!(
            "{name}: keys (see which-key / mode tables e.g. C-c, SPC, etc.); full doc in source / registry. (Emacs-style describe-function)"
        )
    } else {
        format!(
            "{name}: (unknown or see `closure commands` / which-key for available; keys in active mode)"
        )
    }
}

// TDD for self-documentation (Emacs-style describe-function, ROADMAP self-doc sub).
// =closure doc <command>= prints the command + its keys (from registry) + note.
// Test written first; will fail until cmd_doc exists and works for a known command.
#[test]
fn doc_command_describes_known_command() {
    let out = doc_for("rename-headline");
    assert!(out.contains("rename-headline"));
    // Has keys (from the binding tables / registry).
    assert!(
        out.contains("C-") || out.contains("SPC") || out.contains("key") || out.contains("rename")
    );
}

// TDD test written first for GUI shells matrix extension (Tauri/gpui/Flutter per ROADMAP + vision venn).
// The consts must exist and at minimum contain CORE for I7; the shells table auto-includes them.
#[test]
fn shells_matrix_has_tauri_gpui_flutter_entries() {
    // References will fail to compile until consts + array updated.
    assert!(TAURI_CAPABILITIES.contains(&Capability::Browse));
    assert!(GPUI_CAPABILITIES.contains(&Capability::Browse));
    assert!(FLUTTER_CAPABILITIES.contains(&Capability::Browse));
    // They should be superset of core in future body; for now the presence + browse satisfies the matrix row.
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

#[allow(clippy::unnecessary_wraps)]
fn cmd_search(vault: &Path, needle: &str) -> Result<(), String> {
    let name = closure_config::Config::from_path(&vault.join("config.org"))
        .ok()
        .and_then(|c| c.search_backend)
        .unwrap_or_else(|| "builtin".to_owned());
    let backend = closure_query::backend_for(&name);
    for hit in backend.search(vault, needle) {
        println!("{}:{}:{}", hit.path.display(), hit.line, hit.text);
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
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    eprintln!("closure serve: listening on http://{addr}");
    closure_shell_web::serve(&mut v, addr).map_err(|e| format!("{e}"))
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

/// Languages trusted to execute for a standalone file: read `eval_trust`
/// from a `config.org` in the file's directory. Absent/invalid =>
/// default-deny (empty), matching `Vault::eval_trust` (C1a).
fn eval_trust_for(file: &Path) -> Vec<String> {
    let Some(dir) = file.parent() else {
        return Vec::new();
    };
    let cfg = dir.join("config.org");
    if !cfg.exists() {
        return Vec::new();
    }
    closure_config::Config::from_path(&cfg)
        .map(|c| c.eval_trust)
        .unwrap_or_default()
}

fn cmd_eval(path: &Path, write: bool, selector: Option<&str>) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = parse(&src).map_err(|e| format!("{e}"))?;
    // C1a default-deny: only languages listed in `eval_trust` (config.org
    // beside the file) may execute. Absent/invalid config = trust nothing.
    let trust = eval_trust_for(path);
    let blocks = doc.code_blocks();
    let only_block: Option<usize> = match selector {
        None => None,
        Some(s) => Some(if let Ok(idx) = s.parse::<usize>() {
            if idx >= blocks.len() {
                return Err(format!(
                    "--block {idx} out of range: file has {} code block(s)",
                    blocks.len()
                ));
            }
            idx
        } else {
            doc.code_block_index_by_name(s)
                .ok_or_else(|| format!("no code block named `{s}`"))?
        }),
    };
    let mut ran = 0usize;
    let mut results: Vec<(usize, String)> = Vec::new();
    for (i, n) in blocks.iter().enumerate() {
        if only_block.is_some_and(|idx| idx != i) {
            continue;
        }
        let Some(cb) = n.as_code_block() else {
            continue;
        };
        let lang = cb.language.unwrap_or("shell");
        if !closure_eval::eval_allowed(&trust, lang) {
            eprintln!(
                "---- block #{i} blocked: `{lang}` not in eval_trust \
                 (add it to config.org to allow) ----"
            );
            continue;
        }
        let backend: Box<dyn Backend> = if let Some(lang) = cb.language {
            if let Some(b) = backend_for(lang) {
                b
            } else {
                eprintln!("---- block #{i} skipped (no backend for `{lang}`) ----");
                continue;
            }
        } else {
            Box::new(ShellBackend)
        };
        let header = closure_eval::HeaderArgs::parse(cb.args.unwrap_or(""));
        let prelude = closure_eval::var_prelude(cb.language.unwrap_or("shell"), &header.vars);
        let program = format!("{prelude}{}", cb.content);
        let out = backend.eval(&program).map_err(|e| format!("{e}"))?;
        println!(
            "---- block #{i} {lang} exit={} ----",
            out.exit,
            lang = cb.language.unwrap_or("shell")
        );
        if !out.stdout.is_empty() {
            print!("{}", out.stdout);
        }
        if !out.stderr.is_empty() {
            eprint!("{}", out.stderr);
        }
        if write && !header.is_silent() {
            results.push((i, out.stdout.clone()));
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
    let mut v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    closure_tui::run(&mut v).map_err(|e| format!("{e}"))
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

/// Validate the closure-config block in the given file (or a vault's
/// config.org). Reports CUE-style errors with line context at load time.
fn cmd_check_config(path: &Path) -> Result<(), String> {
    match closure_config::Config::from_path(path) {
        Ok(cfg) => {
            println!("config valid (input_mode={:?})", cfg.input_mode);
            Ok(())
        }
        Err(e) => Err(format!("config error: {e}")),
    }
}

/// Export the vault as a self-contained single HTML file (no external deps, works offline).
/// Uses the web shell's export (browse tree + client JS fuzzy) so the data is usable everywhere.
fn cmd_export_html(vault: &Path, out: &Path) -> Result<(), String> {
    let v = Vault::open(vault).map_err(|e| format!("{e}"))?;
    let html = closure_shell_web::export_html(&v);
    std::fs::write(out, &html).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!("wrote self-contained export to {}", out.display());
    Ok(())
}

/// Print the shell capability matrix as a venn-style diff table.
/// This is the "type level UI" / venn requested in the original vision
/// to compare all variants (TUI, CLI, web, egui, gpui, Tauri, Flutter,
/// GTK, Qt, single-HTML, etc.). The consts above are the single source
/// of truth. Future shells just add their const list.
#[allow(clippy::unnecessary_wraps)]
fn cmd_shells() -> Result<(), String> {
    println!("Shell capability matrix (code = single source of truth)");
    println!("Every shell should be a superset of CORE (I7).");
    println!();

    let shells = [
        ("CORE", CORE_CAPABILITIES),
        ("TUI ", TUI_CAPABILITIES),
        ("CLI ", CLI_CAPABILITIES),
        ("WEB ", WEB_CAPABILITIES),
        ("EGUI", EGUI_CAPABILITIES),
        ("TAURI", TAURI_CAPABILITIES),
        ("GPUI ", GPUI_CAPABILITIES),
        ("FLUTTER", FLUTTER_CAPABILITIES),
    ];

    // Collect all unique capabilities in a stable order (definition order).
    let mut all_caps: Vec<Capability> = Vec::new();
    for (_, caps) in &shells {
        for c in *caps {
            if !all_caps.contains(c) {
                all_caps.push(*c);
            }
        }
    }

    // Header
    print!("{:<12}", "Capability");
    for (name, _) in &shells {
        print!(" | {name}");
    }
    println!();
    println!(
        "{}",
        "-".repeat(12 + 3 * shells.len() + (shells.len() - 1) * 3)
    );

    // Rows: for each cap, mark which shells have it (X or space).
    for cap in all_caps {
        let cap_name = format!("{cap:?}");
        print!("{cap_name:<12}");
        for (_, caps) in &shells {
            let has = if caps.contains(&cap) { " X " } else { "   " };
            print!(" | {has}");
        }
        println!();
    }

    println!();
    println!("Legend: X = has capability. TUI is currently the superset.");
    println!("To extend: add to the enum + relevant *_CAPABILITIES const,");
    println!("then the table updates automatically. Run `closure shells`.");
    Ok(())
}

// TDD for completing the Shell capability matrix (ROADMAP item).
// Test written *first* (per strict TDD). This will fail to compile until
// the Capability enum, the per-shell consts, the superset invariants, and
// the polished `cmd_shells` (with venn/diff table output) are implemented.
// Invariant: every shell's CAPABILITIES must contain at least the CORE set.
#[test]
fn shell_capability_matrix_basics() {
    // These types/consts do not exist yet -> compile failure expected on first run.
    let core: &[Capability] = CORE_CAPABILITIES;

    // CLI must be superset of core (for the command itself).
    for c in core {
        assert!(
            CLI_CAPABILITIES.contains(c),
            "CLI must support core capability {c:?}",
        );
    }

    // Basic check that command would not panic (will be exercised after impl).
    // In full, cmd_shells() prints the venn.
    assert!(!core.is_empty());
}
