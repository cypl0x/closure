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
use closure_core::{BlockId, Command, Document, EnsureId, Registry, RenameHeadline};
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
        Cmd::Eval { file } => cmd_eval(file),
        Cmd::Backlinks { vault, id } => cmd_backlinks(vault, id),
        Cmd::Id { file } => cmd_id(file),
        Cmd::Db { vault } => cmd_db(vault),
        Cmd::Serve { vault, addr } => cmd_serve(vault, addr),
        Cmd::Sync { vault, op, message } => cmd_sync(vault, op, message.as_deref()),
    }
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
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
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

fn cmd_eval(path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = parse(&src).map_err(|e| format!("{e}"))?;
    let mut ran = 0usize;
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
        ran += 1;
    }
    if ran == 0 {
        eprintln!("no code blocks found");
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
