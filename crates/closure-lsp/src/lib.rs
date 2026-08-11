//! LSP bridge.
//!
//! Surfaces the command registry as an LSP server so editors (neovim,
//! `VSCode`, helix, emacs-lsp) can drive closure through the same code
//! path as the TUI. [`serve`] speaks real Content-Length-framed JSON-RPC
//! over any reader/writer (stdio via [`serve_stdio`], the `closure lsp`
//! CLI): `initialize` advertises the capabilities, then hover,
//! completion, diagnostics, document symbols, references (read-only) and
//! rename (server-authoritative, I8) are dispatched per request. Pure +
//! hermetic — every method is a function over `&Vault` / `&mut Vault`,
//! tested without an editor process.

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
///
/// Flushed, and that is not a detail. Rust's stdout is line-buffered
/// and an LSP frame ends with the JSON body, which has no newline —
/// so the header went out and the answer sat in the buffer until the
/// process exited. Against a client that closes the pipe (a shell
/// pipeline, a test) it looked perfect; against a client that keeps
/// the connection open, which is every editor, the server answered
/// nothing, ever.
fn write_frame<W: std::io::Write>(out: &mut W, body: &str) -> Result<(), LspError> {
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())
        .map_err(|e| LspError::Transport(e.to_string()))?;
    out.flush().map_err(|e| LspError::Transport(e.to_string()))
}

/// Serve LSP over Content-Length-framed JSON-RPC on `input`/`output`.
///
/// Dispatches every method via [`handle_message_mut`]: `initialize`,
/// `textDocument/{documentSymbol,hover,completion,diagnostic,references}`
/// (read-only) and `textDocument/rename` (server-authoritative, I8), plus
/// `shutdown`; unknown methods get a `-32601` error. The `initialized`
/// notification is a no-op; an `exit` notification stops the loop (as
/// does clean EOF). Requests carrying an `id` get a framed response.
///
/// # Errors
///
/// [`LspError::Transport`] on IO / framing failure.
pub fn serve<R: BufRead, W: std::io::Write>(
    vault: &mut Vault,
    input: R,
    output: &mut W,
) -> Result<(), LspError> {
    serve_with(vault, &mut Embeddings::default(), input, output)
}

/// [`serve`], forwarding positions inside a `#+BEGIN_SRC` block to the
/// language server configured for that block's language.
///
/// "org-edit-special on src blocks and then fiddle with the source
/// code": inside `#+BEGIN_SRC rust` the thing that knows what the
/// cursor is on is rust-analyzer, and the thing that knows where the
/// block's line 12 lives in the file is this.
///
/// # Errors
///
/// [`LspError::Transport`] on IO / framing failure.
pub fn serve_with<R: BufRead, W: std::io::Write>(
    vault: &mut Vault,
    embeddings: &mut Embeddings,
    mut input: R,
    output: &mut W,
) -> Result<(), LspError> {
    let mut overlay = Overlay::default();
    while let Some(msg) = read_frame(&mut input)? {
        if let Some(resp) = embedded_answer(vault, &overlay, embeddings, &msg) {
            write_frame(output, &resp)?;
        } else if let Some(resp) = handle_message_with(vault, &mut overlay, &msg) {
            write_frame(output, &resp)?;
        }
        if closure_jsonrpc::string_field(&msg, "method").as_deref() == Some("exit") {
            break;
        }
    }
    Ok(())
}

/// The answer from a src block's own language server, when the request
/// is a position inside one and a server is configured for it.
///
/// `None` means "not one of theirs", which is how the request falls
/// through to closure's own answer rather than being swallowed.
fn embedded_answer(
    vault: &Vault,
    overlay: &Overlay,
    embeddings: &mut Embeddings,
    msg: &str,
) -> Option<String> {
    if embeddings.is_empty() {
        return None;
    }
    let method = closure_jsonrpc::string_field(msg, "method")?;
    if !matches!(
        method.as_str(),
        "textDocument/hover" | "textDocument/definition" | "textDocument/completion"
    ) {
        return None;
    }
    let id = closure_jsonrpc::raw_field(msg, "id")?;
    let (line, character) = req_position(msg);
    let block = SrcBlock::at(&req_source(vault, overlay, msg), line)?;
    // A server that fails has nothing to say about this position —
    // better than an error the editor shows as a broken feature for
    // the rest of the session.
    embeddings
        .ask(&method, &block, line, character)?
        .ok()
        .map(|result| closure_jsonrpc::response(&id, &result))
}

/// The capabilities `initialize` advertises.
const INITIALIZE_RESULT: &str = "{\"capabilities\":{\"documentSymbolProvider\":true,\
     \"hoverProvider\":true,\
     \"completionProvider\":{\"triggerCharacters\":[\":\"]},\
     \"diagnosticProvider\":{\"interFileDependencies\":true,\
     \"workspaceDiagnostics\":false},\
     \"referencesProvider\":true,\"renameProvider\":true,\
     \"definitionProvider\":true},\
     \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}";

/// The source text of the document the request's `uri` names (relative
/// to the vault root); empty when absent.
fn req_source(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let uri = closure_jsonrpc::string_field(json, "uri").unwrap_or_default();
    // What the editor has open wins over what is saved — that is the
    // whole premise of the protocol, and the only answer available at
    // all for a document with no path, like a src block handed over on
    // its own.
    if let Some(text) = overlay.get(&uri) {
        return text.to_owned();
    }
    let rel = uri.strip_prefix("file://").unwrap_or(&uri);
    vault
        .document_relative(std::path::Path::new(rel))
        .map(closure_core::Document::source)
        .unwrap_or_default()
}

