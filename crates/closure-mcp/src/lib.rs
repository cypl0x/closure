//! MCP (Model Context Protocol) bridge.
//!
//! Exposes the command registry as MCP tools. External MCP-speaking
//! clients (agents, IDEs) invoke commands through this bridge only —
//! never reaching the Document directly (I8).
//!
//! M7 skeleton: a text-mode stdio dispatcher that accepts one
//! `<command-name> [args...]` per line. The full JSON-RPC framing
//! lands behind a feature flag once a JSON dependency is picked.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader};

use closure_core::Registry;
use thiserror::Error;

/// MCP bridge error.
#[derive(Debug, Error)]
pub enum McpError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
}

/// Result of looking up a command name in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The named command exists.
    Found(String),
    /// No command matches the name.
    Unknown(String),
    /// The line was blank or a comment.
    Skip,
    /// Caller asked for a registry listing (`LIST`).
    List,
}

/// Resolve a single text-protocol line.
///
/// The line may be `<name>` or `<name> args...`; the name is matched
/// against `registry`. Returns the matched command name when found,
/// the requested name when not, and [`DispatchOutcome::Skip`] for
/// blank lines / `# comments`.
#[must_use]
pub fn resolve_line(registry: &Registry, line: &str) -> DispatchOutcome {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return DispatchOutcome::Skip;
    }
    let name = trimmed.split_whitespace().next().unwrap_or("");
    if name == "LIST" {
        return DispatchOutcome::List;
    }
    if registry.get(name).is_some() {
        DispatchOutcome::Found(name.to_owned())
    } else {
        DispatchOutcome::Unknown(name.to_owned())
    }
}

/// Run the stdio dispatcher loop.
///
/// Reads from `input`, writes outcomes to `output`. Each line of
/// input produces one line of output: `OK <name>`, `UNKNOWN <name>`,
/// or nothing for blank / comment lines.
pub fn run<R: BufRead, W: std::io::Write>(
    registry: &Registry,
    mut input: R,
    output: &mut W,
) -> Result<(), McpError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| McpError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        match resolve_line(registry, &line) {
            DispatchOutcome::Found(name) => {
                writeln!(output, "OK {name}").map_err(|e| McpError::Transport(e.to_string()))?;
            }
            DispatchOutcome::Unknown(name) => {
                writeln!(output, "UNKNOWN {name}")
                    .map_err(|e| McpError::Transport(e.to_string()))?;
            }
            DispatchOutcome::Skip => {}
            DispatchOutcome::List => {
                let mut names: Vec<&str> = registry.names().collect();
                names.sort_unstable();
                for n in names {
                    writeln!(output, "{n}").map_err(|e| McpError::Transport(e.to_string()))?;
                }
            }
        }
    }
    Ok(())
}

/// Wrap stdin/stdout for the typical CLI invocation.
pub fn run_stdio(registry: &Registry) -> Result<(), McpError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    run(registry, reader, &mut stdout)
}

/// The vault tools exposed over MCP, mirroring [`Vault::run_tool`].
const TOOLS: &[(&str, &str)] = &[
    ("list-files", "List every org file in the vault"),
    ("read", "Read one file's org source: read <file>"),
    ("search", "Search headline titles: search <text>"),
    (
        "capture",
        "Append a TODO entry to inbox.org: capture <title>",
    ),
    ("rename", "Rename a headline: rename <id> <title>"),
    (
        "set-property",
        "Set a property: set-property <id> <key> <value>",
    ),
];

/// MCP prompts exposed by `prompts/list` / `prompts/get` (V8a): closure's
/// capture + ask templates as reusable prompt entries.
const PROMPTS: &[(&str, &str)] = &[
    (
        "capture",
        "Capture a new TODO into the inbox. Provide a concise title.",
    ),
    (
        "ask",
        "Ask about the vault. The agent may use the list/read/search tools.",
    ),
];

