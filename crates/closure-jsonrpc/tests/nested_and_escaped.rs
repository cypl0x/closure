//! The hand-rolled JSON reader, on the inputs it will actually meet.
//!
//! This crate parses JSON by scanning rather than by building a tree,
//! which is a reasonable choice for a protocol with a known shape and a
//! dangerous one to leave untested: every function here tracks quoting
//! and nesting by hand, and the failure mode is not a parse error but a
//! value cut off at the wrong byte.
//!
//! `raw_value`, `array_items`, `request` and `notification` had no
//! tests of their own at all — they are exercised only through the LSP
//! and MCP crates, where a wrong answer shows up as a protocol bug
//! several layers from its cause.
//!
//! The cases that matter are the ones where a brace or a comma appears
//! *inside a string*. A scanner that counts them regardless will close
//! an object early, and the payload it hands back will look almost
//! right.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_jsonrpc::{
    array_items, error, json_escape, method_not_found, notification, raw_field, raw_value, request,
    response, string_field, string_value,
};

#[test]
fn a_nested_object_comes_back_whole() {
    let json = r#"{"id":1,"params":{"a":1,"b":{"c":2}},"x":9}"#;
    assert_eq!(
        raw_value(json, "params").as_deref(),
        Some(r#"{"a":1,"b":{"c":2}}"#)
    );
}

#[test]
fn a_nested_array_comes_back_whole() {
    let json = r#"{"items":[1,[2,3],{"k":4}],"after":0}"#;
    assert_eq!(
        raw_value(json, "items").as_deref(),
        Some(r#"[1,[2,3],{"k":4}]"#)
    );
}

#[test]
fn a_brace_inside_a_string_does_not_close_the_object() {
    // The defect this scanner exists to avoid. A counter that does not
    // know it is inside a string ends the value at the `}` in the text
    // and returns something that still parses.
    let json = r#"{"params":{"text":"a } brace and a { one"},"after":1}"#;
    assert_eq!(
        raw_value(json, "params").as_deref(),
        Some(r#"{"text":"a } brace and a { one"}"#)
    );
}

#[test]
fn an_escaped_quote_inside_a_string_does_not_end_it() {
    let json = r#"{"params":{"text":"he said \"hi\" and }"},"after":1}"#;
    assert_eq!(
        raw_value(json, "params").as_deref(),
        Some(r#"{"text":"he said \"hi\" and }"}"#)
    );
}

#[test]
fn a_bare_number_or_keyword_stops_at_the_delimiter() {
    let json = r#"{"n":42,"t":true,"z":null}"#;
    assert_eq!(raw_value(json, "n").as_deref(), Some("42"));
    assert_eq!(raw_value(json, "t").as_deref(), Some("true"));
    assert_eq!(raw_value(json, "z").as_deref(), Some("null"));
}

#[test]
fn a_string_value_comes_back_quoted_and_re_escaped() {
    let json = r#"{"s":"with \"quotes\" in it"}"#;
    let v = raw_value(json, "s").expect("a value");
    assert!(v.starts_with('"') && v.ends_with('"'), "{v}");
    assert!(v.contains("\\\""), "the quotes were not re-escaped: {v}");
}

#[test]
fn a_key_that_is_not_there_is_none_rather_than_a_guess() {
    let json = r#"{"a":1}"#;
    assert_eq!(raw_value(json, "b"), None);
    assert_eq!(raw_field(json, "b"), None);
    assert_eq!(string_field(json, "b"), None);
}

#[test]
fn an_object_that_never_closes_is_none() {
    // Truncated input is what a half-written socket write looks like,
    // and returning the fragment as if it were complete is how a
    // protocol bug becomes a data bug.
    let json = r#"{"params":{"a":1"#;
    assert_eq!(raw_value(json, "params"), None);
}

#[test]
fn array_items_splits_only_at_the_top_level() {
    let items = array_items(r#"[1,[2,3],{"k":4},"a,b"]"#);
    assert_eq!(items, vec!["1", "[2,3]", r#"{"k":4}"#, r#""a,b""#]);
}

#[test]
fn array_items_of_an_empty_array_is_empty() {
    assert!(array_items("[]").is_empty());
    assert!(array_items("[   ]").is_empty());
}

#[test]
fn array_items_of_something_that_is_not_an_array_is_empty() {
    // Not a panic and not a one-element list containing the whole
    // string, which is the tempting wrong answer.
    assert!(array_items(r#"{"a":1}"#).is_empty());
    assert!(array_items("").is_empty());
    assert!(array_items("plain text").is_empty());
}

#[test]
fn array_items_ignores_a_comma_inside_a_string() {
    let items = array_items(r#"["a,b","c"]"#);
    assert_eq!(items, vec![r#""a,b""#, r#""c""#]);
}

#[test]
fn array_items_ignores_an_escaped_quote() {
    let items = array_items(r#"["a\"b,c","d"]"#);
    assert_eq!(items, vec![r#""a\"b,c""#, r#""d""#]);
}

#[test]
fn array_items_drops_a_trailing_comma_rather_than_yielding_an_empty_item() {
    assert_eq!(array_items("[1,2,]"), vec!["1", "2"]);
}

#[test]
fn string_value_decodes_the_escapes_it_knows() {
    assert_eq!(string_value(r#""a\nb""#).as_deref(), Some("a\nb"));
    assert_eq!(string_value(r#""a\tb""#).as_deref(), Some("a\tb"));
    assert_eq!(string_value(r#""a\rb""#).as_deref(), Some("a\rb"));
    assert_eq!(string_value(r#""a\\b""#).as_deref(), Some("a\\b"));
    assert_eq!(string_value(r#""a\"b""#).as_deref(), Some("a\"b"));
}

#[test]
fn string_value_of_something_unterminated_is_none() {
    assert_eq!(string_value(r#""no closing quote"#), None);
    assert_eq!(string_value("not a string at all"), None);
    // A trailing backslash with nothing after it: the escape never
    // completes, which is the input a truncated write produces.
    assert_eq!(string_value(r#""abc\"#), None);
}

#[test]
fn escaping_is_the_inverse_of_reading() {
    // The property that matters: anything this crate writes, it can
    // read back. A mismatch here corrupts every message carrying an
    // unusual character.
    for raw in [
        "plain",
        "with \"quotes\"",
        "with \\ backslash",
        "with \n newline",
        "with \t tab",
        "with } brace and , comma",
    ] {
        let wire = format!("\"{}\"", json_escape(raw));
        assert_eq!(
            string_value(&wire).as_deref(),
            Some(raw),
            "round trip failed for {raw:?} (wire: {wire})"
        );
    }
}

#[test]
fn a_request_names_its_id_and_method_and_carries_its_params() {
    let msg = request(7, "textDocument/didOpen", r#"{"uri":"file:///x"}"#);
    assert_eq!(raw_value(&msg, "id").as_deref(), Some("7"));
    assert_eq!(
        string_field(&msg, "method").as_deref(),
        Some("textDocument/didOpen")
    );
    assert_eq!(
        raw_value(&msg, "params").as_deref(),
        Some(r#"{"uri":"file:///x"}"#)
    );
    assert!(msg.contains(r#""jsonrpc":"2.0""#), "{msg}");
}

#[test]
fn a_notification_has_no_id_at_all() {
    // The whole difference between the two. A notification with an id
    // makes the peer wait for a reply that never comes.
    let msg = notification("exit", "{}");
    assert_eq!(raw_value(&msg, "id"), None, "{msg}");
    assert_eq!(string_field(&msg, "method").as_deref(), Some("exit"));
}

#[test]
fn a_method_name_with_a_quote_in_it_survives_the_wire() {
    let msg = request(1, r#"odd"name"#, "{}");
    assert_eq!(string_field(&msg, "method").as_deref(), Some(r#"odd"name"#));
}

#[test]
fn a_response_carries_its_result_and_an_error_carries_its_code() {
    let ok = response("1", r#"{"ok":true}"#);
    assert_eq!(raw_value(&ok, "result").as_deref(), Some(r#"{"ok":true}"#));

    let bad = error("1", -32601, "no such method");
    assert_eq!(
        string_field(&bad, "message").as_deref(),
        Some("no such method")
    );
    assert_eq!(raw_value(&bad, "code").as_deref(), Some("-32601"));

    let nf = method_not_found("2");
    assert_eq!(raw_value(&nf, "code").as_deref(), Some("-32601"));
}

#[test]
fn a_key_scan_steps_over_escapes_in_earlier_strings() {
    // `find_key` walks string literals to know which quotes are
    // structural. A backslash in an *earlier* key or value has to be
    // stepped over, or the scanner loses track of where strings begin
    // and finds the wanted key inside one.
    let json = r#"{"a\"b":1,"target":2}"#;
    assert_eq!(raw_value(json, "target").as_deref(), Some("2"));

    let with_escape_in_value = r#"{"first":"back\\slash","target":3}"#;
    assert_eq!(
        raw_value(with_escape_in_value, "target").as_deref(),
        Some("3")
    );
}

#[test]
fn a_string_that_never_closes_ends_the_search_rather_than_running_off() {
    // Truncated input again, this time hitting the key scan. There is
    // no key to find because there is no complete string left.
    assert_eq!(raw_value(r#"{"unterminated : 1"#, "unterminated"), None);
    assert_eq!(raw_field(r#"{"a":1,"b":"open"#, "b"), None);
}

#[test]
fn a_key_with_no_value_after_it_is_none() {
    // `{"a":}` is malformed, and the honest answer is that there is no
    // value — not an empty string, which a caller would go on to use.
    assert_eq!(raw_field(r#"{"a":}"#, "a"), None);
    assert_eq!(raw_field(r#"{"a":,"b":1}"#, "a"), None);
}

#[test]
fn a_key_at_the_very_end_with_nothing_after_it_is_none() {
    assert_eq!(raw_value(r#"{"a":"#, "a"), None);
    assert_eq!(raw_field(r#"{"a":"#, "a"), None);
}

#[test]
fn array_items_skips_an_empty_slot_between_two_commas() {
    // `[1,,2]` is malformed. Yielding an empty item would hand the
    // caller a value that is not one; skipping it is the same choice
    // the trailing-comma case makes.
    assert_eq!(array_items("[1,,2]"), vec!["1", "2"]);
    assert_eq!(array_items("[,1]"), vec!["1"]);
}
