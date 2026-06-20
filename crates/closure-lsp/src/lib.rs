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
use closure_store::Vault;
use thiserror::Error;

/// Read one `Content-Length: N\r\n\r\n<body>` LSP frame from `input`,
/// returning the JSON body. `None` at clean EOF.
fn read_frame<R: BufRead>(input: &mut R) -> Result<Option<String>, LspError> {
    let mut content_len: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = input
            .read_line(&mut header)
            .map_err(|e| LspError::Transport(e.to_string()))?;
        if n == 0 {
            return Ok(None); // EOF before any header
        }
        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            content_len = v.trim().parse().ok();
        }
    }
    let len = content_len.ok_or_else(|| LspError::Transport("missing Content-Length".into()))?;
    let mut buf = vec![0u8; len];
    std::io::Read::read_exact(input, &mut buf).map_err(|e| LspError::Transport(e.to_string()))?;
    String::from_utf8(buf)
        .map(Some)
        .map_err(|e| LspError::Transport(e.to_string()))
}

/// Write a JSON `body` as one LSP `Content-Length` frame.
fn write_frame<W: std::io::Write>(out: &mut W, body: &str) -> Result<(), LspError> {
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())
        .map_err(|e| LspError::Transport(e.to_string()))
}

/// Serve LSP over Content-Length-framed JSON-RPC on `input`/`output`.
///
/// Handles `initialize`, `textDocument/documentSymbol` (over
/// [`document_symbols`]), `textDocument/hover` (over [`hover`]), and
/// `shutdown`; everything else is a `-32601` error. All read the doc
/// through `vault` and are read-only — no mutation. Requests carrying an
/// `id` get a framed response.
///
/// # Errors
///
/// [`LspError::Transport`] on IO / framing failure.
pub fn serve<R: BufRead, W: std::io::Write>(
    vault: &Vault,
    mut input: R,
    output: &mut W,
) -> Result<(), LspError> {
    while let Some(msg) = read_frame(&mut input)? {
        if let Some(resp) = handle_message(vault, &msg) {
            write_frame(output, &resp)?;
        }
    }
    Ok(())
}

/// Handle one LSP JSON-RPC message; `None` for notifications (no `id`).
#[must_use]
pub fn handle_message(vault: &Vault, json: &str) -> Option<String> {
    use closure_jsonrpc::{json_escape, raw_field, string_field};
    let id = raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => "{\"capabilities\":{\"documentSymbolProvider\":true,\
             \"hoverProvider\":true},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
            .to_owned(),
        "shutdown" => "null".to_owned(),
        "textDocument/hover" => {
            let uri = string_field(json, "uri").unwrap_or_default();
            let rel = uri.strip_prefix("file://").unwrap_or(&uri);
            let src = vault
                .document_relative(std::path::Path::new(rel))
                .map(closure_core::Document::source)
                .unwrap_or_default();
            let line = raw_field(json, "line").and_then(|s| s.parse().ok()).unwrap_or(0);
            let ch = raw_field(json, "character")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            hover(&src, vault, line, ch).map_or_else(
                || "null".to_owned(),
                |h| {
                    format!(
                        "{{\"contents\":{{\"kind\":\"plaintext\",\"value\":\"{}\"}}}}",
                        json_escape(&h)
                    )
                },
            )
        }
        "textDocument/documentSymbol" => {
            let uri = string_field(json, "uri").unwrap_or_default();
            let rel = uri.strip_prefix("file://").unwrap_or(&uri);
            let src = vault
                .document_relative(std::path::Path::new(rel))
                .map(closure_core::Document::source)
                .unwrap_or_default();
            let items: Vec<String> = document_symbols(&src)
                .iter()
                .map(|s| {
                    let line = s.line;
                    format!(
                        "{{\"name\":\"{}\",\"kind\":6,\"range\":{{\"start\":\
                         {{\"line\":{line},\"character\":0}},\"end\":\
                         {{\"line\":{line},\"character\":0}}}},\"selectionRange\":\
                         {{\"start\":{{\"line\":{line},\"character\":0}},\"end\":\
                         {{\"line\":{line},\"character\":0}}}}}}",
                        json_escape(&s.name),
                    )
                })
                .collect();
            format!("[{}]", items.join(","))
        }
        _ => return Some(closure_jsonrpc::method_not_found(&id)),
    };
    Some(closure_jsonrpc::response(&id, &result))
}

