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
    ("capture", "Append a TODO entry to inbox.org: capture <title>"),
    ("rename", "Rename a headline: rename <id> <title>"),
    ("set-property", "Set a property: set-property <id> <key> <value>"),
];

/// Handle one MCP JSON-RPC message against a vault.
///
/// Returns the response line, or `None` for notifications. Supported:
/// `initialize`, `tools/list`, `tools/call`; everything else is a
/// `-32601` error. Mutations go through [`Vault::run_tool`] (I8).
#[must_use]
pub fn handle_message(vault: &mut closure_store::Vault, json: &str) -> Option<String> {
    let id = raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => {
            "{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
                .to_owned()
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

/// Raw token after `"key":` — number, string (with quotes), etc.
fn raw_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = json.find(&needle)?;
    let rest = json[at + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        return string_value(rest).map(|s| format!("\"{}\"", json_escape(&s)));
    }
    let end = rest
        .find([',', '}', ']'])
        .unwrap_or(rest.len());
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
