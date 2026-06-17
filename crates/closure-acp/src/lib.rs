//! ACP (Agent Communication Protocol) bridge.
//!
//! Allows external agents to drive the command registry over a text
//! protocol. The on-the-wire format mirrors [`closure_mcp`]: one
//! `<command-name> [args...]` line per request, `OK <name>` /
//! `UNKNOWN <name>` per response. JSON-RPC framing lands behind a
//! feature flag once a JSON dependency is picked.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader};

use closure_core::Registry;
use closure_store::Vault;
use thiserror::Error;

/// ACP bridge error.
#[derive(Debug, Error)]
pub enum AcpError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
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

/// Run the dispatcher loop.
pub fn run<R: BufRead, W: std::io::Write>(
    registry: &Registry,
    mut input: R,
    output: &mut W,
) -> Result<(), AcpError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| AcpError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        match resolve_line(registry, &line) {
            Outcome::Found(name) => {
                writeln!(output, "OK {name}").map_err(|e| AcpError::Transport(e.to_string()))?;
            }
            Outcome::Unknown(name) => {
                writeln!(output, "UNKNOWN {name}")
                    .map_err(|e| AcpError::Transport(e.to_string()))?;
            }
            Outcome::Skip => {}
        }
    }
    Ok(())
}

/// Wrap stdin/stdout for the typical CLI invocation.
pub fn run_stdio(registry: &Registry) -> Result<(), AcpError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    run(registry, reader, &mut stdout)
}

/// Agent tools exposed for the card (mirrors the MCP tool surface so ACP
/// serves the same capabilities for agent discovery). Schemas are minimal
/// object for now (args string); matches the JSON subset used by MCP.
const AGENT_TOOLS: &[(&str, &str)] = &[
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

/// Run the ACP JSON-RPC server over a reader/writer.
///
/// One request per line; one response line per request that carries an
/// `id` (notifications get none). All mutations route through
/// [`closure_store::Vault::run_tool`] (I8). Mirrors
/// [`closure_mcp::serve_jsonrpc`].
///
/// # Errors
///
/// [`AcpError::Transport`] on IO failure.
pub fn serve_jsonrpc<R: BufRead, W: std::io::Write>(
    vault: &mut Vault,
    mut input: R,
    output: &mut W,
) -> Result<(), AcpError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| AcpError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_message(vault, &line) {
            writeln!(output, "{resp}").map_err(|e| AcpError::Transport(e.to_string()))?;
        }
    }
    Ok(())
}

/// Run the ACP JSON-RPC server on stdio against `vault`.
///
/// # Errors
///
/// [`AcpError::Transport`] on IO failure.
pub fn serve_jsonrpc_stdio(vault: &mut Vault) -> Result<(), AcpError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    serve_jsonrpc(vault, reader, &mut stdout)
}

/// Raw token after `"key":` — number, string (with quotes), etc.
/// (duplicated from mcp json helpers; lean, no serde, per I10/hermetic).
fn raw_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = json.find(&needle)?;
    let rest = json[at + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        return string_value(rest).map(|s| format!("\"{}\"", json_escape(&s)));
    }
    let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
    let tok = rest[..end].trim();
    if tok.is_empty() {
        None
    } else {
        Some(tok.to_owned())
    }
}

/// Unescaped string value after `"key":`.
fn string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = json.find(&needle)?;
    let rest = json[at + needle.len()..].trim_start();
    string_value(rest.strip_prefix(':')?.trim_start())
}

/// Parse a JSON string literal at the start of `s`, unescaping.
fn string_value(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

/// Escape a string for embedding in a JSON literal.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Handle one JSON-RPC message against a vault using the MCP JSON subset
/// extended for ACP agent card discovery.
///
/// Supported (over the subset):
/// - initialize (compat with MCP clients)
/// - agent/card — returns {"name":"closure", "tools":[... with inputSchema ...]}
/// - tools/list , tools/call (full subset passthrough to `Vault::run_tool` per I8)
/// - notifications (no response)
///
/// Everything else -> -32601 method not found.
/// This delivers the ROADMAP item: agent card (name, tools, schemas) served
/// over the MCP JSON subset.
#[must_use]
pub fn handle_message(vault: &mut Vault, json: &str) -> Option<String> {
    let id = raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => "{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
            .to_owned(),
        "agent/card" => {
            let tools: Vec<String> = AGENT_TOOLS
                .iter()
                .map(|(name, desc)| {
                    format!(
                        "{{\"name\":\"{name}\",\"description\":\"{desc}\",\
                         \"inputSchema\":{{\"type\":\"object\",\"properties\":\
                         {{\"args\":{{\"type\":\"string\"}}}}}}}}"
                    )
                })
                .collect();
            format!(
                "{{\"name\":\"closure\",\"version\":\"0.0.0\",\"tools\":[{}]}}",
                tools.join(",")
            )
        }
        "tools/list" => {
            let tools: Vec<String> = AGENT_TOOLS
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
            format!(
                "{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}",
                json_escape(&text)
            )
        }
        _ => {
            return Some(format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":\
                 {{\"code\":-32601,\"message\":\"method not found\"}}}}"
            ));
        }
    };
    Some(format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}"
    ))
}