/// Run the LSP server on stdio against `vault`.
///
/// # Errors
///
/// [`LspError::Transport`] on IO failure.
pub fn serve_stdio(vault: &Vault) -> Result<(), LspError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    serve(vault, reader, &mut stdout)
}

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

/// True when `line` is an org headline (one-or-more leading `*` then a
/// space).
fn is_headline_line(line: &str) -> bool {
    let stars = line.bytes().take_while(|&b| b == b'*').count();
    stars > 0 && line[stars..].starts_with(' ')
}

/// The `id:` link value whose `id:<value>` span covers byte column
/// `character` on `line`, if any. Value = the run of ASCII alphanumerics
/// after `id:` (a ULID).
fn id_at(line: &str, character: usize) -> Option<&str> {
    let mut from = 0;
    while let Some(rel) = line[from..].find("id:") {
        let kw = from + rel;
        let val_start = kw + 3;
        let val_len = line[val_start..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(line.len() - val_start);
        let val_end = val_start + val_len;
        if val_end > val_start && character >= kw && character <= val_end {
            return Some(&line[val_start..val_end]);
        }
        from = val_start;
    }
    None
}

/// Hover info for the symbol under a zero-based `line`/`character`.
///
/// Over an `id:` link, previews the linked headline: its title plus a
/// `file › ancestor › … › title` breadcrumb. Over a headline, describes
/// it (`level N · id:… · TODO kw · :tags:`). `None` when nothing
/// resolvable sits under the cursor or the position is out of range.
#[must_use]
pub fn hover(src: &str, vault: &Vault, line: u32, character: u32) -> Option<String> {
    let text = src.lines().nth(line as usize)?;
    if let Some(id) = id_at(text, character as usize) {
        return hover_id(vault, id);
    }
    if is_headline_line(text) {
        return hover_headline(src, line as usize);
    }
    None
}

/// Preview a link target: title + `file › ancestors › title` breadcrumb.
fn hover_id(vault: &Vault, id: &str) -> Option<String> {
    let (target, path) = vault.find_by_id(&closure_core::BlockId::from_existing(id))?;
    let doc = vault.document(path)?;
    let tpath = target.path();
    let mut crumbs: Vec<&str> = doc
        .all_headlines()
        .filter(|h| h.path().len() < tpath.len() && tpath.starts_with(h.path()))
        .map(closure_core::DocHeadline::title)
        .collect();
    crumbs.push(target.title());
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    Some(format!("{}\n{file} › {}", target.title(), crumbs.join(" › ")))
}

/// Describe the headline whose stars sit on zero-based `line` of `src`.
fn hover_headline(src: &str, line: usize) -> Option<String> {
    // The headline's index in document order = how many headline lines
    // precede (and include) this one.
    let idx = src
        .lines()
        .take(line + 1)
        .filter(|l| is_headline_line(l))
        .count()
        .checked_sub(1)?;
    let doc = closure_core::Document::load_str(src).ok()?;
    let h = doc.all_headlines().nth(idx)?;
    let mut out = format!("level {} · id:{}", h.level(), h.id());
    if let Some(kw) = h.todo() {
        out.push_str(" · ");
        out.push_str(kw);
    }
    if !h.tags().is_empty() {
        out.push_str(" · :");
        out.push_str(&h.tags().join(":"));
        out.push(':');
    }
    Some(out)
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
