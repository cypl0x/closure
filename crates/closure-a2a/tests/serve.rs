//! J2: A2A JSON-RPC serve loop over in-memory buffers (hermetic).
//! `task/delegate` routes through `delegate_task` -> `Vault::run_tool`
//! (I8).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_a2a::serve_jsonrpc;
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn initialize_card_and_delegate_over_buffers() {
    let (_d, mut v) = vault();
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"agent/card\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"task/delegate\",\"task\":\"capture Delegated todo\"}\n",
    );
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].contains("\"id\":1") && lines[0].contains("closure"));
    assert!(lines[1].contains("\"id\":2"));
    assert!(lines[2].contains("\"id\":3"));
    // I8: the delegated task ran through run_tool.
    assert!(v.find_by_title("Delegated todo").is_some(), "delegate changed the vault");
}

#[test]
fn unknown_method_returns_error() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"nope\"}\n";
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("-32601") && text.contains("\"id\":7"), "{text}");
}

#[test]
fn notification_without_id_silent() {
    let (_d, mut v) = vault();
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, b"{\"jsonrpc\":\"2.0\",\"method\":\"initialize\"}\n".as_slice(), &mut out)
        .expect("serve");
    assert!(out.is_empty());
}
