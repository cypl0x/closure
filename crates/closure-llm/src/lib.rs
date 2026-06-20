#![allow(clippy::doc_markdown)]
//! LLM adapter. BYOK + local + OpenAI-compatible + Claude.
//!
//! The LLM does not mutate the [`Document`] directly. It issues command
//! names from the [`Registry`] through a `CommandDispatcher`; the
//! registry executes the command and records the resulting [`Edit`].
//! This is I8 (command-registry as only side-effect surface).
//!
//! M7 skeleton: defines the provider trait and a mock provider. HTTP
//! clients for Claude / OpenAI / Ollama land behind feature flags.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Special command the executor handles to describe the current UI.
///
/// Returns the rendered state (mode, selection, visible
/// headlines/source) so the LLM can "see" what the user sees (vision
/// + ROADMAP deep access).
pub const VIEW_STATE_COMMAND: &str = "view-state";

/// The render-access tool: returns the rendered `ViewTree` (V3a). Gated
/// by [`LlmPermissions`] (V3b) — opt-in and revocable at runtime.
pub const RENDER_TOOL: &str = "view-render";

/// The command that toggles render access live (bound in every input
/// mode's keymap so it appears in which-key, I4).
pub const TOGGLE_RENDER_COMMAND: &str = "toggle-llm-render";

/// Live, configurable permission gate for LLM tools (V3b).
///
/// `base` is the `llm_tools` allowlist (`None` = every non-render tool
/// allowed). Render access is a separate opt-in bit: it is **off by
/// default** (the LLM cannot see the rendered screen unless explicitly
/// granted) and can be flipped at runtime via [`Self::toggle_render`] —
/// the live toggle bound to [`TOGGLE_RENDER_COMMAND`].
#[derive(Debug, Clone, Default)]
pub struct LlmPermissions {
    base: Option<std::collections::HashSet<String>>,
    render_granted: bool,
}

impl LlmPermissions {
    /// Build from the `llm_tools` config list. An empty list leaves
    /// non-render tools unrestricted; render is granted only when the
    /// list explicitly names `view-render` or `view`.
    #[must_use]
    pub fn from_config(list: Vec<String>) -> Self {
        let render_granted = list.iter().any(|t| t == RENDER_TOOL || t == "view");
        let base = if list.is_empty() {
            None
        } else {
            Some(list.into_iter().collect())
        };
        Self {
            base,
            render_granted,
        }
    }

    /// Whether `tool` may run. Render obeys the live opt-in bit; every
    /// other tool obeys the `base` allowlist (matching by exact name,
    /// `-`-prefix base, or prefix).
    #[must_use]
    pub fn allows(&self, tool: &str) -> bool {
        if tool == RENDER_TOOL {
            return self.render_granted;
        }
        self.base.as_ref().is_none_or(|set| {
            let cmd_base = tool.split('-').next().unwrap_or(tool);
            set.iter()
                .any(|a| a == tool || a == cmd_base || tool.starts_with(a.as_str()))
        })
    }

    /// Grant render access (live).
    pub const fn grant_render(&mut self) {
        self.render_granted = true;
    }

    /// Revoke render access (live).
    pub const fn revoke_render(&mut self) {
        self.render_granted = false;
    }

    /// Flip render access live; returns the new granted state.
    pub const fn toggle_render(&mut self) -> bool {
        self.render_granted = !self.render_granted;
        self.render_granted
    }
}

/// LLM provider trait.
pub trait Provider {
    /// Send a prompt and receive a (possibly streamed) completion.
    fn complete(&self, prompt: &str) -> Result<String, LlmError>;
}

/// Which provider a configured `llm_provider` name selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Echo (no network, no key) — also the default when unset.
    Echo,
    /// Local Ollama over the self-contained HTTP client (no key).
    Ollama,
    /// OpenAI-compatible HTTPS (curl path, BYOK).
    OpenAi,
    /// Anthropic HTTPS (curl path, BYOK).
    Anthropic,
}

/// Map an `llm_provider` config name to a [`ProviderKind`]. Unset and
/// `echo` select [`ProviderKind::Echo`] (no key required); an unknown
/// name falls back to [`ProviderKind::Anthropic`] (BYOK).
#[must_use]
pub fn provider_kind(name: Option<&str>) -> ProviderKind {
    match name {
        None | Some("echo") => ProviderKind::Echo,
        Some("ollama") => ProviderKind::Ollama,
        Some("openai") => ProviderKind::OpenAi,
        _ => ProviderKind::Anthropic,
    }
}

