//! Tests for the BYOK provider builders, request-body templates, the
//! crude response extractors, and json escaping — all pure, no network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{
    anthropic, extract_anthropic_content, extract_ollama_response, extract_openai_content,
    json_string, ollama, openai,
};

#[test]
fn anthropic_builder_sets_url_and_key_header() {
    let p = anthropic("sk-test", "claude-sonnet-4-6");
    assert_eq!(p.url, "https://api.anthropic.com/v1/messages");
    assert!(p.headers.iter().any(|h| h == "x-api-key: sk-test"));
    assert!(p.headers.iter().any(|h| h.contains("anthropic-version")));
    let body = (p.body)("hi", false);
    assert!(body.contains("\"messages\""));
    assert!(body.contains("\"content\":\"hi\""));
}

#[test]
fn openai_builder_sets_bearer_and_body() {
    let p = openai("sk-oa", "gpt-4o");
    assert_eq!(p.url, "https://api.openai.com/v1/chat/completions");
    assert!(p.headers.iter().any(|h| h == "authorization: Bearer sk-oa"));
    let body = (p.body)("yo", false);
    assert!(body.contains("\"model\":\"gpt-4o\""));
    assert!(body.contains("\"content\":\"yo\""));
}

#[test]
fn ollama_builder_uses_host_and_no_auth() {
    let p = ollama("http://localhost:11434", "llama3");
    assert_eq!(p.url, "http://localhost:11434/api/generate");
    assert!(
        !p.headers
            .iter()
            .any(|h| h.to_lowercase().contains("authorization"))
    );
    let body = (p.body)("q", false);
    assert!(body.contains("\"stream\":false"));
}

#[test]
fn extract_anthropic_reads_text_field() {
    let body = r#"{"content":[{"type":"text","text":"Hello there"}]}"#;
    assert_eq!(
        extract_anthropic_content(body).as_deref(),
        Some("Hello there")
    );
}

#[test]
fn extract_openai_reads_message_content() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":"the answer"}}]}"#;
    assert_eq!(extract_openai_content(body).as_deref(), Some("the answer"));
}

#[test]
fn extract_ollama_reads_response_field() {
    let body = r#"{"model":"llama3","response":"local reply","done":true}"#;
    assert_eq!(
        extract_ollama_response(body).as_deref(),
        Some("local reply")
    );
}

#[test]
fn extractors_unescape_json_strings() {
    let body = r#"{"text":"line one\nline \"two\""}"#;
    assert_eq!(
        extract_anthropic_content(body).as_deref(),
        Some("line one\nline \"two\"")
    );
}

#[test]
fn extractors_none_on_missing_field() {
    assert!(extract_anthropic_content("{}").is_none());
    assert!(extract_openai_content("not json").is_none());
}

#[test]
fn json_string_escapes_quotes_backslash_and_whitespace() {
    assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
    assert_eq!(json_string("tab\tnl\n"), r#""tab\tnl\n""#);
}

#[test]
fn json_string_roundtrips_through_the_extractor() {
    let original = "weird \"value\" with \\ and \n newline";
    let json = format!("{{\"text\":{}}}", json_string(original));
    assert_eq!(extract_anthropic_content(&json).as_deref(), Some(original));
}
