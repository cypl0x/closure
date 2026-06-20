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
fn write_frame<W: std::io::Write>(out: &mut W, body: &str) -> Result<(), LspError> {
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())
        .map_err(|e| LspError::Transport(e.to_string()))
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
    mut input: R,
    output: &mut W,
) -> Result<(), LspError> {
    while let Some(msg) = read_frame(&mut input)? {
        if let Some(resp) = handle_message_mut(vault, &msg) {
            write_frame(output, &resp)?;
        }
        if closure_jsonrpc::string_field(&msg, "method").as_deref() == Some("exit") {
            break;
        }
    }
    Ok(())
}

/// The capabilities `initialize` advertises.
const INITIALIZE_RESULT: &str = "{\"capabilities\":{\"documentSymbolProvider\":true,\
     \"hoverProvider\":true,\
     \"completionProvider\":{\"triggerCharacters\":[\":\"]},\
     \"diagnosticProvider\":{\"interFileDependencies\":true,\
     \"workspaceDiagnostics\":false},\
     \"referencesProvider\":true,\"renameProvider\":true},\
     \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}";

/// The source text of the document the request's `uri` names (relative
/// to the vault root); empty when absent.
fn req_source(vault: &Vault, json: &str) -> String {
    let uri = closure_jsonrpc::string_field(json, "uri").unwrap_or_default();
    let rel = uri.strip_prefix("file://").unwrap_or(&uri);
    vault
        .document_relative(std::path::Path::new(rel))
        .map(closure_core::Document::source)
        .unwrap_or_default()
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
fn hover_result(vault: &Vault, json: &str) -> String {
    let src = req_source(vault, json);
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
fn completion_result(vault: &Vault, json: &str) -> String {
    let src = req_source(vault, json);
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
fn diagnostic_result(vault: &Vault, json: &str) -> String {
    let src = req_source(vault, json);
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
fn references_result(vault: &Vault, json: &str) -> String {
    let src = req_source(vault, json);
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
fn rename_result(vault: &mut Vault, json: &str) -> String {
    let src = req_source(vault, json);
    let (line, ch) = req_position(json);
    let new_name = closure_jsonrpc::string_field(json, "newName").unwrap_or_default();
    if let Some(id) = id_at_position(&src, line, ch) {
        let _ = rename_symbol(vault, &id, &new_name);
    }
    "null".to_owned()
}

/// `textDocument/documentSymbol` result fragment.
fn symbol_result(vault: &Vault, json: &str) -> String {
    let src = req_source(vault, json);
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
    let id = closure_jsonrpc::raw_field(json, "id")?;
    let method = closure_jsonrpc::string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => INITIALIZE_RESULT.to_owned(),
        "shutdown" => "null".to_owned(),
        "textDocument/hover" => hover_result(vault, json),
        "textDocument/completion" => completion_result(vault, json),
        "textDocument/diagnostic" => diagnostic_result(vault, json),
        "textDocument/documentSymbol" => symbol_result(vault, json),
        "textDocument/references" => references_result(vault, json),
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
    let method = closure_jsonrpc::string_field(json, "method").unwrap_or_default();
    if method == "textDocument/rename" {
        let id = closure_jsonrpc::raw_field(json, "id")?;
        let result = rename_result(vault, json);
        return Some(closure_jsonrpc::response(&id, &result));
    }
    handle_message(vault, json)
}

/// Run the LSP server on stdio against `vault`.
///
/// # Errors
///
/// [`LspError::Transport`] on IO failure.
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

    // closure-widget expansion errors (unknown / cyclic refs), resolved
    // against widget definitions across the whole vault (V2b).
    if let Err(e) =
        closure_query::expand_widgets_with(src, &closure_query::vault_widget_defs(vault))
    {
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
        out.push(Diagnostic {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            start_char: 0,
            end_char: end,
            severity: Severity::Error,
            code: DiagnosticCode::Widget,
            message: e.to_string(),
        });
    }

    out
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
