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
    /// Backend exceeded the per-call timeout and was killed.
    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),
}

/// A language backend.
pub trait Backend {
    /// Human-readable identifier (matches the org language field).
    fn language(&self) -> &str;
    /// Execute `src` and return captured output.
    fn eval(&self, src: &str) -> Result<Output, EvalError>;
    /// Execute `src` with a wall-clock timeout. The default
    /// implementation falls back to [`Self::eval`] when the backend
    /// doesn't support timeouts; concrete backends override.
    fn eval_with_timeout(
        &self,
        src: &str,
        _timeout: std::time::Duration,
    ) -> Result<Output, EvalError> {
        self.eval(src)
    }
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
        run_via_stdin("/bin/sh", &[], src, None)
    }

    fn eval_with_timeout(
        &self,
        src: &str,
        timeout: std::time::Duration,
    ) -> Result<Output, EvalError> {
        run_via_stdin("/bin/sh", &[], src, Some(timeout))
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
        run_via_stdin("python3", &[], src, None)
    }

    fn eval_with_timeout(
        &self,
        src: &str,
        timeout: std::time::Duration,
    ) -> Result<Output, EvalError> {
        run_via_stdin("python3", &[], src, Some(timeout))
    }
}

/// Node.js backend: runs the source through `node` via stdin.
#[derive(Debug, Default, Clone, Copy)]
pub struct NodeBackend;

impl Backend for NodeBackend {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "javascript"
    }

    fn eval(&self, src: &str) -> Result<Output, EvalError> {
        run_via_stdin("node", &[], src, None)
    }

    fn eval_with_timeout(
        &self,
        src: &str,
        timeout: std::time::Duration,
    ) -> Result<Output, EvalError> {
        run_via_stdin("node", &[], src, Some(timeout))
    }
}

/// Ruby backend: runs the source through `ruby` via stdin.
#[derive(Debug, Default, Clone, Copy)]
pub struct RubyBackend;

impl Backend for RubyBackend {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "ruby"
    }

    fn eval(&self, src: &str) -> Result<Output, EvalError> {
        run_via_stdin("ruby", &[], src, None)
    }

    fn eval_with_timeout(
        &self,
        src: &str,
        timeout: std::time::Duration,
    ) -> Result<Output, EvalError> {
        run_via_stdin("ruby", &[], src, Some(timeout))
    }
}

/// Pick a backend for a language identifier (case-insensitive). Returns
/// `None` if no backend is registered for the language.
#[must_use]
pub fn backend_for(lang: &str) -> Option<Box<dyn Backend>> {
    match lang.to_ascii_lowercase().as_str() {
        "shell" | "sh" | "bash" => Some(Box::new(ShellBackend)),
        "python" | "py" => Some(Box::new(PythonBackend)),
        "javascript" | "js" | "node" => Some(Box::new(NodeBackend)),
        "ruby" | "rb" => Some(Box::new(RubyBackend)),
        _ => None,
    }
}

/// In-memory result cache keyed by `(language, source)`. Avoids
/// re-running unchanged code blocks across successive evaluations.
#[derive(Debug, Default)]
pub struct EvalCache {
    entries: std::collections::HashMap<(String, String), Output>,
}

impl EvalCache {
    /// Fresh empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of cached results.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Run `backend.eval(src)` if the result isn't cached, otherwise
    /// return the cached output. The cache is keyed by
    /// `(backend.language(), src)`.
    pub fn eval_cached(&mut self, backend: &dyn Backend, src: &str) -> Result<Output, EvalError> {
        let key = (backend.language().to_owned(), src.to_owned());
        if let Some(out) = self.entries.get(&key) {
            return Ok(out.clone());
        }
        let out = backend.eval(src)?;
        self.entries.insert(key, out.clone());
        Ok(out)
    }
}

fn run_via_stdin(
    prog: &str,
    args: &[&str],
    src: &str,
    timeout: Option<std::time::Duration>,
) -> Result<Output, EvalError> {
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

    if let Some(t) = timeout {
        let deadline = std::time::Instant::now() + t;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(EvalError::Timeout(t));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(EvalError::Io(e.to_string())),
            }
        }
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
