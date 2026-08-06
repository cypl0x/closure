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
    /// The server answered, with an error of its own. Carries the
    /// server's words: a bridge that replaces them with its own is a
    /// bridge you debug twice.
    #[error("{0}")]
    Server(String),
    /// The server answered with something this is not able to read.
    #[error("protocol: {0}")]
    Protocol(String),
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

/// One tool a server we are a client of offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTool {
    /// Qualified `<server>/<tool>`, so two servers may both have a
    /// `search` and the assistant can still say which it means.
    pub name: String,
    /// The server's own sentence about it.
    pub description: String,
    /// Its `inputSchema`, verbatim — what the caller has to send as
    /// the arguments object. Passed through rather than parsed: the
    /// schema is for whoever writes the call, and inventing a shape
    /// for it here would be a second, wrong answer.
    pub schema: String,
}

/// A connection to an MCP server closure is the *client* of.
///
/// The mirror of [`serve_jsonrpc`], on the same line-delimited
/// JSON-RPC: closure already exposes its vault as MCP tools, and this
/// is the other direction — a filesystem, a browser, an issue tracker
/// someone else wrote, showing up as tools the assistant can call.
///
/// Generic over the two pipe ends rather than owning a child process,
/// so the wire can be tested without spawning anything.
pub struct Conn<R: BufRead, W: std::io::Write> {
    name: String,
    reader: R,
    writer: W,
    next_id: u64,
}

impl<R: BufRead, W: std::io::Write> Conn<R, W> {
    /// Shake hands with the server on the other end of these pipes.
    ///
    /// `initialize`, then the `notifications/initialized` the spec
    /// requires before any other request — a server is entitled to
    /// refuse everything until it arrives.
    ///
    /// # Errors
    ///
    /// [`McpError::Transport`] if the pipe is closed or unreadable,
    /// [`McpError::Server`] if it refuses the handshake.
    pub fn connect(name: &str, reader: R, writer: W) -> Result<Self, McpError> {
        let mut conn = Self {
            name: name.to_owned(),
            reader,
            writer,
            next_id: 0,
        };
        // The version closure's own server speaks, so the two halves
        // agree about what they are.
        conn.request(
            "initialize",
            "{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\
             \"clientInfo\":{\"name\":\"closure\",\"version\":\"0.0.0\"}}",
        )?;
        conn.notify("notifications/initialized", "{}")?;
        Ok(conn)
    }

