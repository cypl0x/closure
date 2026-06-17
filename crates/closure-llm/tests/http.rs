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
    assert!(seen.contains("ping"), "server saw the posted body: {seen:?}");
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
    assert!(provider.complete("x").is_err(), "refused -> error, no panic");
}
