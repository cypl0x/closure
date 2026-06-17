//! J1: ACP JSON-RPC serve loop over in-memory buffers (hermetic). One
//! request per line; tools/call routes through `Vault::run_tool` (I8).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_acp::serve_jsonrpc;
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn serves_initialize_card_and_tool_call_over_buffers() {
    let (_d, mut v) = vault();
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"agent/card\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"name\":\"capture\",\"args\":\"Buy milk\"}\n",
    );
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "one response per request: {text}");
    assert!(lines[0].contains("\"id\":1") && lines[0].contains("closure"));
    assert!(
        lines[1].contains("\"id\":2") && lines[1].contains("\"tools\""),
        "card: {}",
        lines[1]
    );
    assert!(lines[2].contains("\"id\":3"));
    // I8: the tools/call actually ran through the registry.
    assert!(
        v.find_by_title("Buy milk").is_some(),
        "capture changed the vault"
    );
}

#[test]
fn notification_without_id_gets_no_response() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\"}\n"; // no id
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    assert!(out.is_empty(), "notification (no id) -> no response");
}

#[test]
fn unknown_method_returns_error() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"bogus\"}\n";
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("-32601") && text.contains("\"id\":9"),
        "method-not-found: {text}"
    );
}
