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
    /// Wasm sandbox failure: malformed module, denied import, missing
    /// `run` export, fuel exhaustion, or a trap (C1c).
    #[error("wasm: {0}")]
    Wasm(String),
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

/// Wasm sandbox backend (C1c): runs a `wasm` block — WAT text or binary
/// — under wasmtime with **no host imports**, the genuinely sandboxed
/// exec tier. The block must export `run: () -> i32`; that integer is
/// the result. Execution is fuel-bounded so an infinite loop traps
/// rather than hanging. Available only under the `wasmtime` feature.
#[cfg(feature = "wasmtime")]
#[derive(Debug, Default, Clone, Copy)]
pub struct WasmBackend;

#[cfg(feature = "wasmtime")]
impl WasmBackend {
    /// Instruction budget for a single sandboxed call. Generous for
    /// real compute, finite so a runaway loop traps (out-of-fuel).
    const FUEL: u64 = 1_000_000_000;
}

#[cfg(feature = "wasmtime")]
impl Backend for WasmBackend {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "wasm"
    }

    fn eval(&self, src: &str) -> Result<Output, EvalError> {
        use wasmtime::{Config, Engine, Instance, Module, Store};

        let wasm_err = |e: wasmtime::Error| EvalError::Wasm(e.to_string());
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(wasm_err)?;
        // `Module::new` auto-detects WAT text vs binary (the `wat`
        // feature). A parse failure is a clean Err, never a panic.
        let module = Module::new(&engine, src.as_bytes()).map_err(wasm_err)?;
        let mut store = Store::new(&engine, ());
        store.set_fuel(Self::FUEL).map_err(wasm_err)?;
        // Empty import list: a module that needs any import fails to
        // instantiate — the host surface is exactly nothing.
        let instance = Instance::new(&mut store, &module, &[]).map_err(wasm_err)?;
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .map_err(|_| EvalError::Wasm("module has no `run: () -> i32` export".into()))?;
        let value = run.call(&mut store, ()).map_err(wasm_err)?;
        Ok(Output {
            stdout: value.to_string(),
            stderr: String::new(),
            exit: 0,
        })
    }
}

/// Pick a backend for a language identifier (case-insensitive). Returns
/// Why a noweb reference could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NowebError {
    /// A block includes itself, directly or through others.
    #[error("noweb cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// `<<name>>` names a block that is not in the document.
    #[error("no block named `{0}`")]
    Unknown(String),
    /// Nested deeper than [`NOWEB_DEPTH_LIMIT`] without repeating.
    #[error("noweb nesting deeper than {0}")]
    TooDeep(usize),
}

/// How deep noweb references may nest.
///
/// Same reasoning as the widget and include limits: a deep nest is not
/// a cycle, and recursing it would end the process rather than the
/// expansion (I5).
pub const NOWEB_DEPTH_LIMIT: usize = 32;

/// The body of the `#+NAME: name` source block in `doc`, if there is
/// one.
fn named_block(doc: &str, name: &str) -> Option<String> {
    let mut lines = doc.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let Some(rest) = t
            .strip_prefix("#+NAME:")
            .or_else(|| t.strip_prefix("#+name:"))
        else {
            continue;
        };
        if rest.trim() != name {
            continue;
        }
        // The block itself is the next `#+BEGIN_SRC` … `#+END_SRC`.
        let mut body = String::new();
        let mut inside = false;
        for l in lines.by_ref() {
            let lt = l.trim_start().to_ascii_lowercase();
            if !inside {
                if lt.starts_with("#+begin_src") {
                    inside = true;
                }
                continue;
            }
            if lt.starts_with("#+end_src") {
                return Some(body);
            }
            body.push_str(l);
            body.push('\n');
        }
        return Some(body);
    }
    None
}

