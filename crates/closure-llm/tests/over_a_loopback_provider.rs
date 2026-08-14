//! `complete` and `stream` against a server on 127.0.0.1.
//!
//! The whole HTTP half of this crate was untested: it shells out to
//! curl, so every test so far used the Echo provider and stopped at the
//! boundary. But a loopback socket is hermetic — the same argument
//! `closure-sync`'s TCP tests and `closure-plugin-host`'s registry
//! fetch already make — so the wire format, the streaming loop and the
//! extraction can all be exercised without a network or a key.
//!
//! What this cannot check is whether the real providers still speak the
//! shapes assumed here; that is what `just llm-live` is for. What it
//! does check is everything between a prompt and a socket, which is
//! where this crate's own bugs would be.
//!
//! Streaming matters more than completion. A token dropped, doubled, or
//! run into the next one is a visibly wrong answer that no status code
//! reports.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread::JoinHandle;

use closure_llm::{Provider, ProviderKind, anthropic_at, openai_at, stream_delta};

fn curl_available() -> bool {
    std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Answer one request with `body`, then close.
fn serve_once(listener: TcpListener, body: String) -> JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    })
}

fn addr_and_server(body: String) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let h = serve_once(listener, body);
    (format!("http://{addr}/v1/messages"), h)
}

// === stream_delta, which needs no socket at all ===

#[test]
fn an_anthropic_delta_is_taken_only_from_a_content_block_delta() {
    // The comment in the source explains the trap: other events carry
    // text-shaped fields, and one leaking would put the model's own id
    // into the answer.
    let good = r#"data: {"type":"content_block_delta","delta":{"text":"hello"}}"#;
    assert_eq!(
        stream_delta(ProviderKind::Anthropic, good).as_deref(),
        Some("hello")
    );

    let other_event = r#"data: {"type":"message_start","message":{"text":"not the answer"}}"#;
    assert_eq!(stream_delta(ProviderKind::Anthropic, other_event), None);
}

#[test]
fn a_space_is_a_token_and_an_empty_delta_is_not() {
    // Stated in the source: a delta of `""` is a frame with no token,
    // and a delta of `" "` is a real space — dropping it runs every
    // word into the next one.
    let space = r#"data: {"type":"content_block_delta","delta":{"text":" "}}"#;
    assert_eq!(
        stream_delta(ProviderKind::Anthropic, space).as_deref(),
        Some(" ")
    );
    let empty = r#"data: {"type":"content_block_delta","delta":{"text":""}}"#;
    assert_eq!(stream_delta(ProviderKind::Anthropic, empty), None);
}

#[test]
fn the_frames_that_carry_no_token_are_ignored() {
    for line in ["", "   ", "event: message_start", "data: [DONE]"] {
        assert_eq!(
            stream_delta(ProviderKind::Anthropic, line),
            None,
            "{line:?} produced a token"
        );
        assert_eq!(stream_delta(ProviderKind::OpenAi, line), None, "{line:?}");
    }
}

#[test]
fn openai_reads_content_and_ollama_reads_bare_json() {
    let openai = r#"data: {"choices":[{"delta":{"content":"tok"}}]}"#;
    assert_eq!(
        stream_delta(ProviderKind::OpenAi, openai).as_deref(),
        Some("tok")
    );
    // Ollama sends bare JSON lines, not SSE frames.
    let ollama = r#"{"response":"tok","done":false}"#;
    assert_eq!(
        stream_delta(ProviderKind::Ollama, ollama).as_deref(),
        Some("tok")
    );
    // And the SSE prefix is required for the others: a bare line is
    // not a frame.
    assert_eq!(
        stream_delta(ProviderKind::OpenAi, r#"{"choices":[]}"#),
        None
    );
}

#[test]
fn the_echo_provider_streams_nothing_because_nothing_leaves_the_process() {
    for line in [r#"data: {"delta":{"text":"x"}}"#, "anything"] {
        assert_eq!(stream_delta(ProviderKind::Echo, line), None);
        assert_eq!(stream_delta(ProviderKind::Unknown, line), None);
    }
}

// === over a real socket ===

#[test]
fn a_completion_comes_back_from_the_wire() {
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let body = r#"{"content":[{"type":"text","text":"the whole answer"}]}"#.to_owned();
    let (url, h) = addr_and_server(body);
    let p = anthropic_at(&url, "not-a-real-key", "test-model");
    let out = p.complete("a question").expect("complete");
    h.join().expect("join");
    assert!(out.contains("the whole answer"), "{out}");
}

#[test]
fn a_stream_arrives_token_by_token_and_adds_up_to_the_whole() {
    // Both halves matter: the caller sees each token as it lands, and
    // the return value is the concatenation. A stream that delivered
    // tokens but returned nothing would look right on screen and be
    // empty to anything that used the result.
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\"}\n\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\" \"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"world\"}}\n\n",
        "data: [DONE]\n\n",
    )
    .to_owned();
    let (url, h) = addr_and_server(body);
    let p = anthropic_at(&url, "not-a-real-key", "test-model");

    let mut seen: Vec<String> = Vec::new();
    let whole = p
        .stream("a question", &mut |t| seen.push(t.to_owned()))
        .expect("stream");
    h.join().expect("join");

    assert_eq!(seen, vec!["Hello", " ", "world"], "tokens arrived wrong");
    assert_eq!(whole, "Hello world", "the return value is not the sum");
}

#[test]
fn an_openai_stream_is_read_with_the_openai_shape() {
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"one\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" two\"}}]}\n\n",
        "data: [DONE]\n\n",
    )
    .to_owned();
    let (url, h) = addr_and_server(body);
    let p = openai_at(&url, "not-a-real-key", "test-model");

    let mut seen = String::new();
    let whole = p
        .stream("a question", &mut |t| seen.push_str(t))
        .expect("stream");
    h.join().expect("join");
    assert_eq!(seen, "one two");
    assert_eq!(whole, "one two");
}

#[test]
fn a_server_that_is_not_there_is_an_error_not_a_hang() {
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    // Bind and drop, so the port is closed and nothing is listening.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);

    let p = anthropic_at(&format!("http://{addr}/v1/messages"), "k", "m");
    assert!(
        p.complete("a question").is_err(),
        "a closed port produced an answer"
    );
}

#[test]
fn a_reply_that_is_not_the_expected_shape_is_an_error() {
    // A proxy returning an HTML error page is the ordinary way this
    // happens, and treating the page as an answer would put HTML into
    // somebody's notes.
    if !curl_available() {
        eprintln!("skipped: no curl on this host");
        return;
    }
    let (url, h) = addr_and_server("<html>gateway timeout</html>".to_owned());
    let p = anthropic_at(&url, "k", "m");
    let got = p.complete("a question");
    h.join().expect("join");
    assert!(got.is_err(), "an HTML error page was accepted: {got:?}");
}
