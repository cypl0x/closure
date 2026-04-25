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
