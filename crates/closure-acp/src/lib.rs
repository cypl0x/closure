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
/// Capabilities this agent advertises for negotiation (V8b). A client
/// proposes a set; `agent/negotiate` returns the intersection.
const CAPABILITIES: &[&str] = &["tools", "tools-call", "resources", "capability-negotiation"];

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
    use closure_jsonrpc::{json_escape, string_field};
    let id = closure_jsonrpc::raw_field(json, "id")?;
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
            let caps: Vec<String> = CAPABILITIES.iter().map(|c| format!("\"{c}\"")).collect();
            format!(
                "{{\"name\":\"closure\",\"version\":\"0.0.0\",\"capabilities\":[{}],\"tools\":[{}]}}",
                caps.join(","),
                tools.join(",")
            )
        }
        "agent/negotiate" => {
            // The client proposes capabilities (whitespace/comma list);
            // return the intersection this agent actually supports (V8b).
            let requested = string_field(json, "capabilities").unwrap_or_default();
            let agreed: Vec<String> = requested
                .split([',', ' '])
                .map(str::trim)
                .filter(|c| !c.is_empty() && CAPABILITIES.contains(c))
                .map(|c| format!("\"{c}\""))
                .collect();
            format!("{{\"agreed\":[{}]}}", agreed.join(","))
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
        _ => return Some(closure_jsonrpc::method_not_found(&id)),
    };
    Some(closure_jsonrpc::response(&id, &result))
}
