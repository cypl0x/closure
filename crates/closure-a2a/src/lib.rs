//! A2A (Agent-to-Agent) bridge.
//!
//! Lets closure participate in agent swarms as a first-class peer.
//! Same text-mode protocol as [`closure_mcp`] / [`closure_acp`]: one
//! command name per request, `OK` or `UNKNOWN` per response.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::io::{BufRead, BufReader};

use closure_core::{BlockId, Registry};
use closure_store::Vault;
use thiserror::Error;

/// A2A bridge error.
#[derive(Debug, Error)]
pub enum A2aError {
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
) -> Result<(), A2aError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| A2aError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        match resolve_line(registry, &line) {
            Outcome::Found(name) => {
                writeln!(output, "OK {name}").map_err(|e| A2aError::Transport(e.to_string()))?;
            }
            Outcome::Unknown(name) => {
                writeln!(output, "UNKNOWN {name}")
                    .map_err(|e| A2aError::Transport(e.to_string()))?;
            }
            Outcome::Skip => {}
        }
    }
    Ok(())
}

/// Wrap stdin/stdout for the typical CLI invocation.
pub fn run_stdio(registry: &Registry) -> Result<(), A2aError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    run(registry, reader, &mut stdout)
}

/// Delegate and execute a task line against the target vault.
///
/// Execution goes exclusively through `Vault::run_tool` (dispatches to
/// registered commands only, per I8). Used for A2A round-trip: agent A
/// posts the task string; agent B (separate vault) calls this, mutates,
/// and returns the result text to the caller.
#[must_use]
pub fn delegate_task(vault: &mut Vault, task: &str) -> String {
    vault.run_tool(task)
}

/// Handle one A2A JSON-RPC message against a vault.
///
/// Returns the response line, or `None` for notifications (no `id`).
/// Supported: `initialize`, `agent/card`, and `task/delegate` (its
/// `task` string routed through [`delegate_task`] → `Vault::run_tool`,
/// I8); everything else is a `-32601` error. Uses the same lean,
/// serde-free JSON helpers as `closure_mcp`/`closure_acp` (triplicated
/// for now — a shared extraction is a later cleanup).
#[must_use]
pub fn handle_message(vault: &mut Vault, json: &str) -> Option<String> {
    let id = raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => "{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tasks\":{}},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
            .to_owned(),
        "agent/card" => {
            "{\"name\":\"closure\",\"version\":\"0.0.0\",\"skills\":[\"task/delegate\"]}".to_owned()
        }
        "task/delegate" => {
            let task = string_field(json, "task").unwrap_or_default();
            let text = delegate_task(vault, &task);
            format!("{{\"text\":\"{}\"}}", json_escape(&text))
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

/// Run the A2A JSON-RPC server over a reader/writer (one request per
/// line; one response per request with an `id`). Mirrors
/// [`closure_mcp`]'s serve loop.
///
/// # Errors
///
/// [`A2aError::Transport`] on IO failure.
pub fn serve_jsonrpc<R: BufRead, W: std::io::Write>(
    vault: &mut Vault,
    mut input: R,
    output: &mut W,
) -> Result<(), A2aError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input
            .read_line(&mut line)
            .map_err(|e| A2aError::Transport(e.to_string()))?;
        if n == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_message(vault, &line) {
            writeln!(output, "{resp}").map_err(|e| A2aError::Transport(e.to_string()))?;
        }
    }
    Ok(())
}

/// Run the A2A JSON-RPC server on stdio against `vault`.
///
/// # Errors
///
/// [`A2aError::Transport`] on IO failure.
pub fn serve_jsonrpc_stdio(vault: &mut Vault) -> Result<(), A2aError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    serve_jsonrpc(vault, reader, &mut stdout)
}

/// Raw token after `"key":` (serde-free JSON helper; see module note).
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

/// Run a simulated swarm of N workers that drain a task queue (represented
/// by stable `task_ids` of TODO headlines in the vault's org files).
///
/// Each worker "picks" an unclaimed task (exactly-once via internal claimed
/// set), executes a side-effect via `delegate_task` (e.g. proof capture),
/// and records state via set-property (so the queue can be inspected).
/// Returns the number of tasks drained.
///
/// Property (enforced by callers/tests): for any input set of task ids,
/// the number drained == |input|, and every task is processed exactly once
/// (no duplicates, no losses). N is advisory (sim is sequential for hermetic
/// test; real swarm uses threads + locking or CRDT claim).
#[must_use]
pub fn swarm_drain(vault: &mut Vault, _num_workers: usize, task_ids: &[BlockId]) -> usize {
    let mut done = 0usize;
    let mut claimed: HashSet<String> = HashSet::new();
    for id in task_ids {
        if claimed.insert(id.to_string()) {
            // execute side-effect (delegation surface); proof capture in vault
            let _ = delegate_task(vault, &format!("capture swarm-proof-{id}"));
            // mark state on the task (available tool; real impl could SetTodo DONE)
            let _ = delegate_task(vault, &format!("set-property {id} swarm-state done"));
            done += 1;
        }
    }
    done
}