/// Replace every `<<name>>` in `src` with the block it names, taken
/// from `doc`.
///
/// Only a reference alone on its line — possibly indented — is one:
/// `a << b` is a shift, and a parser that says otherwise finds
/// references in arithmetic. The indentation at the reference is
/// applied to every line that replaces it, because a block being
/// pasted into Python or YAML is being pasted where indentation is
/// syntax.
///
/// # Errors
///
/// [`NowebError::Unknown`] naming the reference, [`NowebError::Cycle`]
/// carrying the ring, or [`NowebError::TooDeep`].
pub fn expand_noweb(src: &str, doc: &str) -> Result<String, NowebError> {
    let mut stack: Vec<String> = Vec::new();
    expand_noweb_in(src, doc, &mut stack)
}

/// [`expand_noweb`], carrying the chain of blocks being expanded.
fn expand_noweb_in(src: &str, doc: &str, stack: &mut Vec<String>) -> Result<String, NowebError> {
    if !src.contains("<<") {
        return Ok(src.to_owned());
    }
    if stack.len() >= NOWEB_DEPTH_LIMIT {
        return Err(NowebError::TooDeep(NOWEB_DEPTH_LIMIT));
    }
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        let indent: String = bare.chars().take_while(|c| c.is_whitespace()).collect();
        let trimmed = bare.trim();
        let Some(name) = trimmed
            .strip_prefix("<<")
            .and_then(|r| r.strip_suffix(">>"))
            .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))
        else {
            out.push_str(line);
            continue;
        };
        if stack.iter().any(|s| s == name) {
            let from = stack.iter().position(|s| s == name).unwrap_or(0);
            let mut ring: Vec<String> = stack[from..].to_vec();
            ring.push(name.to_owned());
            return Err(NowebError::Cycle(ring));
        }
        let body = named_block(doc, name).ok_or_else(|| NowebError::Unknown(name.to_owned()))?;
        stack.push(name.to_owned());
        let expanded = expand_noweb_in(&body, doc, stack)?;
        stack.pop();
        for l in expanded.lines() {
            out.push_str(&indent);
            out.push_str(l);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Why a `#+CALL:` could not be run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// The line is not a `#+CALL:` at all.
    #[error("not a #+CALL: line")]
    NotACall,
    /// No block in the document carries that `#+NAME:`.
    #[error("no block named `{0}`")]
    Unknown(String),
    /// The named block's language is not in the vault's trust list.
    ///
    /// Routed through the same check every other block uses: a call
    /// that could run what an ordinary block may not would be a way
    /// around the check rather than a feature.
    #[error("`{0}` is not a trusted language in this vault")]
    NotTrusted(String),
    /// The block was assembled or run and failed.
    #[error("{0}")]
    Failed(String),
}

/// The language of the `#+NAME: name` source block in `doc`.
fn named_block_language(doc: &str, name: &str) -> Option<String> {
    let mut lines = doc.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let Some(rest) = t
            .strip_prefix("#+NAME:")
            .or_else(|| t.strip_prefix("#+name:"))
        else {
            continue;
        };
        if rest.trim() != name {
            continue;
        }
        for l in lines.by_ref() {
            let lt = l.trim_start().to_ascii_lowercase();
            if let Some(args) = lt.strip_prefix("#+begin_src") {
                return Some(args.split_whitespace().next().unwrap_or("").to_owned());
            }
        }
        return None;
    }
    None
}

/// Run the block a `#+CALL:` line names.
///
/// Noweb assembles a named block into another; this evaluates one from
/// somewhere else, and between them that is what a reusable block is.
/// The block is assembled first, so `<<setup>>` inside a called block
/// means what it means anywhere else.
///
/// # Errors
///
/// [`CallError::NotACall`] for anything that is not one,
/// [`CallError::Unknown`] naming the block, [`CallError::NotTrusted`]
/// naming the language, or [`CallError::Failed`].
pub fn run_call(line: &str, doc: &str, trust: &[String]) -> Result<Output, CallError> {
    let name = call_target(line).ok_or(CallError::NotACall)?;
    let body = named_block(doc, &name).ok_or_else(|| CallError::Unknown(name.clone()))?;
    let lang = named_block_language(doc, &name).unwrap_or_default();
    if !eval_allowed(trust, &lang) {
        return Err(CallError::NotTrusted(lang));
    }
    let program = expand_noweb(&body, doc).map_err(|e| CallError::Failed(e.to_string()))?;
    let backend = backend_for(&lang).ok_or_else(|| CallError::NotTrusted(lang.clone()))?;
    backend
        .eval(&program)
        .map_err(|e| CallError::Failed(e.to_string()))
}

