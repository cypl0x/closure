//! Shared JSON helper + envelope tests (hermetic, no deps).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_jsonrpc::{
    error, json_escape, method_not_found, raw_field, response, string_field, string_value,
};

#[test]
fn raw_field_reads_numeric_and_string_ids() {
    assert_eq!(
        raw_field(r#"{"jsonrpc":"2.0","id":7,"method":"x"}"#, "id").as_deref(),
        Some("7")
    );
    // A string id round-trips re-quoted.
    assert_eq!(
        raw_field(r#"{"id":"abc","m":1}"#, "id").as_deref(),
        Some("\"abc\"")
    );
    assert_eq!(raw_field(r#"{"method":"x"}"#, "id"), None);
}

#[test]
fn string_field_reads_and_unescapes() {
    assert_eq!(
        string_field(r#"{"method":"tools/call"}"#, "method").as_deref(),
        Some("tools/call")
    );
    assert_eq!(
        string_field(r#"{"name":"a\"b\nc"}"#, "name").as_deref(),
        Some("a\"b\nc")
    );
    assert_eq!(
        string_field(r#"{"id":3}"#, "id"),
        None,
        "non-string -> None"
    );
}

#[test]
fn string_value_stops_at_closing_quote() {
    assert_eq!(string_value(r#""hi" rest"#).as_deref(), Some("hi"));
    assert_eq!(string_value(r#""unterminated"#), None);
}

#[test]
fn json_escape_handles_quotes_backslash_whitespace() {
    assert_eq!(json_escape("a\"b\\c\nd\te\rf"), "a\\\"b\\\\c\\nd\\te\\rf");
}

#[test]
fn escape_then_value_round_trips() {
    let original = "line1\nq\"uote\\slash\ttab";
    let literal = format!("\"{}\"", json_escape(original));
    assert_eq!(string_value(&literal).as_deref(), Some(original));
}

#[test]
fn response_envelope_embeds_id_and_result() {
    assert_eq!(
        response("7", "{\"ok\":true}"),
        r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#
    );
    // String id stays quoted.
    assert_eq!(
        response("\"abc\"", "null"),
        r#"{"jsonrpc":"2.0","id":"abc","result":null}"#
    );
}

#[test]
fn error_and_method_not_found_envelopes() {
    assert_eq!(
        error("1", -32602, "bad params"),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"bad params"}}"#
    );
    assert_eq!(
        method_not_found("9"),
        r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32601,"message":"method not found"}}"#
    );
}