/// Read an API key from the named environment variable. The key lives
/// only in the environment — never in the config / org file. Returns
/// `None` when the variable is unset.
#[must_use]
pub fn resolve_key(env_var: &str) -> Option<String> {
    std::env::var(env_var).ok()
}

/// Build a boxed [`Provider`] for `kind`.
///
/// Ollama uses the self-contained [`HttpProvider`] at `ollama_host`;
/// OpenAI/Anthropic use the [`CurlProvider`] HTTPS path with `key` (read
/// from the environment by the caller via [`resolve_key`]); Echo needs
/// neither.
#[must_use]
pub fn build_provider(
    kind: ProviderKind,
    model: &str,
    ollama_host: &str,
    key: &str,
) -> Box<dyn Provider> {
    match kind {
        ProviderKind::Echo => Box::new(EchoProvider),
        ProviderKind::Ollama => Box::new(ollama_http(ollama_host, model)),
        ProviderKind::OpenAi => Box::new(openai(key, model)),
        ProviderKind::Anthropic => Box::new(anthropic(key, model)),
    }
}

/// LLM adapter error.
#[derive(Debug, Error)]
pub enum LlmError {
    /// Transport or API failure.
    #[error("provider: {0}")]
    Provider(String),
    /// Credential missing or invalid.
    #[error("auth")]
    Auth,
}

/// Echo provider: returns the prompt unchanged. Useful for tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct EchoProvider;

impl Provider for EchoProvider {
    fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        Ok(prompt.to_owned())
    }
}

/// HTTP provider that shells out to `curl` to POST a JSON body.
///
/// Generic enough to talk to OpenAI-, Anthropic-, or Ollama-compatible
/// APIs by varying the URL, header list, and request template. Avoids
/// pulling an HTTP client into the kernel build; users who need
/// pure-Rust transport can write their own [`Provider`] in a few
/// dozen lines.
pub struct CurlProvider {
    /// Endpoint URL (e.g. `https://api.openai.com/v1/chat/completions`).
    pub url: String,
    /// Extra headers (e.g. `Authorization: Bearer ...`).
    pub headers: Vec<String>,
    /// Function that builds the JSON body from a prompt.
    pub body: fn(&str) -> String,
    /// Function that extracts the completion text from the response
    /// JSON. The default just returns the response verbatim.
    pub extract: fn(&str) -> Result<String, LlmError>,
}

impl CurlProvider {
    /// Construct a provider with sensible defaults: empty body /
    /// passthrough extractor. Replace fields per-API.
    #[must_use]
    pub fn new(url: String) -> Self {
        Self {
            url,
            headers: Vec::new(),
            body: |p| format!("{{\"prompt\": {}}}", json_string(p)),
            extract: |s| Ok(s.to_owned()),
        }
    }
}

impl Provider for CurlProvider {
    fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        let body = (self.body)(prompt);
        let mut cmd = std::process::Command::new("curl");
        cmd.arg("-sS").arg("-X").arg("POST").arg("-d").arg(&body);
        for h in &self.headers {
            cmd.arg("-H").arg(h);
        }
        cmd.arg(&self.url);
        let out = cmd
            .output()
            .map_err(|e| LlmError::Provider(e.to_string()))?;
        if !out.status.success() {
            return Err(LlmError::Provider(format!(
                "curl exit {}",
                out.status.code().unwrap_or(-1)
            )));
        }
        let body = String::from_utf8_lossy(&out.stdout);
        (self.extract)(&body)
    }
}

/// Self-contained HTTP/1.1 provider over `std::net` (no TLS, no deps).
///
/// POSTs the built body to a plain-HTTP endpoint and returns the
/// extracted response. Suited to localhost services like Ollama; HTTPS
/// APIs (Anthropic/OpenAI) use [`CurlProvider`] instead. Same
/// url/headers/body/extract shape as [`CurlProvider`] so the per-API
/// constructors can target either transport.
pub struct HttpProvider {
    /// Endpoint URL (`http://host[:port]/path`).
    pub url: String,
    /// Extra request headers (`Name: value`).
    pub headers: Vec<String>,
    /// Builds the request body from a prompt.
    pub body: fn(&str) -> String,
    /// Extracts the completion from the response body.
    pub extract: fn(&str) -> Result<String, LlmError>,
}

