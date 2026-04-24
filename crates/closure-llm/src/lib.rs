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