/// The block a `#+CALL: name(...)` line runs, if this is one.
///
/// The parentheses are org's and are required: `#+CALL: setup` without
/// them is not a call, and guessing would make every keyword line a
/// candidate.
#[must_use]
pub fn call_target(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("#+CALL:")
        .or_else(|| t.strip_prefix("#+call:"))?
        .trim();
    let name = rest.split_once('(')?.0.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// `None` if no backend is registered for the language.
#[must_use]
pub fn backend_for(lang: &str) -> Option<Box<dyn Backend>> {
    match lang.to_ascii_lowercase().as_str() {
        "shell" | "sh" | "bash" => Some(Box::new(ShellBackend)),
        "python" | "py" => Some(Box::new(PythonBackend)),
        "javascript" | "js" | "node" => Some(Box::new(NodeBackend)),
        "ruby" | "rb" => Some(Box::new(RubyBackend)),
        #[cfg(feature = "wasmtime")]
        "wasm" => Some(Box::new(WasmBackend)),
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
        // A diagram runs a program on your machine, so it is trusted
        // by language like everything else, and `tex` in the allowlist
        // has to trust a `latex` block for the same reason `py` trusts
        // `python`.
        "latex" | "tex" => "latex".to_owned(),
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
fn drain_capped<R>(
    mut r: R,
    cap: usize,
    tx: std::sync::mpsc::Sender<()>,
) -> std::thread::JoinHandle<Vec<u8>>
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
fn run_bounded(prog: &str, args: &[&str], src: &str, bounds: Bounds) -> Result<Output, EvalError> {
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

/// Run a shell command for the `:!` escape: bounded by `timeout`, and
/// never held open by a grandchild that inherited the pipe.
///
/// `:! xdg-open .` froze the whole app. `xdg-open` exits almost at
/// once; what it leaves behind is a file manager holding the *write
/// end* of the pipe it inherited, so the read end never sees EOF. Every
/// path that collects output by reading to EOF then waits for a program
/// the user opened deliberately and will close in ten minutes.
///
/// [`run_bounded`] looks like it covers this and does not: it polls the
/// child against a deadline, and then *joins* the drain threads — which
/// are precisely the reads that never finish. The deadline never comes
/// into it, because the child really did exit.
///
/// So: once the process we started is gone, stop waiting on its pipe.
/// Whatever arrived by then is the output. The drain threads are left
/// to finish on their own whenever the grandchild is done; they hold
/// nothing but a pipe and their own buffer.
///
/// # Errors
///
/// [`EvalError::Spawn`] if the shell will not start,
/// [`EvalError::Timeout`] if the command itself outlives `timeout`.
pub fn shell_escape(cmd: &str, timeout: std::time::Duration) -> Result<Output, EvalError> {
    /// How long to keep reading after the child has exited. Enough for
    /// output already in flight down the pipe, short enough that a
    /// grandchild holding it open is not felt.
    const LINGER: std::time::Duration = std::time::Duration::from_millis(150);

    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| EvalError::Spawn(e.to_string()))?;
    let out_rx = drain_to_channel(child.stdout.take());
    let err_rx = drain_to_channel(child.stderr.take());

    let deadline = std::time::Instant::now() + timeout;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(EvalError::Timeout(timeout));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(e) => return Err(EvalError::Io(e.to_string())),
        }
    };
    // The child is gone. Take what the pipe has to give in a moment,
    // and do not wait on anything still holding it open.
    let collect = |rx: &std::sync::mpsc::Receiver<Vec<u8>>| {
        let until = std::time::Instant::now() + LINGER;
        let mut buf = Vec::new();
        // Until EOF or the linger runs out, whichever comes first. The
        // chunks have to be accumulated rather than waited for as one
        // buffer: a grandchild holds the pipe open, so "the whole
        // thing" never arrives, and what the command actually printed
        // would be thrown away with it.
        while let Ok(chunk) =
            rx.recv_timeout(until.saturating_duration_since(std::time::Instant::now()))
        {
            buf.extend_from_slice(&chunk);
        }
        String::from_utf8_lossy(&buf).into_owned()
    };
    Ok(Output {
        stdout: collect(&out_rx),
        stderr: collect(&err_rx),
        exit: code,
    })
}

/// Read a pipe on its own thread, sending each chunk down a channel as
/// it arrives — so the reader can be *waited on with a deadline*,
/// which joining a thread cannot be.
///
/// Chunks rather than one buffer at EOF, because in the case this
/// exists for EOF never comes: a grandchild is holding the pipe. The
/// output the command actually produced is already through, and
/// delivering it only at EOF would throw it away.
fn drain_to_channel<R>(r: Option<R>) -> std::sync::mpsc::Receiver<Vec<u8>>
where
    R: std::io::Read + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut r) = r {
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match r.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(chunk[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    rx
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

// === Diagrams: blocks whose output is a picture ===
//
// "mermaid diagrams (plugin)" and "(inline) LaTeX preview (plugin)".
//
// Both are the shape org has had for LaTeX since forever: a block of
// text is handed to an external program and what comes back is looked
// at rather than read. Nothing here is a new subsystem — a diagram is
// a src block whose result is an image, so it is gated by the same
// eval-trust allowlist as any other block that runs a program, and it
// is painted by the inline-picture path that already exists.
//
// closure ships neither `mmdc` nor `latex`, exactly as org ships
// neither: you point it at what you have. The only unacceptable
// outcome is silence, so a missing tool names itself and says where to
// get it.

/// A block language whose output is a picture rather than text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diagram {
    /// `#+begin_src mermaid` — rendered by mermaid-cli.
    Mermaid,
    /// `#+begin_src latex` — rendered by a TeX installation.
    Latex,
}

impl Diagram {
    /// The program that renders this language, by default.
    ///
    /// `mmdc` is mermaid-cli's binary and what every org mermaid setup
    /// shells out to; `latex` is where
    /// `org-preview-latex-process-alist` starts.
    #[must_use]
    pub const fn tool(self) -> &'static str {
        match self {
            Self::Mermaid => "mmdc",
            Self::Latex => "latex",
        }
    }

    /// The canonical org language name.
    #[must_use]
    pub const fn language(self) -> &'static str {
        match self {
            Self::Mermaid => "mermaid",
            Self::Latex => "latex",
        }
    }

    /// Where to get the tool, for the message a user actually reads.
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Mermaid => "install mermaid-cli (nix shell nixpkgs#mermaid-cli)",
            Self::Latex => {
                // The real attribute names, both halves: `dvipng` is
                // not a top-level nixpkgs package and no TeX scheme
                // carries it, so a hint saying "a TeX distribution"
                // sends you somewhere that does not have it.
                "install TeX and dvipng (nix shell nixpkgs#texliveSmall \
                            nixpkgs#texlivePackages.dvipng)"
            }
        }
    }
}

