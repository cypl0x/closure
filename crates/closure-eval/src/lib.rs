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

use std::fmt::Write as _;
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
    /// Execute `src` under resource [`Bounds`] (wall-clock deadline +
    /// output cap). The default falls back to [`Self::eval`] (no
    /// bounds); interpreter backends override to enforce them (C1b).
    fn eval_bounded(&self, src: &str, _bounds: Bounds) -> Result<Output, EvalError> {
        self.eval(src)
    }
}

/// Resource limits applied to a bounded evaluation (C1b).
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    /// Wall-clock deadline; the child is killed and [`EvalError::Timeout`]
    /// returned if it has not exited by then.
    pub timeout: std::time::Duration,
    /// Maximum bytes retained from stdout (and, independently, stderr).
    /// Beyond this the child is killed and the captured output truncated.
    pub max_output: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            timeout: std::time::Duration::from_secs(10),
            max_output: 10 * 1024 * 1024,
        }
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

    fn eval_bounded(&self, src: &str, bounds: Bounds) -> Result<Output, EvalError> {
        run_bounded("/bin/sh", &[], src, bounds)
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

    fn eval_bounded(&self, src: &str, bounds: Bounds) -> Result<Output, EvalError> {
        run_bounded("python3", &[], src, bounds)
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

    fn eval_bounded(&self, src: &str, bounds: Bounds) -> Result<Output, EvalError> {
        run_bounded("node", &[], src, bounds)
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

    fn eval_bounded(&self, src: &str, bounds: Bounds) -> Result<Output, EvalError> {
        run_bounded("ruby", &[], src, bounds)
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

/// Canonical language name for a (case-insensitive) identifier or alias.
///
/// `sh`/`bash` → `shell`, `py` → `python`, `js`/`node` → `javascript`,
/// `rb` → `ruby`. Unknown languages map to their own lowercased form.
/// Used by the eval-trust policy so an allowlist entry and a block's
/// language compare on the same canonical key.
#[must_use]
pub fn canonical_language(lang: &str) -> String {
    match lang.to_ascii_lowercase().as_str() {
        "shell" | "sh" | "bash" => "shell".to_owned(),
        "python" | "py" => "python".to_owned(),
        "javascript" | "js" | "node" => "javascript".to_owned(),
        "ruby" | "rb" => "ruby".to_owned(),
        other => other.to_owned(),
    }
}

/// C1a security gate: whether `lang` may execute given the allowlist.
///
/// Default-deny — an empty `trust` runs nothing. Both sides are
/// canonicalised, so `py` in the allowlist trusts a `python` block and
/// vice versa.
#[must_use]
pub fn eval_allowed(trust: &[String], lang: &str) -> bool {
    let want = canonical_language(lang);
    trust.iter().any(|t| canonical_language(t) == want)
}

/// All recognised language identifiers in canonical form. Used by
/// the CLI / shells to enumerate which backends are wired in.
#[must_use]
pub const fn known_languages() -> &'static [&'static str] {
    &[
        "shell",
        "sh",
        "bash",
        "python",
        "py",
        "javascript",
        "js",
        "node",
        "ruby",
        "rb",
    ]
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

/// Read `r` to EOF, retaining at most `cap` bytes; signal once on `tx`
/// when the cap is first exceeded. Reading continues past the cap (the
/// bytes are discarded) so the child's pipe never fills and blocks —
/// the caller kills the child on the signal.
fn drain_capped<R>(mut r: R, cap: usize, tx: std::sync::mpsc::Sender<()>) -> std::thread::JoinHandle<Vec<u8>>
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut signalled = false;
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() < cap {
                        let take = (cap - buf.len()).min(n);
                        buf.extend_from_slice(&chunk[..take]);
                    }
                    if buf.len() >= cap && !signalled {
                        signalled = true;
                        let _ = tx.send(());
                    }
                }
            }
        }
        buf
    })
}

