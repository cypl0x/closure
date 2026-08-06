//! Pointing the LLM at an OpenAI-compatible endpoint.
//!
//! The provider set already covers Anthropic, `OpenAI` and a local
//! Ollama, but "OpenAI-compatible" is a whole ecosystem — vLLM,
//! `llama.cpp`'s server, LM Studio, `OpenRouter`, a company proxy — and
//! all of them are "`OpenAI`'s wire format at a different URL". That is
//! one setting, not a new provider each time.
//!
//! It lives in the vault's `config.org` like everything else: plain
//! text, in the vault, versioned with it. The key itself never does —
//! `llm_key_env` names an environment variable, so a vault can be
//! committed and shared without leaking a credential.
//!
//! Validation is at load time, in the CUE-ish spirit the rest of the
//! config already follows (I9): a misconfiguration is an error when
//! the file is read, not a surprise the first time you ask a question.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, ConfigError};

fn parse(body: &str) -> Result<Config, ConfigError> {
    Config::from_org_source(&format!("#+BEGIN_SRC closure-config\n{body}\n#+END_SRC\n"))
}

#[test]
fn an_endpoint_can_be_set() {
    let cfg = parse(
        "llm_provider = openai-compatible\n\
         llm_endpoint = http://localhost:8080/v1/chat/completions\n\
         llm_model = local-model\n\
         llm_key_env = LOCAL_API_KEY",
    )
    .expect("valid");
    assert_eq!(
        cfg.llm_endpoint.as_deref(),
        Some("http://localhost:8080/v1/chat/completions")
    );
    assert_eq!(cfg.llm_provider.as_deref(), Some("openai-compatible"));
}

#[test]
fn no_endpoint_is_the_normal_case() {
    let cfg = parse("llm_provider = anthropic\nllm_key_env = ANTHROPIC_API_KEY").expect("valid");
    assert_eq!(
        cfg.llm_endpoint, None,
        "the built-in providers know their own URL"
    );
}

#[test]
fn an_openai_compatible_provider_requires_an_endpoint() {
    // Naming the wire format says nothing about *where*; without a URL
    // there is nothing to talk to, and finding that out on the first
    // question is worse than finding it out on load.
    let err = parse("llm_provider = openai-compatible\nllm_key_env = KEY").expect_err("refused");
    let ConfigError::BadValue { key, reason } = err else {
        panic!("expected a BadValue, got {err:?}");
    };
    assert_eq!(key, "llm_endpoint");
    assert!(!reason.is_empty(), "the reason must say what to do");
}

#[test]
fn an_endpoint_must_be_a_url_we_can_dial() {
    for bad in ["not a url", "ftp://example.com/v1", "/v1/chat"] {
        let err = parse(&format!(
            "llm_provider = openai-compatible\nllm_endpoint = {bad}\nllm_key_env = KEY"
        ))
        .expect_err("refused");
        assert!(
            matches!(err, ConfigError::BadValue { .. }),
            "{bad} should be refused, got {err:?}"
        );
    }
}

#[test]
fn https_and_http_are_both_accepted() {
    // Plain http is how every local runner ships; refusing it would
    // just mean nobody can point this at llama.cpp.
    for url in [
        "http://127.0.0.1:11434/v1/chat/completions",
        "https://openrouter.ai/api/v1/chat/completions",
    ] {
        parse(&format!(
            "llm_provider = openai-compatible\nllm_endpoint = {url}\nllm_key_env = KEY"
        ))
        .unwrap_or_else(|e| panic!("{url} should be accepted: {e:?}"));
    }
}

#[test]
fn an_endpoint_without_a_provider_is_refused() {
    // A URL with nothing to use it is a typo, not a configuration.
    let err = parse("llm_endpoint = http://localhost:8080/v1").expect_err("refused");
    assert!(matches!(err, ConfigError::BadValue { .. }), "{err:?}");
}

#[test]
fn a_keyless_local_endpoint_needs_no_key_env() {
    // llama.cpp and friends usually want no credential at all.
    let cfg =
        parse("llm_provider = ollama\nllm_endpoint = http://localhost:11434/v1/chat/completions")
            .expect("valid");
    assert_eq!(cfg.llm_key_env, None);
    assert!(cfg.llm_endpoint.is_some());
}

#[test]
fn the_key_itself_is_never_a_config_key() {
    // Naming the variable is the whole point: a vault is plain text
    // that gets committed, and a credential in it would be committed
    // with it.
    let err = parse("llm_api_key = sk-secret").expect_err("refused");
    // `UnknownKey` grew fields — the line and the key you probably
    // meant — for the "configuration language" item on 2026-08-06,
    // which asked for errors you can act on rather than errors you
    // have to go looking for. What this test is about is unchanged:
    // the key itself is never a config key.
    assert!(
        matches!(err, ConfigError::UnknownKey { ref key, .. } if key == "llm_api_key"),
        "{err:?}"
    );
}
