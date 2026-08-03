//! "Token Streaming" — asked for alongside the OpenAI/Anthropic
//! compatible APIs.
//!
//! Every provider sent `"stream": false` and waited for the whole
//! reply, so the assistant sat silent for however long the model took
//! and then printed everything at once. For a local 7B over Ollama
//! that is several seconds of a window that looks hung.
//!
//! The three wire formats disagree about almost everything, which is
//! why this is parsed rather than guessed:
//!
//! - OpenAI: SSE, `data: {"choices":[{"delta":{"content":"Hel"}}]}`,
//!   terminated by a literal `data: [DONE]`.
//! - Anthropic: SSE with named events; only `content_block_delta`
//!   carries text, in `delta.text`, and `message_start`/`ping` must not
//!   be mistaken for content.
//! - Ollama: not SSE at all — bare NDJSON, one object per line, text in
//!   `response`.
//!
//! A line that carries no token is not an error; it is most of the
//! stream.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::doc_markdown,
    missing_docs
)]

use closure_llm::{ProviderKind, stream_delta};

#[test]
fn an_openai_chunk_yields_its_token() {
    let line = r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#;
    assert_eq!(
        stream_delta(ProviderKind::OpenAi, line).as_deref(),
        Some("Hel")
    );
}

#[test]
fn the_openai_done_sentinel_is_not_a_token() {
    // Printing a literal `[DONE]` at the end of every answer is the
    // classic way to get this wrong.
    assert_eq!(stream_delta(ProviderKind::OpenAi, "data: [DONE]"), None);
}

#[test]
fn an_openai_role_chunk_carries_no_text() {
    // The opening chunk announces the role and has no content at all.
    let line = r#"data: {"choices":[{"index":0,"delta":{"role":"assistant"}}]}"#;
    assert_eq!(stream_delta(ProviderKind::OpenAi, line), None);
}

#[test]
fn an_anthropic_text_delta_yields_its_token() {
    let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#;
    assert_eq!(
        stream_delta(ProviderKind::Anthropic, line).as_deref(),
        Some("Hel")
    );
}

#[test]
fn anthropic_housekeeping_events_carry_no_text() {
    // `message_start` names the model and carries a `"text"`-free
    // payload; `ping` carries nothing. Neither is a token, and both
    // arrive on every single request.
    for line in [
        "event: message_start",
        r#"data: {"type":"message_start","message":{"id":"msg_1","model":"claude"}}"#,
        r#"data: {"type":"ping"}"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"data: {"type":"message_stop"}"#,
    ] {
        assert_eq!(
            stream_delta(ProviderKind::Anthropic, line),
            None,
            "treated as a token: {line}"
        );
    }
}

#[test]
fn an_ollama_line_yields_its_token() {
    // Not SSE: no `data:` prefix, one bare JSON object per line.
    let line = r#"{"model":"llama3","response":"Hel","done":false}"#;
    assert_eq!(
        stream_delta(ProviderKind::Ollama, line).as_deref(),
        Some("Hel")
    );
}

#[test]
fn the_ollama_final_line_carries_no_token() {
    let line = r#"{"model":"llama3","response":"","done":true}"#;
    assert_eq!(stream_delta(ProviderKind::Ollama, line), None);
}

#[test]
fn blank_lines_are_not_tokens() {
    // SSE separates events with them, so they outnumber the content.
    for kind in [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Ollama,
    ] {
        assert_eq!(stream_delta(kind, ""), None);
        assert_eq!(stream_delta(kind, "   "), None);
    }
}

#[test]
fn a_token_that_is_a_space_survives() {
    // The reason a delta of `" "` must not be filtered out as empty:
    // words would run together and the answer would be unreadable.
    let line = r#"data: {"choices":[{"delta":{"content":" "}}]}"#;
    assert_eq!(
        stream_delta(ProviderKind::OpenAi, line).as_deref(),
        Some(" ")
    );
}

#[test]
fn an_escaped_newline_arrives_as_a_newline() {
    // Models emit `\n` inside the JSON string; a stream that printed
    // the two characters would destroy every list and code block.
    let line = r#"data: {"choices":[{"delta":{"content":"a\nb"}}]}"#;
    assert_eq!(
        stream_delta(ProviderKind::OpenAi, line).as_deref(),
        Some("a\nb")
    );
}

#[test]
fn echo_has_nothing_to_stream() {
    assert_eq!(stream_delta(ProviderKind::Echo, "anything"), None);
}

#[test]
fn a_whole_openai_stream_reassembles_into_the_answer() {
    // The property that matters end to end: the concatenated tokens
    // are exactly what the non-streaming call would have returned.
    let body = "\
data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\
\n\
data: [DONE]\n";
    let joined: String = body
        .lines()
        .filter_map(|l| stream_delta(ProviderKind::OpenAi, l))
        .collect();
    assert_eq!(joined, "Hello there");
}

#[test]
fn a_whole_anthropic_stream_reassembles_into_the_answer() {
    let body = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n";
    let joined: String = body
        .lines()
        .filter_map(|l| stream_delta(ProviderKind::Anthropic, l))
        .collect();
    assert_eq!(joined, "Hello");
}
