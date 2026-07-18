//! Q7-L2: the opt-in LIVE gate (`just llm-live`) — a real end-to-end
//! ask against a local Ollama daemon. Hermetic runs skip: the test
//! only engages when `CLOSURE_LLM_LIVE=1`, and skips gracefully when
//! no daemon answers on localhost (the iroh-gate pattern).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::Provider as _;

#[test]
fn live_ollama_roundtrip_or_graceful_skip() {
    if std::env::var("CLOSURE_LLM_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set CLOSURE_LLM_LIVE=1 (just llm-live) to run");
        return;
    }
    // Daemon reachable?
    if std::net::TcpStream::connect(("127.0.0.1", 11434)).is_err() {
        eprintln!("skipped: no Ollama daemon on 127.0.0.1:11434");
        return;
    }
    let model = std::env::var("CLOSURE_LLM_MODEL").unwrap_or_else(|_| "llama3".to_owned());
    let provider = closure_llm::ollama_http("http://127.0.0.1:11434", &model);
    let out = provider
        .complete("Reply with exactly the word: pong")
        .expect("live completion");
    assert!(!out.trim().is_empty(), "non-empty live answer: {out:?}");
    eprintln!("live answer: {out}");
}