/// Handle one MCP JSON-RPC message against a vault.
///
/// Returns the response line, or `None` for notifications. Supported:
/// `initialize`, `tools/list`, `tools/call`; everything else is a
/// `-32601` error. Mutations go through [`Vault::run_tool`] (I8).
#[must_use]
pub fn handle_message(vault: &mut closure_store::Vault, json: &str) -> Option<String> {
    use closure_jsonrpc::{json_escape, string_field};
    let id = closure_jsonrpc::raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => "{\"protocolVersion\":\"2024-11-05\",\
             \"capabilities\":{\"tools\":{},\"resources\":{},\"prompts\":{}},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
            .to_owned(),
        "resources/list" => {
            let items: Vec<String> = vault
                .paths()
                .iter()
                .map(|p| {
                    let disp = p.display().to_string();
                    let name = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&disp)
                        .to_owned();
                    format!(
                        "{{\"uri\":\"file://{}\",\"name\":\"{}\",\"mimeType\":\"text/x-org\"}}",
                        json_escape(&disp),
                        json_escape(&name)
                    )
                })
                .collect();
            format!("{{\"resources\":[{}]}}", items.join(","))
        }
        "resources/read" => {
            let uri = string_field(json, "uri").unwrap_or_default();
            // Both, in this order, because the listing hands out
            // `file://` + the *absolute* path and this only tried the
            // relative lookup — so every uri a client could have got
            // from `resources/list` read back as an empty file. A
            // silent empty answer, which is the worst kind: the model
            // is told the note exists and is blank.
            let path = uri
                .strip_prefix("file://")
                .or_else(|| uri.strip_prefix("closure://"))
                .unwrap_or(&uri);
            let path = std::path::Path::new(path);
            let text = vault
                .document(path)
                .or_else(|| vault.document_relative(path))
                .map(closure_core::Document::source)
                .unwrap_or_default();
            format!(
                "{{\"contents\":[{{\"uri\":\"{}\",\"mimeType\":\"text/x-org\",\"text\":\"{}\"}}]}}",
                json_escape(&uri),
                json_escape(&text)
            )
        }
        // Every client's "are you still there". Answering it is an
        // empty object; not answering it is a server that looks dead.
        "ping" => "{}".to_owned(),
        "prompts/list" => {
            let prompts: Vec<String> = PROMPTS
                .iter()
                .map(|(name, desc)| format!("{{\"name\":\"{name}\",\"description\":\"{desc}\"}}"))
                .collect();
            format!("{{\"prompts\":[{}]}}", prompts.join(","))
        }
        "prompts/get" => {
            let name = string_field(json, "name").unwrap_or_default();
            let text = PROMPTS
                .iter()
                .find(|(n, _)| *n == name)
                .map_or("unknown prompt", |(_, desc)| desc);
            format!(
                "{{\"messages\":[{{\"role\":\"user\",\"content\":\
                 {{\"type\":\"text\",\"text\":\"{}\"}}}}]}}",
                json_escape(text)
            )
        }
        "tools/list" => {
            let tools: Vec<String> = TOOLS
                .iter()
                .map(|(name, desc)| {
                    format!(
                        "{{\"name\":\"{name}\",\"description\":\"{desc}\",\
                         \"inputSchema\":{{\"type\":\"object\",\"properties\":\
                         {{\"args\":{{\"type\":\"string\"}}}}}}}}"
                    )
                })
                .collect();
            format!("{{\"tools\":[{}]}}", tools.join(","))
        }
        "tools/call" => {
            let name = string_field(json, "name").unwrap_or_default();
            let args = string_field(json, "args").unwrap_or_default();
            let line = if args.is_empty() {
                name
            } else {
                format!("{name} {args}")
            };
            let text = vault.run_tool(&line);
            // MCP has a place to say "this went wrong", and without it
            // a client hands the failure to the model as an answer —
            // so "ERROR no such file" becomes something the model
            // believes about your vault.
            let failed = text.starts_with("ERROR");
            format!(
                "{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}],\"isError\":{failed}}}",
                json_escape(&text)
            )
        }
        _ => return Some(closure_jsonrpc::method_not_found(&id)),
    };
    Some(closure_jsonrpc::response(&id, &result))
}

/// Run the JSON-RPC MCP server over a reader/writer.
///
/// One request per line; one response line per request that carries an
/// `id` (notifications get none). Mutations route through
/// [`closure_store::Vault::run_tool`] (I8).
///
/// # Errors
///
/// [`McpError::Transport`] on IO failure.
pub fn serve_jsonrpc<R: BufRead, W: std::io::Write>(
    vault: &mut closure_store::Vault,
    mut input: R,
    output: &mut W,
) -> Result<(), McpError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| McpError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_message(vault, &line) {
            writeln!(output, "{resp}").map_err(|e| McpError::Transport(e.to_string()))?;
        }
    }
    Ok(())
}

/// Run the JSON-RPC MCP server on stdio against `vault`.
///
/// # Errors
///
/// [`McpError::Transport`] on IO failure.
pub fn serve_jsonrpc_stdio(vault: &mut closure_store::Vault) -> Result<(), McpError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    serve_jsonrpc(vault, reader, &mut stdout)
}
