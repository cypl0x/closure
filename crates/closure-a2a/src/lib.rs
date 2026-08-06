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

/// Lifecycle state of a delegated A2A task (V8b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Posted, not yet executing.
    Submitted,
    /// Executing.
    Working,
    /// Finished successfully.
    Done,
    /// Finished with an error.
    Failed,
}

impl TaskState {
    /// The lowercase wire token (`submitted`/`working`/`done`/`failed`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Working => "working",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// One thing closure will do for another agent.
///
/// The card used to advertise `task/delegate` as its single skill,
/// which names the *transport*: an agent that read it learned it could
/// delegate a task and not one thing about what a task may be. These
/// are the tools, which is what it was asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skill {
    /// The word to send as the task line.
    pub id: &'static str,
    /// A short human name.
    pub name: &'static str,
    /// What it does, in a sentence.
    pub description: &'static str,
}

/// Every skill the card advertises.
///
/// One list, and a test drives each of them through the delegate path —
/// a skill another machine is promised and cannot get is worse than one
/// that was never offered.
pub const SKILLS: &[Skill] = &[
    Skill {
        id: "view-state",
        name: "Vault overview",
        description: "How many files and headlines there are, and which TODO keywords and tags are in use",
    },
    Skill {
        id: "list-files",
        name: "List files",
        description: "Every org file in the vault, by path",
    },
    Skill {
        id: "read",
        name: "Read a file",
        description: "The full org source of one file, given its path",
    },
    Skill {
        id: "search",
        name: "Search",
        description: "Headlines and body lines matching a phrase, with the file each is in",
    },
    Skill {
        id: "capture",
        name: "Capture a note",
        description: "File a new headline with the given title into the capture target",
    },
    Skill {
        id: "rename",
        name: "Rename a headline",
        description: "Give the headline with this id a new title",
    },
    Skill {
        id: "set-property",
        name: "Set a property",
        description: "Write a key and value into a headline's property drawer",
    },
];

/// A delegated task + its lifecycle state, so a caller can poll progress
/// (V8b).
///
/// Created [`Submitted`](TaskState::Submitted); [`run`](Self::run) drives
/// it through `Working` to `Done`/`Failed` (a tool result starting
/// `ERROR` is a failure).
#[derive(Debug, Clone)]
pub struct Task {
    /// The task line (a `Vault::run_tool` command).
    pub command: String,
    /// Current lifecycle state.
    pub state: TaskState,
    /// Result text once run.
    pub result: String,
}

impl Task {
    /// Post a task in the `Submitted` state.
    #[must_use]
    pub fn submit(command: &str) -> Self {
        Self {
            command: command.to_owned(),
            state: TaskState::Submitted,
            result: String::new(),
        }
    }

    /// Execute the task against `vault` (I8), transitioning `Working` →
    /// `Done`/`Failed`. Idempotent re-runs re-execute the command.
    pub fn run(&mut self, vault: &mut Vault) {
        self.state = TaskState::Working;
        self.result = delegate_task(vault, &self.command);
        self.state = if self.result.starts_with("ERROR") || self.result.starts_with("error") {
            TaskState::Failed
        } else {
            TaskState::Done
        };
    }
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
    use closure_jsonrpc::{json_escape, string_field};
    let id = closure_jsonrpc::raw_field(json, "id")?;
    let method = string_field(json, "method").unwrap_or_default();
    let result = match method.as_str() {
        "initialize" => "{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tasks\":{}},\
             \"serverInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}"
            .to_owned(),
        "agent/card" => {
            let skills: Vec<String> = SKILLS
                .iter()
                .map(|s| {
                    format!(
                        "{{\"id\":\"{}\",\"name\":\"{}\",\"description\":\"{}\"}}",
                        s.id,
                        json_escape(s.name),
                        json_escape(s.description)
                    )
                })
                .collect();
            format!(
                "{{\"name\":\"closure\",\"description\":\"A local-first plain-text \
                 knowledge base over org files. Tasks are delegated as one tool \
                 name plus its argument.\",\"version\":\"0.0.0\",\
                 \"capabilities\":{{\"tasks\":{{}}}},\"skills\":[{}]}}",
                skills.join(",")
            )
        }
        "task/delegate" => {
            let task_line = string_field(json, "task").unwrap_or_default();
            let mut task = Task::submit(&task_line);
            task.run(vault);
            format!(
                "{{\"state\":\"{}\",\"text\":\"{}\"}}",
                task.state.as_str(),
                json_escape(&task.result)
            )
        }
        _ => return Some(closure_jsonrpc::method_not_found(&id)),
    };
    Some(closure_jsonrpc::response(&id, &result))
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
