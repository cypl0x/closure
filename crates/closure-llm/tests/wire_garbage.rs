//! What the providers do with a response that is not what they expect.
//!
//! Every function here is pure — a string in, an option out — so none
//! of it needs a network. The uncovered lines in this crate are the
//! arms that run when a provider answers with something unexpected,
//! and those are the ones that run in production: a rate-limit page, a
//! proxy's HTML error, a truncated stream, a 200 with an error body.
//!
//! The claim is that a bad response is `None` and never a panic. A
//! model provider having a bad day must not take the notebook with it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_llm::{
    ProviderKind, extract_anthropic_content, extract_ollama_response, extract_openai_content,
    json_string, stream_delta,
};

/// Bodies a provider has actually been known to return.
const GARBAGE: &[(&str, &str)] = &[
    ("empty", ""),
    ("whitespace", "   \n  "),
    ("not json", "Bad Gateway"),
    (
        "html error page",
        "<html><body>502 Bad Gateway</body></html>",
    ),
    ("truncated json", "{\"choices\":[{\"message\":"),
    ("json array", "[]"),
    ("json null", "null"),
    ("json number", "42"),
    ("empty object", "{}"),
    (
        "error object",
        "{\"error\":{\"message\":\"rate limited\",\"type\":\"rate_limit\"}}",
    ),
    (
        "right shape, empty content",
        "{\"choices\":[{\"message\":{\"content\":\"\"}}]}",
    ),
    ("right shape, no choices", "{\"choices\":[]}"),
    (
        "content is a number",
        "{\"choices\":[{\"message\":{\"content\":7}}]}",
    ),
    (
        "content is an object",
        "{\"choices\":[{\"message\":{\"content\":{}}}]}",
    ),
    (
        "deeply nested",
        "{\"a\":{\"a\":{\"a\":{\"a\":{\"a\":{\"a\":1}}}}}}",
    ),
    (
        "unicode escapes",
        "{\"choices\":[{\"message\":{\"content\":\"\\u00e9\\u4e2d\"}}]}",
    ),
    (
        "very long",
        "{\"choices\":[{\"message\":{\"content\":\"xxxxxxxxxx\"}}]}",
    ),
];

#[test]
fn no_response_body_makes_an_extractor_panic() {
    for (name, body) in GARBAGE {
        let _ = extract_openai_content(body);
        let _ = extract_anthropic_content(body);
        let _ = extract_ollama_response(body);
        assert!(!name.is_empty());
    }
}

#[test]
fn an_extractor_returns_none_rather_than_an_empty_answer() {
    // The distinction that matters to a caller: `None` is "the provider
    // did not answer", and `Some("")` is "the model said nothing".
    // Reporting the first as the second makes a failed request look
    // like a terse model.
    for (name, body) in GARBAGE {
        if *name == "right shape, empty content" {
            continue;
        }
        assert!(
            extract_openai_content(body) != Some(String::new()),
            "`{name}` came back as an empty answer rather than no answer"
        );
    }
}

#[test]
fn a_well_formed_response_still_extracts() {
    // The control. An extractor that returned `None` for everything
    // would pass every assertion above.
    let body = "{\"choices\":[{\"message\":{\"content\":\"hello\"}}]}";
    assert_eq!(extract_openai_content(body).as_deref(), Some("hello"));
}

#[test]
fn no_stream_line_makes_a_delta_panic() {
    // Streams arrive a line at a time and get cut off mid-frame, so
    // half a JSON object is the ordinary case rather than the strange
    // one.
    let lines = [
        "",
        "data:",
        "data: ",
        "data: [DONE]",
        "data: {",
        "data: {\"choices\":[{\"delta\":{}}]}",
        "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
        "event: ping",
        ": a comment",
        "garbage",
        "data: null",
    ];
    for kind in [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Ollama,
        ProviderKind::Echo,
    ] {
        for line in lines {
            let _ = stream_delta(kind, line);
        }
    }
}

#[test]
fn a_stream_delta_comes_through_when_there_is_one() {
    let got = stream_delta(
        ProviderKind::OpenAi,
        "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
    );
    assert_eq!(got.as_deref(), Some("hi"));
}

#[test]
fn json_string_escapes_what_would_break_a_request() {
    // This builds the body closure sends. A prompt containing a quote
    // or a newline must not end up as two JSON fields, and a prompt
    // containing a backslash must not eat the character after it.
    for (raw, must_contain) in [
        ("say \"hi\"", "\\\""),
        ("line\nbreak", "\\n"),
        ("back\\slash", "\\\\"),
        ("tab\there", "\\t"),
    ] {
        let out = json_string(raw);
        assert!(out.contains(must_contain), "`{raw}` was not escaped: {out}");
    }
}

#[test]
fn a_prompt_with_a_control_character_is_still_valid_json() {
    // Somebody pastes a note with a NUL or a bell in it, and the
    // request has to remain parseable rather than silently truncating.
    let out = json_string("a\u{0}b\u{7}c");
    assert!(out.starts_with('"') && out.ends_with('"'), "{out}");
    assert!(
        !out.contains('\u{0}'),
        "a NUL went into the body raw: {out}"
    );
}