/// The diagram language `lang` names, if it is one.
///
/// `tex` is an alias for `latex` because that is what people type.
/// Everything else — `shell`, `python` — still produces text, and
/// `#+RESULTS:` remains the contract for those.
#[must_use]
pub fn diagram_for(lang: &str) -> Option<Diagram> {
    match lang.to_ascii_lowercase().as_str() {
        "mermaid" => Some(Diagram::Mermaid),
        "latex" | "tex" => Some(Diagram::Latex),
        _ => None,
    }
}

/// Where the picture for this source lives.
///
/// Named for the hash of what produced it, so the same source always
/// names the same file and an edit names a different one. This is what
/// makes rendering affordable at all: a note full of diagrams renders
/// once, ever, and reopening it renders nothing.
#[must_use]
pub fn diagram_path(
    cache: &std::path::Path,
    kind: Diagram,
    src: &str,
    ink: u32,
) -> std::path::PathBuf {
    let mut hasher = blake3::Hasher::new();
    // The language goes into the hash, not just the filename: the same
    // bytes rendered by two different programs are two different
    // pictures.
    hasher.update(kind.language().as_bytes());
    hasher.update(b"\0");
    // …and so does the ink, because a picture drawn for a dark theme
    // is the wrong picture for a light one. A cache that ignored the
    // colour would hand the old one over after a theme switch.
    hasher.update(&ink.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(src.as_bytes());
    let hex = hasher.finalize().to_hex();
    cache.join(format!("{}-{}.png", kind.language(), &hex[..32]))
}

/// A diagram failed to become a picture.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The renderer is not installed. Named, with somewhere to get it:
    /// this is the failure that must never be silent.
    #[error("{tool} is not installed — {hint}")]
    ToolMissing {
        /// The program that could not be found.
        tool: String,
        /// Where to get it.
        hint: &'static str,
    },
    /// The renderer ran and refused the source.
    #[error("{0}")]
    Failed(String),
    /// Reading or writing the cache failed.
    #[error("io: {0}")]
    Io(String),
}