impl HttpProvider {
    /// New provider with a JSON-prompt body + passthrough extractor.
    #[must_use]
    pub fn new(url: String) -> Self {
        Self {
            url,
            headers: vec!["content-type: application/json".into()],
            body: |p| format!("{{\"prompt\": {}}}", json_string(p)),
            extract: |s| Ok(s.to_owned()),
        }
    }
}

impl Provider for HttpProvider {
    fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        let body = (self.body)(prompt);
        let resp = http_post(&self.url, &self.headers, &body)?;
        (self.extract)(&resp)
    }
}

/// Parse `http://host[:port]/path` into `(host, port, path)`.
fn parse_http_url(url: &str) -> Result<(String, u16, String), LlmError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| LlmError::Provider(format!("not an http:// url: {url}")))?;
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let (host, port) = authority.split_once(':').map_or_else(
        || (authority.to_owned(), 80u16),
        |(h, p)| (h.to_owned(), p.parse().unwrap_or(80)),
    );
    Ok((host, port, format!("/{path}")))
}

/// POST `body` to a plain-HTTP `url` with `headers`; return the response
/// body on a 2xx status, else [`LlmError::Provider`]. No TLS.
fn http_post(url: &str, headers: &[String], body: &str) -> Result<String, LlmError> {
    use std::io::{Read as _, Write as _};
    let (host, port, path) = parse_http_url(url)?;
    let mut stream = std::net::TcpStream::connect((host.as_str(), port))
        .map_err(|e| LlmError::Provider(e.to_string()))?;
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for h in headers {
        req.push_str(h);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream
        .write_all(req.as_bytes())
        .map_err(|e| LlmError::Provider(e.to_string()))?;
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|e| LlmError::Provider(e.to_string()))?;
    let status_ok = raw
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .is_some_and(|code| code.starts_with('2'));
    let resp_body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_owned();
    if status_ok {
        Ok(resp_body)
    } else {
        Err(LlmError::Provider(format!(
            "http status: {}",
            raw.lines().next().unwrap_or("(no status line)")
        )))
    }
}

/// Configure a [`CurlProvider`] for an Anthropic `messages` endpoint.
///
/// `api_key` becomes the `x-api-key` header. The default body uses
/// model `claude-sonnet-4-6` and a 1024-token max — callers needing a
/// different model swap `provider.body` after construction. Extractor
/// returns the response verbatim until a real JSON parser lands.
#[must_use]
pub fn anthropic(api_key: &str, _model: &str) -> CurlProvider {
    CurlProvider {
        url: "https://api.anthropic.com/v1/messages".into(),
        headers: vec![
            "content-type: application/json".into(),
            "anthropic-version: 2023-06-01".into(),
            format!("x-api-key: {api_key}"),
        ],
        body: anthropic_body,
        extract: |s| {
            extract_anthropic_content(s).ok_or_else(|| LlmError::Provider("no content".into()))
        },
    }
}

/// Configure a [`CurlProvider`] for an OpenAI-compatible
/// `/v1/chat/completions` endpoint. Default model is `gpt-4o`.
#[must_use]
pub fn openai(api_key: &str, _model: &str) -> CurlProvider {
    CurlProvider {
        url: "https://api.openai.com/v1/chat/completions".into(),
        headers: vec![
            "content-type: application/json".into(),
            format!("authorization: Bearer {api_key}"),
        ],
        body: openai_body,
        extract: |s| {
            extract_openai_content(s).ok_or_else(|| LlmError::Provider("no content".into()))
        },
    }
}

/// Configure a [`CurlProvider`] for a local Ollama server (no auth).
/// Default model is `llama3`.
#[must_use]
pub fn ollama(host: &str, _model: &str) -> CurlProvider {
    CurlProvider {
        url: format!("{host}/api/generate"),
        headers: vec!["content-type: application/json".into()],
        body: ollama_body,
        extract: |s| {
            extract_ollama_response(s).ok_or_else(|| LlmError::Provider("no response".into()))
        },
    }
}

