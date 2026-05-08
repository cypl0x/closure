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

/// LLM provider trait.
pub trait Provider {
    /// Send a prompt and receive a (possibly streamed) completion.
    fn complete(&self, prompt: &str) -> Result<String, LlmError>;
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
