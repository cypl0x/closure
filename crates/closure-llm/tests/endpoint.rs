//! Located defect: "`closure_llm::openai()` hardcodes the URL;
//! `llm_endpoint` is used only as the Ollama host."
//!
//! So `llm_endpoint` was a key config *validated* and then ignored:
//! setting `llm_provider = openai-compatible` makes the loader insist
//! on an endpoint, and nothing ever read it. Point closure at
//! `llama.cpp`, `vLLM`, `LiteLLM`, or a company's own gateway — all of which
//! speak `/v1/chat/completions` — and it talked to `api.openai.com`
//! anyway.
//!
//! The endpoint belongs to every provider that has a URL, which is all
//! of them. It is also what makes the stack testable without a key: a
//! local stub answering `/v1/chat/completions` is a whole provider
//! round-trip with nothing secret in it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_llm::{ProviderKind, build_provider_at, openai_at};

#[test]
fn an_openai_provider_can_be_pointed_somewhere_else() {
    let p = openai_at("http://127.0.0.1:8080/v1/chat/completions", "k", "m");
    assert_eq!(p.url, "http://127.0.0.1:8080/v1/chat/completions");
}

#[test]
fn the_default_is_still_openai() {
    // Nobody who has not set an endpoint should notice this change.
    let p = closure_llm::openai("k", "m");
    assert_eq!(p.url, "https://api.openai.com/v1/chat/completions");
}

#[test]
fn the_endpoint_reaches_an_openai_compatible_build() {
    let p = build_provider_at(
        ProviderKind::OpenAi,
        "m",
        Some("http://127.0.0.1:9099/v1/chat/completions"),
        "k",
    );
    assert!(
        p.endpoint().unwrap_or_default().contains("127.0.0.1:9099"),
        "went to {:?}",
        p.endpoint()
    );
}

#[test]
fn the_endpoint_reaches_an_anthropic_build_too() {
    // A gateway in front of Anthropic is the same shape of need; the
    // defect named openai because that is where it was noticed.
    let p = build_provider_at(
        ProviderKind::Anthropic,
        "m",
        Some("http://127.0.0.1:9099/v1/messages"),
        "k",
    );
    assert!(
        p.endpoint().unwrap_or_default().contains("127.0.0.1:9099"),
        "{:?}",
        p.endpoint()
    );
}

#[test]
fn ollama_still_takes_its_host() {
    // The one provider the endpoint already reached, unchanged.
    let p = build_provider_at(
        ProviderKind::Ollama,
        "m",
        Some("http://127.0.0.1:11500"),
        "",
    );
    assert!(
        p.endpoint().unwrap_or_default().contains("127.0.0.1:11500"),
        "{:?}",
        p.endpoint()
    );
}

#[test]
fn no_endpoint_means_the_providers_own_default() {
    let p = build_provider_at(ProviderKind::OpenAi, "m", None, "k");
    assert!(
        p.endpoint().unwrap_or_default().contains("api.openai.com"),
        "{:?}",
        p.endpoint()
    );
}

#[test]
fn echo_has_no_endpoint_to_report() {
    // It never leaves the process, and claiming a URL would be a lie
    // a status line would repeat.
    let p = build_provider_at(ProviderKind::Echo, "m", None, "");
    assert!(p.endpoint().is_none(), "{:?}", p.endpoint());
}

#[test]
fn a_bare_host_for_an_openai_endpoint_still_reaches_the_api() {
    // People write `llm_endpoint = http://localhost:8080` as often as
    // the full path; guessing wrong there is a connection refused with
    // no explanation.
    let p = openai_at("http://localhost:8080", "k", "m");
    assert_eq!(p.url, "http://localhost:8080/v1/chat/completions");
}