/// Render `src` to a picture, or hand back the one already rendered.
///
/// `tool` is the program to run — the language's default
/// ([`Diagram::tool`]) unless config.org names another, so a user with
/// a wrapper script or a pinned version is not arguing with us.
///
/// A cache hit runs nothing at all, which is deliberate and is what
/// the "already rendered" test pins: opening a note must not shell out
/// once per diagram per frame.
pub fn render_diagram(
    kind: Diagram,
    src: &str,
    cache: &std::path::Path,
    tool: &str,
    ink: u32,
) -> Result<std::path::PathBuf, RenderError> {
    let out = diagram_path(cache, kind, src, ink);
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache).map_err(|e| RenderError::Io(e.to_string()))?;
    match kind {
        Diagram::Mermaid => render_mermaid(src, &out, tool)?,
        Diagram::Latex => render_latex(src, &out, tool, ink)?,
    }
    if out.is_file() {
        Ok(out)
    } else {
        Err(RenderError::Failed(format!(
            "{tool} produced no picture for the {} block",
            kind.language()
        )))
    }
}

/// Distinguish "no such program" from "the program said no".
///
/// The two need completely different messages — one is a thing to
/// install, the other is a thing to fix in the block — and
/// `io::ErrorKind::NotFound` from a spawn is the only reliable way to
/// tell them apart.
fn spawn_err(kind: Diagram, tool: &str, e: &std::io::Error) -> RenderError {
    if e.kind() == std::io::ErrorKind::NotFound {
        RenderError::ToolMissing {
            tool: tool.to_owned(),
            hint: kind.hint(),
        }
    } else {
        RenderError::Failed(format!("{tool}: {e}"))
    }
}

/// mermaid-cli reads a `.mmd` file and writes the picture.
fn render_mermaid(src: &str, out: &std::path::Path, tool: &str) -> Result<(), RenderError> {
    let dir = tempfile::tempdir().map_err(|e| RenderError::Io(e.to_string()))?;
    let input = dir.path().join("diagram.mmd");
    std::fs::write(&input, src).map_err(|e| RenderError::Io(e.to_string()))?;
    let done = Command::new(tool)
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(out)
        // A diagram is drawn to be read on a dark editor background as
        // often as a light one, and mermaid's default white card on a
        // dark note is a torch in the face.
        .arg("-b")
        .arg("transparent")
        .output()
        .map_err(|e| spawn_err(Diagram::Mermaid, tool, &e))?;
    if done.status.success() {
        Ok(())
    } else {
        Err(RenderError::Failed(tail_of(&done.stderr, tool)))
    }
}