/// Run `prog` with `src` on stdin under resource [`Bounds`] (C1b).
///
/// The child runs in its own process group (unix) and stdout/stderr are
/// drained on threads with a byte cap, so neither a runaway loop (killed
/// at the deadline → [`EvalError::Timeout`]) nor a flood of output
/// (killed at the cap → truncated `Output`) can hang or OOM the host.
fn run_bounded(
    prog: &str,
    args: &[&str],
    src: &str,
    bounds: Bounds,
) -> Result<Output, EvalError> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Own process group: isolates the child (and lets a future
        // group-kill reach its descendants — see C1c for true forkbomb
        // containment, which needs a sandboxed runtime).
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| EvalError::Spawn(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin
            .write_all(src.as_bytes())
            .map_err(|e| EvalError::Io(e.to_string()))?;
        // dropped here → EOF for the child's stdin.
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EvalError::Io("no stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| EvalError::Io("no stderr pipe".into()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    let out_h = drain_capped(stdout, bounds.max_output, tx.clone());
    let err_h = drain_capped(stderr, bounds.max_output, tx);

    let deadline = std::time::Instant::now() + bounds.timeout;
    let mut code: Option<i32> = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                code = status.code();
                break;
            }
            Ok(None) => {
                if rx.try_recv().is_ok() {
                    // output cap exceeded: kill and keep the truncated buffer.
                    let _ = child.kill();
                    code = child.wait().ok().and_then(|s| s.code());
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(e) => return Err(EvalError::Io(e.to_string())),
        }
    }
    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();
    if timed_out {
        return Err(EvalError::Timeout(bounds.timeout));
    }
    Ok(Output {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit: code.unwrap_or(-1),
    })
}

/// Run a shell `program` with `input` piped to its stdin — the
/// formula primitive: callers feed database rows in, stdout comes
/// back as the result block.
///
/// # Errors
///
/// [`EvalError::Spawn`]/[`EvalError::Io`] on process failures.
pub fn eval_with_input(program: &str, input: &str) -> Result<Output, EvalError> {
    run_via_stdin("/bin/sh", &["-c", program], input, None)
}

/// Evaluate `program` once per row, feeding the row's cells as one
/// tab-separated stdin line; the trimmed stdout becomes the computed
/// cell. Coda-style column formulas in the user's language of choice.
///
/// # Errors
///
/// Propagates the first row's [`EvalError`].
pub fn formula_column(program: &str, rows: &[Vec<String>]) -> Result<Vec<String>, EvalError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let line = format!("{}\n", row.join("\t"));
        let result = eval_with_input(program, &line)?;
        out.push(result.stdout.trim_end().to_owned());
    }
    Ok(out)
}

/// Parsed babel header arguments (subset): `:results <mode>` and
/// repeated `:var name=value`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderArgs {
    /// `:results` mode (`output`, `silent`, …), verbatim.
    pub results: Option<String>,
    /// `:var name=value` pairs in source order.
    pub vars: Vec<(String, String)>,
    /// `:tangle <path>` target, `None` when absent or `:tangle no`.
    pub tangle: Option<String>,
}

impl HeaderArgs {
    /// Parse the raw header-arg string from a `#+BEGIN_SRC` line.
    /// Unknown directives are ignored (forward-compatible).
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut out = Self::default();
        let mut tokens = raw.split_whitespace();
        while let Some(tok) = tokens.next() {
            match tok {
                ":results" => out.results = tokens.next().map(str::to_owned),
                ":var" => {
                    if let Some((k, v)) = tokens.next().and_then(|kv| kv.split_once('=')) {
                        out.vars.push((k.to_owned(), v.to_owned()));
                    }
                }
                ":tangle" => {
                    out.tangle = tokens.next().filter(|&t| t != "no").map(str::to_owned);
                }
                _ => {}
            }
        }
        out
    }

    /// True when `:results silent` — evaluate but never attach
    /// `#+RESULTS:`.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.results.as_deref() == Some("silent")
    }
}

/// Language-specific prelude assigning `:var` bindings before the
/// block source. Unknown languages get no prelude.
#[must_use]
pub fn var_prelude(language: &str, vars: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in vars {
        match language {
            "shell" | "sh" | "bash" => {
                let _ = writeln!(out, "{k}='{v}'");
            }
            "python" => {
                let _ = writeln!(out, "{k} = \"{v}\"");
            }
            _ => {}
        }
    }
    out
}