/// The documents the client has open, by URI.
///
/// LSP's premise: the client owns the buffer and sends it, because the
/// interesting state is the one not saved yet. A server that re-reads
/// the path answers about a file that agrees with yours only between
/// saves.
#[derive(Debug, Default, Clone)]
pub struct Overlay(std::collections::HashMap<String, String>);

impl Overlay {
    /// The text the client has for `uri`, if it has any.
    #[must_use]
    pub fn get(&self, uri: &str) -> Option<&str> {
        self.0.get(uri).map(String::as_str)
    }

    /// Apply a `textDocument/did*` notification. `true` when it was one.
    ///
    /// `didChange` takes the last `text` in `contentChanges`, which is
    /// the whole document: closure advertises no incremental sync, so
    /// that is what a client sends.
    pub fn absorb(&mut self, json: &str) -> bool {
        let method = closure_jsonrpc::string_field(json, "method").unwrap_or_default();
        let uri = closure_jsonrpc::string_field(json, "uri").unwrap_or_default();
        match method.as_str() {
            "textDocument/didOpen" | "textDocument/didChange" => {
                if let Some(text) = closure_jsonrpc::string_field(json, "text") {
                    self.0.insert(uri, text);
                }
                true
            }
            "textDocument/didClose" => {
                self.0.remove(&uri);
                true
            }
            _ => false,
        }
    }
}

/// The request's zero-based `(line, character)` position (defaults `0`).
fn req_position(json: &str) -> (u32, u32) {
    let n = |key| {
        closure_jsonrpc::raw_field(json, key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    };
    (n("line"), n("character"))
}

/// `textDocument/hover` result fragment.
fn hover_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let (line, ch) = req_position(json);
    hover(&src, vault, line, ch).map_or_else(
        || "null".to_owned(),
        |h| {
            format!(
                "{{\"contents\":{{\"kind\":\"plaintext\",\"value\":\"{}\"}}}}",
                closure_jsonrpc::json_escape(&h)
            )
        },
    )
}

