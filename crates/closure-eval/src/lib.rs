//! Sandboxed evaluator for org code blocks.
//!
//! This crate hosts language backends. Each backend accepts a code
//! string plus its header args and returns a stdout/stderr/exit-code
//! tuple. Execution is opt-in: callers pick a backend explicitly and
//! no implicit execution happens during parse or save (I8: evaluation
//! goes through the command registry).
//!
//! M5 ships the `shell` backend. Python, rust-script, and wasm
//! backends live behind their own crates/features in later
//! milestones.

#![forbid(unsafe_code)]

use std::process::{Command, Stdio};

use thiserror::Error;

/// Evaluation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// stdout.
    pub stdout: String,
    /// stderr.
    pub stderr: String,
    /// exit code. 0 = success.
    pub exit: i32,
}

/// Evaluation error.
#[derive(Debug, Error)]
pub enum EvalError {
    /// Spawning the interpreter failed.
    #[error("spawn: {0}")]
    Spawn(String),
    /// Writing the program to the interpreter's stdin failed.
    #[error("io: {0}")]
    Io(String),
}

/// A language backend.
pub trait Backend {
    /// Human-readable identifier (matches the org language field).
    fn language(&self) -> &str;
    /// Execute `src` and return captured output.
    fn eval(&self, src: &str) -> Result<Output, EvalError>;
}

/// Shell backend: pipes the source into `/bin/sh` on stdin.
#[derive(Debug, Default, Clone, Copy)]
pub struct ShellBackend;

impl Backend for ShellBackend {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "shell"
    }

    fn eval(&self, src: &str) -> Result<Output, EvalError> {
        run_via_stdin("/bin/sh", &[], src)
    }
}

/// Python backend: runs the source through `python3` via stdin.
#[derive(Debug, Default, Clone, Copy)]
pub struct PythonBackend;

impl Backend for PythonBackend {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "python"
    }

    fn eval(&self, src: &str) -> Result<Output, EvalError> {
        run_via_stdin("python3", &[], src)
    }
}

/// Pick a backend for a language identifier (case-insensitive). Returns
/// `None` if no backend is registered for the language.
#[must_use]
pub fn backend_for(lang: &str) -> Option<Box<dyn Backend>> {
    match lang.to_ascii_lowercase().as_str() {
        "shell" | "sh" | "bash" => Some(Box::new(ShellBackend)),
        "python" | "py" => Some(Box::new(PythonBackend)),
        _ => None,
    }
}

fn run_via_stdin(prog: &str, args: &[&str], src: &str) -> Result<Output, EvalError> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EvalError::Spawn(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin
            .write_all(src.as_bytes())
            .map_err(|e| EvalError::Io(e.to_string()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| EvalError::Io(e.to_string()))?;
    Ok(Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        exit: out.status.code().unwrap_or(-1),
    })
}
