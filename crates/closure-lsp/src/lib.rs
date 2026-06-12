//! LSP bridge.
//!
//! Surfaces the command registry as an LSP server so editors (neovim,
//! `VSCode`, helix, emacs-lsp) can drive closure through the same code
//! path as the TUI. The current skeleton uses the same line-oriented
//! protocol as [`closure_mcp`]; full Language Server Protocol framing
//! arrives once a JSON dependency is picked.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader};

use closure_core::Registry;
use thiserror::Error;

/// LSP bridge error.
#[derive(Debug, Error)]
pub enum LspError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
}

/// Per-line resolution outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The named command exists.
    Found(String),
    /// No command matches.
    Unknown(String),
    /// Blank or comment line.
    Skip,
}

/// Resolve a single line.
#[must_use]
pub fn resolve_line(registry: &Registry, line: &str) -> Outcome {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Outcome::Skip;
    }
    let name = trimmed.split_whitespace().next().unwrap_or("");
    if registry.get(name).is_some() {
        Outcome::Found(name.to_owned())
    } else {
        Outcome::Unknown(name.to_owned())
    }
}

/// Run the dispatcher loop.
pub fn run<R: BufRead, W: std::io::Write>(
    registry: &Registry,
    mut input: R,
    output: &mut W,
) -> Result<(), LspError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| LspError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        match resolve_line(registry, &line) {
            Outcome::Found(name) => {
                writeln!(output, "OK {name}").map_err(|e| LspError::Transport(e.to_string()))?;
            }
            Outcome::Unknown(name) => {
                writeln!(output, "UNKNOWN {name}")
                    .map_err(|e| LspError::Transport(e.to_string()))?;
            }
            Outcome::Skip => {}
        }
    }
    Ok(())
}

/// Wrap stdin/stdout for the typical CLI invocation.
pub fn run_stdio(registry: &Registry) -> Result<(), LspError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    run(registry, reader, &mut stdout)
}

/// A document symbol: one org headline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Headline title with TODO keyword, priority cookie, and tags
    /// stripped.
    pub name: String,
    /// Zero-based line number of the headline.
    pub line: u32,
    /// Outline level (number of stars).
    pub level: u8,
}

/// Extract LSP document symbols: one per org headline, in file order.
#[must_use]
pub fn document_symbols(src: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let stars = raw.bytes().take_while(|&b| b == b'*').count();
        if stars == 0 || !raw[stars..].starts_with(' ') {
            continue;
        }
        let mut rest = raw[stars..].trim();
        // Strip a leading all-uppercase TODO keyword.
        if let Some((first, tail)) = rest.split_once(' ')
            && first.len() >= 2
            && first.chars().all(|c| c.is_ascii_uppercase())
        {
            rest = tail.trim_start();
        }
        // Strip a priority cookie like [#A].
        if rest.starts_with("[#")
            && let Some(close) = rest.find(']')
        {
            rest = rest[close + 1..].trim_start();
        }
        // Strip trailing :tag:lists:.
        let name = rest
            .rsplit_once(" :")
            .filter(|(_, tags)| tags.ends_with(':'))
            .map_or(rest, |(head, _)| head.trim_end());
        out.push(Symbol {
            name: name.to_owned(),
            line: u32::try_from(i).unwrap_or(u32::MAX),
            level: u8::try_from(stars).unwrap_or(u8::MAX),
        });
    }
    out
}

/// Resolve an `id:<ULID>` (or bare ULID) link target to its defining
/// file and the zero-based line of the owning headline.
#[must_use]
pub fn definition_of(
    vault: &closure_store::Vault,
    target: &str,
) -> Option<(std::path::PathBuf, u32)> {
    let id = target.strip_prefix("id:").unwrap_or(target);
    let (_, path) = vault.find_by_id(&closure_core::BlockId::from_existing(id))?;
    let src = vault.document(path)?.source();
    let needle = format!(":ID: {id}");
    let mut headline_line = 0u32;
    for (i, line) in src.lines().enumerate() {
        if line.starts_with('*') {
            headline_line = u32::try_from(i).unwrap_or(u32::MAX);
        }
        if line.trim() == needle {
            return Some((path.to_path_buf(), headline_line));
        }
    }
    Some((path.to_path_buf(), 0))
}
