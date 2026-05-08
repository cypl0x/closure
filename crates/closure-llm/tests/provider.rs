#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_llm::{EchoProvider, Provider, json_string};

#[test]
fn echo_provider_returns_prompt() {
    let p = EchoProvider;
    assert_eq!(p.complete("hi").unwrap(), "hi");
}

#[test]
fn json_string_escapes_quotes_and_backslash() {
    assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
    assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
    assert_eq!(json_string("a\nb"), "\"a\\nb\"");
}

#[test]
fn anthropic_provider_sets_headers_and_url() {
    let p = closure_llm::anthropic("sk-test", "claude-sonnet-4-6");
    assert_eq!(p.url, "https://api.anthropic.com/v1/messages");
    assert!(p.headers.iter().any(|h| h.contains("anthropic-version")));
    assert!(p.headers.iter().any(|h| h == "x-api-key: sk-test"));
}

#[test]
fn openai_provider_sets_bearer() {
    let p = closure_llm::openai("sk-test", "gpt-4o");
    assert_eq!(p.url, "https://api.openai.com/v1/chat/completions");
    assert!(
        p.headers
            .iter()
            .any(|h| h == "authorization: Bearer sk-test")
    );
}

#[test]
fn ollama_provider_uses_local_host() {
    let p = closure_llm::ollama("http://127.0.0.1:11434", "llama3");
    assert_eq!(p.url, "http://127.0.0.1:11434/api/generate");
}

#[test]
fn anthropic_extractor_finds_text_field() {
    let body = r#"{"id":"msg","content":[{"type":"text","text":"hello world"}]}"#;
    assert_eq!(
        closure_llm::extract_anthropic_content(body),
        Some("hello world".to_owned())
    );
}

#[test]
fn openai_extractor_finds_content_field() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":"hi there"}}]}"#;
    assert_eq!(
        closure_llm::extract_openai_content(body),
        Some("hi there".to_owned())
    );
}

#[test]
fn ollama_extractor_finds_response_field() {
    let body = r#"{"model":"llama3","response":"yo","done":true}"#;
    assert_eq!(
        closure_llm::extract_ollama_response(body),
        Some("yo".to_owned())
    );
}

#[test]
fn extractor_handles_escaped_quotes() {
    let body = r#"{"content":"he said \"hi\""}"#;
    assert_eq!(
        closure_llm::extract_openai_content(body),
        Some("he said \"hi\"".to_owned())
    );
}
