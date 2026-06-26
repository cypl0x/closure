//! D4: a hermetic mock of the `OpenAI` chat-completions *wire* — it speaks
//! the real protocol (dep-free JSON request encode + response decode) with
//! no curl and no socket, so the tool loop behaves exactly as it would
//! against a live `OpenAI` endpoint, deterministically (I6).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{OpenAiWireProvider, Provider, extract_openai_content, openai_response_json};

#[test]
fn provider_encodes_an_openai_request_body() {
    let p = OpenAiWireProvider::scripted(&["hi there"]);
    let _ = p.complete("summarise my notes").expect("complete");
    let req = p.last_request();
    // A genuine OpenAI chat-completions request: model + a user message
    // carrying the prompt, JSON-encoded.
    assert!(req.contains("\"messages\""), "openai request shape: {req}");
    assert!(req.contains("\"role\":\"user\""), "user turn: {req}");
    assert!(
        req.contains("summarise my notes"),
        "prompt is the user content: {req}"
    );
}

#[test]
fn provider_returns_scripted_assistant_content() {
    let p = OpenAiWireProvider::scripted(&["first", "second"]);
    assert_eq!(p.complete("a").unwrap(), "first");
    assert_eq!(p.complete("b").unwrap(), "second");
    assert!(p.complete("c").is_err(), "script exhausted -> error");
}

#[test]
fn response_envelope_round_trips_arbitrary_content() {
    // openai_response_json is the inverse of extract_openai_content; the
    // dep-free JSON survives quotes / newlines (escaping correctness).
    for content in ["plain", "has \"quotes\"", "two\nlines", "tab\there"] {
        let body = openai_response_json(content);
        assert_eq!(
            extract_openai_content(&body).as_deref(),
            Some(content),
            "round trip for {content:?}"
        );
    }
}