/// `textDocument/completion` result fragment.
fn completion_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let (line, ch) = req_position(json);
    let items: Vec<String> = completion(&src, vault, line, ch)
        .iter()
        .map(|i| {
            format!(
                "{{\"label\":\"{}\",\"detail\":\"{}\",\"kind\":{}}}",
                closure_jsonrpc::json_escape(&i.label),
                closure_jsonrpc::json_escape(&i.detail),
                i.kind.lsp_kind()
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `textDocument/diagnostic` full-report result fragment.
fn diagnostic_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let items: Vec<String> = diagnostics(&src, vault)
        .iter()
        .map(|d| {
            format!(
                "{{\"range\":{{\"start\":{{\"line\":{l},\"character\":{s}}},\
                 \"end\":{{\"line\":{l},\"character\":{e}}}}},\
                 \"severity\":{sev},\"code\":\"{code}\",\"message\":\"{msg}\"}}",
                l = d.line,
                s = d.start_char,
                e = d.end_char,
                sev = d.severity.lsp_severity(),
                code = d.code.as_str(),
                msg = closure_jsonrpc::json_escape(&d.message),
            )
        })
        .collect();
    format!("{{\"kind\":\"full\",\"items\":[{}]}}", items.join(","))
}

/// `textDocument/references` result fragment: an array of `Location`s
/// for the id under the cursor.
/// `textDocument/definition`: where the id under the cursor lives.
///
/// The one an org language server is actually for. `[[id:01…]]` names
/// a headline that is usually in another file, and following it by
/// hand means grepping a ULID — which is exactly the thing ids exist
/// so you would not have to do.
///
/// `null` rather than an empty list when the cursor is not on an id:
/// LSP treats `[]` as "there is a definition and it is nowhere", and
/// editors show that as a failed jump rather than as no jump.
fn definition_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let (line, ch) = req_position(json);
    let Some(id) = id_at_position(&src, line, ch) else {
        return "null".to_owned();
    };
    let bid = closure_core::BlockId::from_existing(&id);
    let Some((headline, path)) = vault.find_by_id(&bid) else {
        return "null".to_owned();
    };
    // Which line the headline starts on, counted the way the rest of
    // this file counts: headlines in document order.
    let want = headline.id().to_string();
    let Some(doc) = vault.document(path) else {
        return "null".to_owned();
    };
    let nth = doc
        .all_headlines()
        .position(|h| h.id().as_str() == want)
        .unwrap_or_default();
    let target = doc
        .source()
        .lines()
        .enumerate()
        .filter(|(_, l)| is_headline_line(l))
        .nth(nth)
        .map_or(0, |(n, _)| n);
    format!(
        "{{\"uri\":\"file://{}\",\"range\":{{\"start\":{{\"line\":{target},\"character\":0}},\
         \"end\":{{\"line\":{target},\"character\":0}}}}}}",
        closure_jsonrpc::json_escape(&path.display().to_string())
    )
}

fn references_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let (line, ch) = req_position(json);
    let Some(id) = id_at_position(&src, line, ch) else {
        return "[]".to_owned();
    };
    let items: Vec<String> = references(vault, &id)
        .iter()
        .map(|(path, l)| {
            format!(
                "{{\"uri\":\"file://{}\",\"range\":{{\"start\":{{\"line\":{l},\"character\":0}},\
                 \"end\":{{\"line\":{l},\"character\":0}}}}}}",
                closure_jsonrpc::json_escape(&path.display().to_string()),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `textDocument/rename` result fragment. closure is server-authoritative
/// (mutations route through the registry + persist, I8), so the rename is
/// applied here and the response is `null` rather than a client-applied
/// `WorkspaceEdit` — see Decision (2026-06-20).
fn rename_result(vault: &mut Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
    let (line, ch) = req_position(json);
    let new_name = closure_jsonrpc::string_field(json, "newName").unwrap_or_default();
    if let Some(id) = id_at_position(&src, line, ch) {
        let _ = rename_symbol(vault, &id, &new_name);
    }
    "null".to_owned()
}

/// `textDocument/documentSymbol` result fragment.
fn symbol_result(vault: &Vault, overlay: &Overlay, json: &str) -> String {
    let src = req_source(vault, overlay, json);
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
                closure_jsonrpc::json_escape(&s.name),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Handle one LSP JSON-RPC message; `None` for notifications (no `id`).
#[must_use]
pub fn handle_message(vault: &Vault, json: &str) -> Option<String> {
    handle_message_over(vault, &Overlay::default(), json)
}

/// [`handle_message`], answering about the documents the client has
/// open rather than about the files on disk.
#[must_use]
pub fn handle_message_over(vault: &Vault, overlay: &Overlay, json: &str) -> Option<String> {
    let id = closure_jsonrpc::raw_field(json, "id")?;
    let method = closure_jsonrpc::string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => INITIALIZE_RESULT.to_owned(),
        "shutdown" => "null".to_owned(),
        "textDocument/hover" => hover_result(vault, overlay, json),
        "textDocument/completion" => completion_result(vault, overlay, json),
        "textDocument/diagnostic" => diagnostic_result(vault, overlay, json),
        "textDocument/documentSymbol" => symbol_result(vault, overlay, json),
        "textDocument/references" => references_result(vault, overlay, json),
        "textDocument/definition" => definition_result(vault, overlay, json),
        _ => return Some(closure_jsonrpc::method_not_found(&id)),
    };
    Some(closure_jsonrpc::response(&id, &result))
}

/// Handle one message that may mutate the vault.
///
/// Dispatches `textDocument/rename` (server-authoritative, I8) and
/// delegates every read-only method to [`handle_message`]. This is the
/// entry point the stdio loop uses; `None` for notifications.
#[must_use]
pub fn handle_message_mut(vault: &mut Vault, json: &str) -> Option<String> {
    handle_message_with(vault, &mut Overlay::default(), json)
}

/// [`handle_message_mut`], keeping `overlay` up to date with the
/// documents the client says it has open — the entry point the stdio
/// loop uses.
///
/// A `did*` notification is absorbed and answered with `None`, which
/// is what a notification gets.
#[must_use]
pub fn handle_message_with(vault: &mut Vault, overlay: &mut Overlay, json: &str) -> Option<String> {
    if overlay.absorb(json) {
        return None;
    }
    let method = closure_jsonrpc::string_field(json, "method").unwrap_or_default();
    if method == "textDocument/rename" {
        let id = closure_jsonrpc::raw_field(json, "id")?;
        let result = rename_result(vault, overlay, json);
        return Some(closure_jsonrpc::response(&id, &result));
    }
    handle_message_over(vault, overlay, json)
}

/// Run the LSP server on stdio against `vault`, with the language
/// servers `embeddings` names answering for what is inside src blocks.
///
/// # Errors
///
/// [`LspError::Transport`] on IO failure.
pub fn serve_stdio_with(vault: &mut Vault, embeddings: &mut Embeddings) -> Result<(), LspError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    serve_with(vault, embeddings, reader, &mut stdout)
}

/// Run the LSP server on stdio against `vault`, with no src-block
/// servers.
///
/// # Errors
///
/// [`LspError::Transport`] on IO / framing failure.
pub fn serve_stdio(vault: &mut Vault) -> Result<(), LspError> {
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
    /// A vault mutation (e.g. rename) failed.
    #[error("vault: {0}")]
    Vault(String),
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
    Some(format!(
        "{}\n{file} › {}",
        target.title(),
        crumbs.join(" › ")
    ))
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

/// What a completion item refers to (maps to an LSP `CompletionItemKind`
/// in the protocol layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// An `id:` link target.
    Reference,
    /// A TODO keyword.
    Keyword,
    /// A headline tag.
    Tag,
}

impl CompletionKind {
    /// The LSP `CompletionItemKind` number.
    #[must_use]
    pub const fn lsp_kind(self) -> u8 {
        match self {
            Self::Reference => 18, // Reference
            Self::Keyword => 14,   // Keyword
            Self::Tag => 20,       // EnumMember
        }
    }
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// Text inserted / shown.
    pub label: String,
    /// Secondary text (e.g. a link target's title).
    pub detail: String,
    /// What the label refers to.
    pub kind: CompletionKind,
}

/// The `id:` value-prefix being typed at end of `prefix` (text up to the
/// cursor): everything after the last `id:`, when it is all ULID chars.
fn id_partial(prefix: &str) -> Option<&str> {
    let at = prefix.rfind("id:")?;
    let partial = &prefix[at + 3..];
    partial
        .chars()
        .all(|c| c.is_ascii_alphanumeric())
        .then_some(partial)
}

/// The TODO-keyword being typed in a headline's keyword slot: the word
/// right after the leading stars + one space, when nothing else follows.
fn keyword_partial(prefix: &str) -> Option<&str> {
    let stars = prefix.bytes().take_while(|&b| b == b'*').count();
    if stars == 0 {
        return None;
    }
    let after = prefix.get(stars + 1..)?;
    if prefix.as_bytes().get(stars) != Some(&b' ') {
        return None;
    }
    (!after.contains([' ', ':'])).then_some(after)
}

/// The tag being typed in a trailing `:tag:` region: text after the last
/// `:` of a ` :…` block whose chars are all tag-legal.
fn tag_partial(prefix: &str) -> Option<&str> {
    let region = &prefix[prefix.rfind(" :")? + 2..];
    let tag_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '#' | '%' | ':');
    if !region.chars().all(tag_char) {
        return None;
    }
    Some(region.rsplit(':').next().unwrap_or(region))
}

/// Context-sensitive completion at a zero-based `line`/`character`.
///
/// Returns sorted candidates for the slot under the cursor: vault ids
/// inside an unterminated `[[id:` (title as `detail`), the configured
/// TODO keywords in a headline's keyword slot, or known vault tags in a
/// trailing `:tag:` region. Empty when no completable context applies.
#[must_use]
pub fn completion(src: &str, vault: &Vault, line: u32, character: u32) -> Vec<CompletionItem> {
    let Some(text) = src.lines().nth(line as usize) else {
        return Vec::new();
    };
    let cut = (character as usize).min(text.len());
    let prefix = &text[..cut];

    let mut items = id_partial(prefix)
        .map(|partial| id_items(vault, partial))
        .or_else(|| is_headline_line(text).then(|| headline_items(vault, prefix)))
        .unwrap_or_default();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup();
    items
}

/// Vault ids whose value starts with `partial` (case-insensitive), with
/// the owning headline's title as `detail`.
fn id_items(vault: &Vault, partial: &str) -> Vec<CompletionItem> {
    let want = partial.to_ascii_uppercase();
    let mut out = Vec::new();
    for (_p, doc) in vault.iter() {
        for h in doc.all_headlines() {
            if let Some((_, id)) = h
                .properties()
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("ID"))
                && id.to_ascii_uppercase().starts_with(&want)
            {
                out.push(CompletionItem {
                    label: id.clone(),
                    detail: h.title().to_owned(),
                    kind: CompletionKind::Reference,
                });
            }
        }
    }
    out
}

/// Tag completions (trailing `:tag:` region) else TODO-keyword
/// completions (headline keyword slot); empty when neither applies.
fn headline_items(vault: &Vault, prefix: &str) -> Vec<CompletionItem> {
    tag_partial(prefix)
        .map(|partial| {
            let want = partial.to_ascii_lowercase();
            vault
                .all_tags()
                .into_iter()
                .filter(|t| t.to_ascii_lowercase().starts_with(&want))
                .map(|t| CompletionItem {
                    label: t,
                    detail: String::new(),
                    kind: CompletionKind::Tag,
                })
                .collect()
        })
        .or_else(|| {
            keyword_partial(prefix).map(|partial| {
                vault
                    .todo_keywords()
                    .into_iter()
                    .filter(|k| k.starts_with(partial))
                    .map(|k| CompletionItem {
                        label: k,
                        detail: String::new(),
                        kind: CompletionKind::Keyword,
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Diagnostic severity (maps to an LSP `DiagnosticSeverity` number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A problem that breaks something (LSP 1).
    Error,
    /// A non-fatal concern (LSP 2).
    Warning,
}

impl Severity {
    /// The LSP `DiagnosticSeverity` number.
    #[must_use]
    pub const fn lsp_severity(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warning => 2,
        }
    }
}

/// What kind of problem a diagnostic reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    /// An `id:` link whose target is not in the vault.
    DeadLink,
    /// An `:ID:` value that occurs more than once across the vault.
    DuplicateId,
    /// A `closure-config` block validation error.
    Config,
    /// A `closure-widget` expansion error (unknown / cyclic reference).
    Widget,
    /// A `[fn:name]` reference with no `[fn:name]` definition (Q5-O2).
    Footnote,
}

impl DiagnosticCode {
    /// A stable machine-readable code string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeadLink => "dead-link",
            Self::DuplicateId => "duplicate-id",
            Self::Config => "config",
            Self::Widget => "widget",
            Self::Footnote => "footnote",
        }
    }
}

/// One ranged problem on a single source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based start column (byte).
    pub start_char: u32,
    /// Zero-based end column (byte, exclusive).
    pub end_char: u32,
    /// Severity.
    pub severity: Severity,
    /// What kind of problem.
    pub code: DiagnosticCode,
    /// Human-readable message.
    pub message: String,
}

/// Every `id:<value>` token on `line` as `(start, end_exclusive, value)`
/// byte spans (value = the ASCII-alphanumeric run after `id:`).
fn id_tokens(line: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = line[from..].find("id:") {
        let kw = from + rel;
        let val_start = kw + 3;
        let val_len = line[val_start..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(line.len() - val_start);
        let val_end = val_start + val_len;
        if val_end > val_start {
            out.push((kw, val_end, &line[val_start..val_end]));
        }
        from = val_start;
    }
    out
}

/// Vault-wide count of each `:ID:` value (from headline `ID` properties).
fn id_counts(vault: &Vault) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for (_p, doc) in vault.iter() {
        for h in doc.all_headlines() {
            if let Some((_, id)) = h
                .properties()
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("ID"))
            {
                *counts.entry(id.clone()).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// Find the `closure-config` block in `src`, returning its content lines
/// joined plus the zero-based document line of the first content line.
fn config_block(src: &str) -> Option<(String, usize)> {
    let mut lines = src.lines().enumerate();
    let start = lines.by_ref().find_map(|(i, l)| {
        let t = l.trim();
        (t.to_ascii_uppercase().starts_with("#+BEGIN_SRC") && t.contains("closure-config"))
            .then_some(i)
    })?;
    let mut content = String::new();
    for (_, l) in lines {
        if l.trim().eq_ignore_ascii_case("#+END_SRC") {
            break;
        }
        content.push_str(l);
        content.push('\n');
    }
    Some((content, start + 1))
}

/// The 1-based line number embedded as `line N` in a config error
/// message, if any.
fn embedded_line(message: &str) -> Option<usize> {
    let at = message.find("line ")?;
    let digits: String = message[at + 5..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Diagnostics for `src` against `vault`: dead `id:` links, duplicate
/// `:ID:` values (vault-wide), and `closure-config` validation errors.
///
/// The footnote name when `line` opens with a `[fn:name]` definition.
fn footnote_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("[fn:")?;
    let close = rest.find(']')?;
    (close > 0).then(|| &rest[..close])
}

/// Q5-O2: dead footnote references. A definition is a line starting
/// `[fn:name]`; any other `[fn:name]` occurrence is a reference and
/// warns when no definition names it.
fn footnote_diagnostics(src: &str, out: &mut Vec<Diagnostic>) {
    let defined: std::collections::HashSet<&str> = src
        .lines()
        .filter_map(|l| footnote_name(l.trim_start()))
        .collect();
    for (i, line) in src.lines().enumerate() {
        let lnum = u32::try_from(i).unwrap_or(u32::MAX);
        let mut cursor = 0usize;
        while let Some(rel) = line[cursor..].find("[fn:") {
            let start = cursor + rel;
            let Some(close) = line[start..].find(']') else {
                break;
            };
            let name = &line[start + 4..start + close];
            let is_definition = start == 0 || line[..start].trim().is_empty();
            if !is_definition && !name.is_empty() && !defined.contains(name) {
                out.push(Diagnostic {
                    line: lnum,
                    start_char: u32::try_from(start).unwrap_or(u32::MAX),
                    end_char: u32::try_from(start + close + 1).unwrap_or(u32::MAX),
                    severity: Severity::Warning,
                    code: DiagnosticCode::Footnote,
                    message: format!("footnote [fn:{name}] has no definition"),
                });
            }
            cursor = start + close + 1;
        }
    }
}

/// Positions are zero-based byte line/column over `src`. Pure +
/// hermetic — no editor process needed.
#[must_use]
pub fn diagnostics(src: &str, vault: &Vault) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let counts = id_counts(vault);

    for (i, line) in src.lines().enumerate() {
        let lnum = u32::try_from(i).unwrap_or(u32::MAX);
        // Dead id: links.
        for (start, end, id) in id_tokens(line) {
            if vault
                .find_by_id(&closure_core::BlockId::from_existing(id))
                .is_none()
            {
                out.push(Diagnostic {
                    line: lnum,
                    start_char: u32::try_from(start).unwrap_or(u32::MAX),
                    end_char: u32::try_from(end).unwrap_or(u32::MAX),
                    severity: Severity::Error,
                    code: DiagnosticCode::DeadLink,
                    message: format!("dead link: id:{id} has no target in the vault"),
                });
            }
        }
        // Duplicate :ID: declaration.
        if let Some(value) = line.trim().strip_prefix(":ID:").map(str::trim)
            && counts.get(value).copied().unwrap_or(0) > 1
        {
            out.push(Diagnostic {
                line: lnum,
                start_char: 0,
                end_char: u32::try_from(line.len()).unwrap_or(u32::MAX),
                severity: Severity::Error,
                code: DiagnosticCode::DuplicateId,
                message: format!(":ID: {value} is declared more than once in the vault"),
            });
        }
    }

    footnote_diagnostics(src, &mut out);

    // closure-config block validation.
    if let Some((content, content_start)) = config_block(src)
        && let Err(e) = closure_config::Config::from_kv_block(&content)
    {
        let message = e.to_string();
        let doc_line =
            embedded_line(&message).map_or(content_start, |n| content_start + n.saturating_sub(1));
        let line = u32::try_from(doc_line).unwrap_or(u32::MAX);
        let end = src
            .lines()
            .nth(doc_line)
            .map_or(0, |l| u32::try_from(l.len()).unwrap_or(u32::MAX));
        out.push(Diagnostic {
            line,
            start_char: 0,
            end_char: end,
            severity: Severity::Error,
            code: DiagnosticCode::Config,
            message,
        });
    }

    // closure-widget expansion errors, resolved against widget
    // definitions across the whole vault (V2b), and pointed at the
    // thing that went wrong rather than at the block it happened in.
    if let Err(e) =
        closure_query::expand_widgets_with(src, &closure_query::vault_widget_defs(vault))
    {
        let (line, start, end) = widget_error_span(src, &e);
        out.push(Diagnostic {
            line,
            start_char: start,
            end_char: end,
            severity: Severity::Error,
            code: DiagnosticCode::Widget,
            message: e.to_string(),
        });
    }

    out
}

/// Where in `src` to underline a composition failure.
///
/// Every one of these used to be reported at the first
/// `#+begin: closure-widget` line in the file, whatever had gone wrong
/// and wherever it had happened — so an editor underlined a block
/// header several lines above a reference that was perfectly visible.
/// The error already knows which name, which argument or which value
/// is at fault; this finds it.
///
/// Falls back to the block header when the text cannot be found, which
/// is better than nothing and is what a depth failure gets: there is no
/// single reference to blame for a nest that went too deep.
fn widget_error_span(src: &str, e: &closure_query::WidgetError) -> (u32, u32, u32) {
    use closure_query::WidgetError as W;
    let needle = match e {
        W::Unknown(name) => format!("{{{{{name}}}}}"),
        W::UnknownArgument { argument, .. } => argument.clone(),
        W::BadArgument { got, .. } => got.clone(),
        W::Cycle(path) => path
            .first()
            .map(|n| format!("{{{{{n}}}}}"))
            .unwrap_or_default(),
        W::TooDeep { .. } => String::new(),
    };
    if !needle.is_empty() {
        for (i, line) in src.lines().enumerate() {
            // Not on the line that *defines* the widget — that is where
            // the name is declared, not where it is used wrongly.
            if line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("#+begin:")
            {
                continue;
            }
            if let Some(at) = line.find(&needle) {
                return (
                    u32::try_from(i).unwrap_or(u32::MAX),
                    u32::try_from(at).unwrap_or(u32::MAX),
                    u32::try_from(at + needle.len()).unwrap_or(u32::MAX),
                );
            }
        }
    }
    let line = src
        .lines()
        .position(|l| {
            l.trim_start()
                .to_ascii_lowercase()
                .starts_with("#+begin: closure-widget")
        })
        .unwrap_or(0);
    let end = src
        .lines()
        .nth(line)
        .map_or(0, |l| u32::try_from(l.len()).unwrap_or(u32::MAX));
    (u32::try_from(line).unwrap_or(u32::MAX), 0, end)
}

/// The id referred to at a zero-based `line`/`character` in `src`: the
/// `id:` link under the cursor, else the `:ID:` of the headline on that
/// line.
#[must_use]
pub fn id_at_position(src: &str, line: u32, character: u32) -> Option<String> {
    let text = src.lines().nth(line as usize)?;
    if let Some(id) = id_at(text, character as usize) {
        return Some(id.to_owned());
    }
    if is_headline_line(text) {
        let idx = src
            .lines()
            .take(line as usize + 1)
            .filter(|l| is_headline_line(l))
            .count()
            .checked_sub(1)?;
        let doc = closure_core::Document::load_str(src).ok()?;
        let h = doc.all_headlines().nth(idx)?;
        return h
            .properties()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("ID"))
            .map(|(_, v)| v.clone());
    }
    None
}

/// Every reference to `id` across the vault: the defining headline plus
/// each `id:` link, as `(file, zero-based line)` pairs, sorted.
///
/// Link text is id-based, so these survive a [`rename_symbol`].
#[must_use]
pub fn references(vault: &Vault, id: &str) -> Vec<(std::path::PathBuf, u32)> {
    let mut out: Vec<(std::path::PathBuf, u32)> = Vec::new();
    if let Some(def) = definition_of(vault, id) {
        out.push(def);
    }
    for (path, doc) in vault.iter() {
        for (i, line) in doc.source().lines().enumerate() {
            if id_tokens(line).iter().any(|(_, _, tok)| *tok == id) {
                out.push((path.to_path_buf(), u32::try_from(i).unwrap_or(u32::MAX)));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Retitle the headline owning `id` to `new_title`, through the command
/// registry (undoable, I3; persisted). Links are id-based, so every
/// [`references`] entry survives unchanged.
///
/// # Errors
///
/// [`LspError::Vault`] when no headline owns `id` or the write fails.
pub fn rename_symbol(vault: &mut Vault, id: &str, new_title: &str) -> Result<(), LspError> {
    vault
        .rename_headline(&closure_core::BlockId::from_existing(id), new_title)
        .map_err(|e| LspError::Vault(e.to_string()))
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

/// One `#+BEGIN_SRC` block, seen as a document of its own.
///
/// An org file is mostly prose with islands of another language in it.
/// Inside `#+BEGIN_SRC rust` the thing that knows what the cursor is on
/// is rust-analyzer; the thing that knows where line 12 of the block
/// lives in the file is closure. So the block is handed over as its own
/// document and every line number that comes back is shifted home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrcBlock {
    /// What `#+BEGIN_SRC` named — `rust`, `python`, `closure-config`.
    pub language: String,
    /// The file line its first *content* line sits on (0-based), which
    /// is the line after `#+BEGIN_SRC`.
    pub first_line: u32,
    /// The block's content, as the language server should see it.
    pub text: String,
}

impl SrcBlock {
    /// The block containing file line `line`, if any.
    ///
    /// The `#+BEGIN_SRC` and `#+END_SRC` lines are org's, not the
    /// language's: a cursor on them is in the org file, and an answer
    /// from rust-analyzer about them would be an answer about a line
    /// it was never shown.
    #[must_use]
    pub fn at(src: &str, line: u32) -> Option<Self> {
        let mut open: Option<(String, u32)> = None;
        for (i, raw) in src.lines().enumerate() {
            let trimmed = raw.trim_start();
            let lower = trimmed.to_lowercase();
            #[allow(clippy::cast_possible_truncation)]
            let i = i as u32;
            if let Some(rest) = lower.strip_prefix("#+begin_src") {
                let language = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                open = Some((language, i + 1));
            } else if lower.starts_with("#+end_src")
                && let Some((language, first_line)) = open.take()
                && (first_line..i).contains(&line)
            {
                {
                    let text = src
                        .lines()
                        .skip(first_line as usize)
                        .take((i - first_line) as usize)
                        .fold(String::new(), |mut acc, l| {
                            acc.push_str(l);
                            acc.push('\n');
                            acc
                        });
                    return Some(Self {
                        language,
                        first_line,
                        text,
                    });
                }
            }
        }
        None
    }

    /// A file line in the block's own coordinates.
    #[must_use]
    pub const fn to_inner(&self, line: u32) -> u32 {
        line.saturating_sub(self.first_line)
    }

    /// A block line back in the file's coordinates.
    #[must_use]
    pub const fn to_outer(&self, line: u32) -> u32 {
        line + self.first_line
    }

    /// Every `"line":N` in a language server's reply, shifted home.
    ///
    /// Columns are left alone: a block's content is not indented into
    /// the file, so character 4 of the block is character 4 of the
    /// line. Only the JSON key is rewritten, so prose in a hover that
    /// happens to contain the word `line` is untouched.
    #[must_use]
    pub fn shift_home(&self, reply: &str) -> String {
        let mut out = String::with_capacity(reply.len());
        let mut rest = reply;
        while let Some(at) = rest.find("\"line\":") {
            let (before, after) = rest.split_at(at);
            out.push_str(before);
            let digits = after["\"line\":".len()..]
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            if let Ok(n) = digits.parse::<u32>() {
                use std::fmt::Write as _;
                let _ = write!(out, "\"line\":{}", self.to_outer(n));
                let tail = &after["\"line\":".len()..];
                let consumed =
                    "\"line\":".len() + (tail.len() - tail.trim_start().len()) + digits.len();
                rest = &after[consumed..];
            } else {
                // Not a number after all — leave it exactly as it was.
                out.push_str("\"line\":");
                rest = &after["\"line\":".len()..];
            }
        }
        out.push_str(rest);
        out
    }
}

/// A language server closure runs for what is *inside* a src block.
///
/// `lsp rust = rust-analyzer` in config.org, and a cursor inside
/// `#+BEGIN_SRC rust` is answered by rust-analyzer rather than by
/// closure guessing at a language it does not know. The block goes over
/// as a document of its own and every answer comes back through
/// [`SrcBlock::shift_home`].
///
/// The mirror of [`serve`]: closure speaks both halves of this
/// protocol, and the framing is the same one in both directions.
pub struct Embedded {
    language: String,
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
    /// Where the block is written for the server to read. Real
    /// language servers key on the path: rust-analyzer will not answer
    /// about a `closure-block://` URI at all, and several others
    /// decide the language from the extension rather than from
    /// `languageId`. Removed when the server goes.
    scratch: Option<std::path::PathBuf>,
}

/// The file extension a language server expects for `language`.
///
/// Only the languages whose extension differs from their name need
/// listing. Anything else is its own name — right for `org`, `sql`,
/// `lua`, `c`, and the least surprising thing to hand a server for a
/// language nobody here has heard of.
fn extension_for(language: &str) -> &str {
    match language {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "haskell" => "hs",
        "markdown" => "md",
        "shell" | "bash" => "sh",
        "yaml" => "yml",
        other => other,
    }
}

impl Embedded {
    /// Start `command` and shake hands with it.
    ///
    /// Split on whitespace and run with no shell, for the reason a
    /// config file should never reach one.
    ///
    /// # Errors
    ///
    /// [`LspError::Transport`] when the command cannot be started or
    /// does not answer `initialize`.
    pub fn start(language: &str, command: &str) -> Result<Self, LspError> {
        let mut parts = command.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| LspError::Transport(format!("`lsp {language}` has no command")))?;
        let mut child = std::process::Command::new(program)
            .args(parts)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Its own diagnostics are not protocol and must not be
            // read as any.
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| {
                LspError::Transport(format!("{language}: cannot start `{program}`: {e}"))
            })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(LspError::Transport(format!("{language}: no pipes")));
        };
        let mut server = Self {
            language: language.to_owned(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            scratch: None,
        };
        server.request("initialize", "{\"capabilities\":{}}")?;
        server.notify("initialized", "{}")?;
        Ok(server)
    }

    /// Which language this one is for.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The symbols of `block`, in the org file's coordinates.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub fn document_symbols(&mut self, block: &SrcBlock) -> Result<String, LspError> {
        let uri = self.open(block)?;
        let reply = self.request(
            "textDocument/documentSymbol",
            &format!("{{\"textDocument\":{{\"uri\":\"{uri}\"}}}}"),
        )?;
        Ok(block.shift_home(&reply))
    }

    /// What the server says about `line`/`character` of the org file,
    /// asked about the block that line is in.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub fn ask_at(
        &mut self,
        method: &str,
        block: &SrcBlock,
        line: u32,
        character: u32,
    ) -> Result<String, LspError> {
        let uri = self.open(block)?;
        let reply = self.request(
            method,
            &format!(
                "{{\"textDocument\":{{\"uri\":\"{uri}\"}},\
                 \"position\":{{\"line\":{},\"character\":{character}}}}}",
                block.to_inner(line)
            ),
        )?;
        Ok(block.shift_home(&reply))
    }

    /// Hand the block over as a document. The URI is the block's
    /// identity to the server and nothing else — it never names a file
    /// on disk, because the block is not one.
    fn open(&mut self, block: &SrcBlock) -> Result<String, LspError> {
        // A real path, with the extension the language server expects.
        // rust-analyzer will not answer about a URI scheme it does not
        // know, and several servers read the language off the
        // extension rather than off `languageId`.
        let path = std::env::temp_dir().join(format!(
            "closure-block-{}-{}.{}",
            std::process::id(),
            block.first_line,
            extension_for(&block.language)
        ));
        std::fs::write(&path, &block.text).map_err(|e| LspError::Transport(e.to_string()))?;
        let uri = format!("file://{}", path.display());
        self.scratch = Some(path);
        self.notify(
            "textDocument/didOpen",
            &format!(
                "{{\"textDocument\":{{\"uri\":\"{uri}\",\"languageId\":\"{}\",\
                 \"version\":1,\"text\":\"{}\"}}}}",
                block.language,
                closure_jsonrpc::json_escape(&block.text)
            ),
        )?;
        Ok(uri)
    }

    fn request(&mut self, method: &str, params: &str) -> Result<String, LspError> {
        self.next_id += 1;
        let body = closure_jsonrpc::request(self.next_id, method, params);
        write_frame(&mut self.stdin, &body)?;
        std::io::Write::flush(&mut self.stdin).map_err(|e| LspError::Transport(e.to_string()))?;
        loop {
            let Some(msg) = read_frame(&mut self.stdout)? else {
                return Err(LspError::Transport(format!(
                    "{}: closed the pipe without answering {method}",
                    self.language
                )));
            };
            // A server may send notifications (progress, diagnostics)
            // between the request and its answer; the one with our id
            // is the answer.
            if closure_jsonrpc::raw_field(&msg, "id").is_none() {
                continue;
            }
            if let Some(err) = closure_jsonrpc::raw_value(&msg, "error") {
                let said = closure_jsonrpc::string_field(&err, "message").unwrap_or(err);
                return Err(LspError::Transport(format!("{}: {said}", self.language)));
            }
            return closure_jsonrpc::raw_value(&msg, "result").ok_or_else(|| {
                LspError::Transport(format!("{}: no result in {msg}", self.language))
            });
        }
    }

    fn notify(&mut self, method: &str, params: &str) -> Result<(), LspError> {
        let body = closure_jsonrpc::notification(method, params);
        write_frame(&mut self.stdin, &body)?;
        std::io::Write::flush(&mut self.stdin).map_err(|e| LspError::Transport(e.to_string()))
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = self.scratch.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The language servers configured for src blocks, by language.
///
/// Started on first use rather than at startup: a config naming four
/// of them should not spawn four language servers for someone who
/// never puts the cursor in a block. One that will not start is
/// remembered as having failed, so it is not retried on every
/// keystroke.
#[derive(Default)]
pub struct Embeddings {
    configured: Vec<(String, String)>,
    running: std::collections::HashMap<String, Option<Embedded>>,
}

impl Embeddings {
    /// From the `lsp <language> = <command>` lines of config.org.
    #[must_use]
    pub fn from_config(servers: &[(String, String)]) -> Self {
        Self {
            configured: servers.to_vec(),
            running: std::collections::HashMap::new(),
        }
    }

    /// Whether any language has a server.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.configured.is_empty()
    }

    /// Ask the server for `block`'s language about a position in the
    /// org file.
    ///
    /// `None` when no server is configured for that language, which
    /// means "closure answers this one" — the difference between "not
    /// mine" and "mine, and the answer is nothing" is the difference
    /// between falling through and swallowing the request.
    pub fn ask(
        &mut self,
        method: &str,
        block: &SrcBlock,
        line: u32,
        character: u32,
    ) -> Option<Result<String, LspError>> {
        let command = self
            .configured
            .iter()
            .find(|(lang, _)| *lang == block.language)
            .map(|(_, command)| command.clone())?;
        let server = self
            .running
            .entry(block.language.clone())
            .or_insert_with(|| Embedded::start(&block.language, &command).ok());
        let server = server.as_mut()?;
        Some(if method == "textDocument/documentSymbol" {
            server.document_symbols(block)
        } else {
            server.ask_at(method, block, line, character)
        })
    }
}
