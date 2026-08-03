//! L1: a self-contained std HTTP/1.1 client (`HttpProvider`), tested
//! against an in-process mock HTTP server over `127.0.0.1` loopback —
//! no TLS, no external network, no API key, no deps. Fully hermetic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread;

use closure_llm::{HttpProvider, LlmError, Provider};

/// Serve one HTTP request, capture its body, and reply with `status` +
/// `resp_body`. Returns the captured request body.
fn serve_once(listener: TcpListener, status: &str, resp_body: &str) -> thread::JoinHandle<String> {
    let status = status.to_owned();
    let resp_body = resp_body.to_owned();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        // Read headers + body (best-effort: read what's available).
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).expect("read");
        let req = String::from_utf8_lossy(&buf[..n]).into_owned();
        let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
            resp_body.len()
        );
        s.write_all(resp.as_bytes()).expect("write");
        body
    })
}

#[test]
fn posts_body_and_returns_response_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = serve_once(listener, "200 OK", "pong");

    let provider = HttpProvider::new(format!("http://{addr}/api"));
    let got = provider.complete("ping").expect("complete");
    assert_eq!(got, "pong", "provider returns the response body");

    let seen = handle.join().expect("join");
    assert!(
        seen.contains("ping"),
        "server saw the posted body: {seen:?}"
    );
}

#[test]
fn non_200_status_errors_without_panic() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let _h = serve_once(listener, "500 Internal Server Error", "boom");
    let provider = HttpProvider::new(format!("http://{addr}/api"));
    assert!(matches!(provider.complete("x"), Err(LlmError::Provider(_))));
}

#[test]
fn connection_refused_errors_without_panic() {
    // Port 1 is not listening; connect must fail cleanly.
    let provider = HttpProvider::new("http://127.0.0.1:1/api".to_owned());
    assert!(
        provider.complete("x").is_err(),
        "refused -> error, no panic"
    );
}

// === L2: Ollama over the real HTTP client, against the mock server. ===

#[test]
fn ollama_http_sends_model_and_prompt_and_extracts_response() {
    use closure_llm::ollama_http;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = serve_once(listener, "200 OK", r#"{"response":"hi there"}"#);

    let provider = ollama_http(&format!("http://{addr}"), "llama3");
    let out = provider.complete("summarize my notes").expect("complete");
    assert_eq!(out, "hi there", "extracts the Ollama response field");

    let body = handle.join().expect("join");
    assert!(body.contains("llama3"), "request carries the model: {body}");
    assert!(
        body.contains("summarize my notes"),
        "request carries the prompt: {body}"
    );
    assert!(body.contains("\"stream\":false"), "non-streaming request");
}

// === Q7-L1: the model is a real parameter, not a constant. ===

#[test]
fn ollama_http_honours_the_model_parameter() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = serve_once(listener, "200 OK", r#"{"response":"ok"}"#);
    let provider = closure_llm::ollama_http(&format!("http://{addr}"), "mistral-nemo");
    provider.complete("hello").expect("complete");
    let body = handle.join().expect("join");
    assert!(
        body.contains("\"model\":\"mistral-nemo\""),
        "per-model body: {body}"
    );
    assert!(!body.contains("llama3"), "no hardcoded default: {body}");
}

#[test]
fn anthropic_and_openai_bodies_carry_the_model() {
    let a = closure_llm::anthropic("test-key", "claude-fable-5");
    let body = (a.body)("hi", false);
    assert!(body.contains("\"model\":\"claude-fable-5\""), "{body}");
    let o = closure_llm::openai("test-key", "gpt-5-mini");
    let body = (o.body)("hi", false);
    assert!(body.contains("\"model\":\"gpt-5-mini\""), "{body}");
}