/// An `0x00RRGGBB` colour as dvipng spells one.
///
/// dvipng wants `rgb r g b` with components in 0..1, not a hex triple,
/// and a wrong spelling here is not an error — it is silently black
/// again, which is the bug this exists to prevent.
#[must_use]
pub fn dvipng_fg(ink: u32) -> String {
    // The truncation is the point: each shift selects one byte.
    let part = |shift: u32| f32::from(u8::try_from((ink >> shift) & 0xff).unwrap_or(0)) / 255.0;
    format!("rgb {:.3} {:.3} {:.3}", part(16), part(8), part(0))
}

/// The smallest document that will hold a fragment, so a note writes
/// `\frac{a}{b}` and not a preamble.
///
/// `article`, with the cropping left to `dvipng -T tight` — which is
/// exactly what org's own dvipng entry in
/// `org-preview-latex-process-alist` does, and for the same reason.
/// This wrapped fragments in `standalone` first, a nicer class that is
/// in no small TeX: on a `texliveSmall` the run stopped to ask where
/// `standalone.cls` was and the note showed "No pages of output". A
/// preview that needs a full TeX installation is a preview most people
/// cannot have.
#[must_use]
pub fn latex_document(src: &str) -> String {
    format!(
        "\\documentclass{{article}}\n\
         \\usepackage{{amsmath,amssymb}}\n\
         \\pagestyle{{empty}}\n\
         \\begin{{document}}\n{src}\n\\end{{document}}\n"
    )
}

/// A LaTeX fragment becomes a picture the way org does it: `latex` to
/// a DVI, then `dvipng` to the image. The fragment is wrapped in the
/// smallest document that will hold it, so a note writes `\frac{a}{b}`
/// and not a preamble.
fn render_latex(src: &str, out: &std::path::Path, tool: &str, ink: u32) -> Result<(), RenderError> {
    let dir = tempfile::tempdir().map_err(|e| RenderError::Io(e.to_string()))?;
    let tex = dir.path().join("fragment.tex");
    let doc = latex_document(src);
    std::fs::write(&tex, doc).map_err(|e| RenderError::Io(e.to_string()))?;
    let done = Command::new(tool)
        .arg("-interaction=nonstopmode")
        .arg("-halt-on-error")
        .arg("-output-directory")
        .arg(dir.path())
        .arg(&tex)
        .output()
        .map_err(|e| spawn_err(Diagram::Latex, tool, &e))?;
    if !done.status.success() {
        // TeX writes its complaint to stdout, not stderr.
        return Err(RenderError::Failed(tail_of(&done.stdout, tool)));
    }
    let dvi = dir.path().join("fragment.dvi");
    let done = Command::new("dvipng")
        .arg("-D")
        .arg("150")
        .arg("-T")
        .arg("tight")
        .arg("-bg")
        .arg("Transparent")
        // The theme's own foreground. dvipng inks in black unless told
        // otherwise, which made the first working preview correct and
        // invisible: black maths on a dark editor.
        .arg("-fg")
        .arg(dvipng_fg(ink))
        .arg("-o")
        .arg(out)
        .arg(&dvi)
        .output()
        .map_err(|e| spawn_err(Diagram::Latex, "dvipng", &e))?;
    if done.status.success() {
        Ok(())
    } else {
        Err(RenderError::Failed(tail_of(&done.stderr, "dvipng")))
    }
}

/// The last few lines of a renderer's complaint.
///
/// TeX in particular writes hundreds of lines and puts the actual
/// error near the end; a status bar can hold about one.
fn tail_of(bytes: &[u8], tool: &str) -> String {
    let text = String::from_utf8_lossy(bytes);
    let last: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(3)
        .collect();
    if last.is_empty() {
        format!("{tool} failed")
    } else {
        last.into_iter().rev().collect::<Vec<_>>().join(" · ")
    }
}