/// Self-contained [`HttpProvider`] for a local Ollama server.
///
/// Plain HTTP, no auth, no TLS — the hermetic, dependency-free path.
/// Uses the same `ollama_body` (model `llama3`, non-streaming) +
/// `extract_ollama_response` as [`ollama`]. `host` is e.g.
/// `http://127.0.0.1:11434`.
#[must_use]
pub fn ollama_http(host: &str, _model: &str) -> HttpProvider {
    HttpProvider {
        url: format!("{host}/api/generate"),
        headers: vec!["content-type: application/json".into()],
        body: ollama_body,
        extract: |s| {
            extract_ollama_response(s).ok_or_else(|| LlmError::Provider("no response".into()))
        },
    }
}

fn anthropic_body(p: &str) -> String {
    format!(
        "{{\"model\":\"claude-sonnet-4-6\",\"max_tokens\":1024,\"messages\":[{{\"role\":\"user\",\"content\":{prompt}}}]}}",
        prompt = json_string(p),
    )
}

fn openai_body(p: &str) -> String {
    format!(
        "{{\"model\":\"gpt-4o\",\"messages\":[{{\"role\":\"user\",\"content\":{prompt}}}]}}",
        prompt = json_string(p),
    )
}

fn ollama_body(p: &str) -> String {
    format!(
        "{{\"model\":\"llama3\",\"prompt\":{prompt},\"stream\":false}}",
        prompt = json_string(p),
    )
}

/// Crude extractor for an Anthropic `messages` response.
///
/// Returns the `content[0].text` field. Hand-rolled, naive —
/// sufficient for well-formed responses but will not handle every
/// JSON edge case until a real parser lands.
#[must_use]
pub fn extract_anthropic_content(body: &str) -> Option<String> {
    extract_json_string_after(body, "\"text\":")
}

/// Crude extractor for an OpenAI `chat/completions` response: returns
/// the `choices[0].message.content` field.
#[must_use]
pub fn extract_openai_content(body: &str) -> Option<String> {
    extract_json_string_after(body, "\"content\":")
}

/// Crude extractor for an Ollama `/api/generate` response: returns
/// the `response` field.
#[must_use]
pub fn extract_ollama_response(body: &str) -> Option<String> {
    extract_json_string_after(body, "\"response\":")
}

fn extract_json_string_after(body: &str, key: &str) -> Option<String> {
    let mut search_from = 0;
    while let Some(pos) = body[search_from..].find(key) {
        let after = body[search_from + pos + key.len()..].trim_start();
        if let Some(stripped) = after.strip_prefix('"')
            && let Some(value) = read_json_string(stripped)
        {
            return Some(value);
        }
        search_from += pos + key.len();
    }
    None
}

fn read_json_string(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code).unwrap_or('?'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Tiny JSON string escape — handles `\\`, `\"`, control chars; not
/// general-purpose JSON, just enough for `prompt` payloads.
#[must_use]
pub fn json_string(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Drive a provider through a text tool-use protocol.
///
/// Each turn the model replies either `CALL <command line>` —
/// executed by `execute` (the command registry surface, I8) with the
/// result fed back — or `DONE <answer>` / a bare answer, which ends
/// the loop.
///
/// # Errors
///
/// Provider failures propagate; exceeding `max_turns` is
/// [`LlmError::Provider`].
pub fn tool_loop(
    provider: &dyn Provider,
    mut execute: impl FnMut(&str) -> String,
    task: &str,
    max_turns: usize,
) -> Result<String, LlmError> {
    use std::fmt::Write as _;
    let mut transcript = format!(
        "TASK: {task}\n\
         Reply with exactly one line per turn:\n\
         CALL <command line>  — execute a registry command\n\
         DONE <answer>        — finish with your answer\n"
    );
    for _ in 0..max_turns {
        let reply = provider.complete(&transcript)?;
        let reply = reply.trim();
        if let Some(cmd) = reply.strip_prefix("CALL ") {
            let observation = execute(cmd);
            let _ = write!(transcript, "\nCALL {cmd}\nRESULT: {observation}\n");
        } else if let Some(answer) = reply.strip_prefix("DONE") {
            return Ok(answer.trim().to_owned());
        } else {
            return Ok(reply.to_owned());
        }
    }
    Err(LlmError::Provider(format!(
        "max turns ({max_turns}) exceeded"
    )))
}