    /// Which server this is — the qualifier on every tool it offers.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.name
    }

    /// What it can do.
    ///
    /// # Errors
    ///
    /// As [`Self::connect`].
    pub fn tools(&mut self) -> Result<Vec<RemoteTool>, McpError> {
        let result = self.request("tools/list", "{}")?;
        let array = closure_jsonrpc::raw_value(&result, "tools")
            .ok_or_else(|| McpError::Protocol("no `tools` in the reply".to_owned()))?;
        Ok(closure_jsonrpc::array_items(&array)
            .iter()
            .filter_map(|item| {
                Some(RemoteTool {
                    name: format!(
                        "{}/{}",
                        self.name,
                        closure_jsonrpc::string_field(item, "name")?
                    ),
                    description: closure_jsonrpc::string_field(item, "description")
                        .unwrap_or_default(),
                    schema: closure_jsonrpc::raw_value(item, "inputSchema")
                        .unwrap_or_else(|| "{}".to_owned()),
                })
            })
            .collect())
    }

    /// Call one, with `arguments` as the JSON object its schema asks
    /// for. The text content of the reply comes back.
    ///
    /// # Errors
    ///
    /// As [`Self::connect`].
    pub fn call(&mut self, tool: &str, arguments: &str) -> Result<String, McpError> {
        // The unqualified name goes on the wire — the server has never
        // heard of the prefix, which is ours for telling servers apart.
        let tool = tool.rsplit('/').next().unwrap_or(tool);
        let params = format!(
            "{{\"name\":\"{}\",\"arguments\":{}}}",
            closure_jsonrpc::json_escape(tool),
            if arguments.trim().is_empty() {
                "{}"
            } else {
                arguments
            }
        );
        let result = self.request("tools/call", &params)?;
        let Some(content) = closure_jsonrpc::raw_value(&result, "content") else {
            return Ok(result);
        };
        Ok(closure_jsonrpc::array_items(&content)
            .iter()
            .filter_map(|part| closure_jsonrpc::string_field(part, "text"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// Send a request and read its reply, returning the `result`.
    fn request(&mut self, method: &str, params: &str) -> Result<String, McpError> {
        self.next_id += 1;
        let line = closure_jsonrpc::request(self.next_id, method, params);
        self.send(&line)?;
        let mut reply = String::new();
        let n = self
            .reader
            .read_line(&mut reply)
            .map_err(|e| McpError::Transport(e.to_string()))?;
        if n == 0 {
            return Err(McpError::Transport(format!(
                "`{}` closed the pipe without answering {method}",
                self.name
            )));
        }
        if let Some(err) = closure_jsonrpc::raw_value(&reply, "error") {
            let said = closure_jsonrpc::string_field(&err, "message").unwrap_or(err);
            return Err(McpError::Server(format!("{}: {said}", self.name)));
        }
        closure_jsonrpc::raw_value(&reply, "result")
            .ok_or_else(|| McpError::Protocol(format!("no `result` in: {}", reply.trim())))
    }

    /// Send a notification — no id, so nothing is waited for.
    fn notify(&mut self, method: &str, params: &str) -> Result<(), McpError> {
        let line = closure_jsonrpc::notification(method, params);
        self.send(&line)
    }

    fn send(&mut self, line: &str) -> Result<(), McpError> {
        writeln!(self.writer, "{line}").map_err(|e| McpError::Transport(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| McpError::Transport(e.to_string()))
    }
}

/// An MCP server closure started and is a client of.
///
/// Owns the child so that dropping the server stops the process: a
/// closure that exits leaving `npx` behind is a closure that leaks one
/// per run.
pub struct Server {
    conn: Conn<BufReader<std::process::ChildStdout>, std::process::ChildStdin>,
    child: std::process::Child,
}

impl Server {
    /// Start `command` and shake hands with it.
    ///
    /// The command is split on whitespace — no shell, so nothing in
    /// config.org can reach a shell metacharacter. A server needing
    /// one can be given `sh -c ...` explicitly, which is then the
    /// user's sentence rather than ours.
    ///
    /// # Errors
    ///
    /// [`McpError::Transport`] if the command cannot be started or
    /// closes its pipes, [`McpError::Server`] if it refuses the
    /// handshake.
    pub fn spawn(name: &str, command: &str) -> Result<Self, McpError> {
        let mut parts = command.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| McpError::Transport(format!("`mcp {name}` has no command")))?;
        let mut child = std::process::Command::new(program)
            .args(parts)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Its diagnostics are its own business and must not be
            // read as protocol; they go where the user can see them.
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| McpError::Transport(format!("{name}: cannot start `{program}`: {e}")))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(McpError::Transport(format!("{name}: no pipes")));
        };
        match Conn::connect(name, BufReader::new(stdout), stdin) {
            Ok(conn) => Ok(Self { conn, child }),
            Err(e) => {
                let _ = child.kill();
                Err(e)
            }
        }
    }

    /// What it can do, `<server>/<tool>`.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`].
    pub fn tools(&mut self) -> Result<Vec<RemoteTool>, McpError> {
        self.conn.tools()
    }

    /// Call one of its tools.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`].
    pub fn call(&mut self, tool: &str, arguments: &str) -> Result<String, McpError> {
        self.conn.call(tool, arguments)
    }

    /// Which server this is.
    #[must_use]
    pub fn name(&self) -> &str {
        self.conn.server_name()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every MCP server closure was configured to be a client of.
///
/// One place that owns the child processes, so the assistant loop asks
/// "is this one of yours?" of a single thing rather than keeping its
/// own list beside this one.
#[derive(Default)]
pub struct Servers {
    started: Vec<Server>,
    failures: Vec<(String, String)>,
}

impl Servers {
    /// Start each `(name, command)` from `mcp <name> = <command>`.
    ///
    /// A server that will not start is recorded, not returned as an
    /// error: one bad line in config.org must not cost you the other
    /// three servers, and the failure has to reach you as text rather
    /// than as an absence.
    #[must_use]
    pub fn start(specs: &[(String, String)]) -> Self {
        let mut out = Self::default();
        for (name, command) in specs {
            match Server::spawn(name, command) {
                Ok(server) => out.started.push(server),
                Err(e) => out
                    .failures
                    .push((name.clone(), format!("mcp {name}: {e}"))),
            }
        }
        out
    }

    /// The servers that did not start, and what they said.
    #[must_use]
    pub fn failures(&self) -> &[(String, String)] {
        &self.failures
    }

    /// Whether there is anything here at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.started.is_empty()
    }

    /// One line per remote tool, for the menu the assistant is given:
    /// the qualified name, the arguments object it wants, and the
    /// server's own sentence about it.
    ///
    /// The schema goes in verbatim because the caller has to fill it
    /// in — a menu that said only the name would be asking the model
    /// to guess the argument names, which is how a tool loop spends
    /// four turns discovering `path` was called `file`.
    pub fn menu(&mut self) -> String {
        let mut lines = Vec::new();
        for server in &mut self.started {
            match server.tools() {
                Ok(tools) => lines.extend(
                    tools
                        .into_iter()
                        .map(|t| format!("{} {} — {}", t.name, t.schema, t.description)),
                ),
                Err(e) => lines.push(format!("({} is not answering: {e})", server.name())),
            }
        }
        lines.join("\n")
    }

    /// Run `line` if it names a remote tool; `None` when it does not,
    /// which leaves the vault's own tools exactly as they were.
    ///
    /// The rest of the line is the arguments object, verbatim. It is
    /// not parsed into one from a bare word: the schema is in the menu
    /// beside the name, and inventing a key here would be a guess that
    /// silently calls the wrong thing.
    pub fn call_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        let (tool, rest) = line.split_once(' ').unwrap_or((line, ""));
        let (server_name, _) = tool.split_once('/')?;
        let server = self.started.iter_mut().find(|s| s.name() == server_name)?;
        let rest = rest.trim();
        if !rest.is_empty() && !rest.starts_with('{') {
            return Some(format!(
                "ERROR `{tool}` takes its arguments as a JSON object, not `{rest}` — \
                 the object it wants is in the tool menu beside the name"
            ));
        }
        Some(match server.call(tool, rest) {
            Ok(out) => out,
            Err(e) => format!("ERROR {e}"),
        })
    }
}
