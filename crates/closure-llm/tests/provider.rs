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
