//! MCP JSON-RPC subset over the vault tool surface: initialize,
//! tools/list (with schemas), tools/call. Mutations go through
//! `Vault::run_tool`, i.e. kernel commands only (I8).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_mcp::handle_message;
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* TODO Ship parser\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn initialize_reports_server_info() {
    let (_td, mut v) = vault();
    let r = handle_message(&mut v, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
        .expect("response");
    assert!(r.contains("\"id\":1"));
    assert!(r.contains("closure"));
    assert!(r.contains("\"result\""));
}

#[test]
fn tools_list_names_every_tool_with_schema() {
    let (_td, mut v) = vault();
    let r = handle_message(&mut v, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
        .expect("response");
    for tool in [
        "list-files",
        "read",
        "search",
        "capture",
        "rename",
        "set-property",
    ] {
        assert!(r.contains(tool), "missing tool {tool}");
    }
    assert!(r.contains("inputSchema"));
}

#[test]
fn tools_call_executes_and_returns_text() {
    let (_td, mut v) = vault();
    let r = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search","arguments":{"args":"ship"}}}"#,
    )
    .expect("response");
    assert!(r.contains("Ship parser"));
    assert!(r.contains("\"id\":3"));
}

#[test]
fn tools_call_capture_mutates_vault() {
    let (_td, mut v) = vault();
    let _ = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"capture","arguments":{"args":"Buy milk"}}}"#,
    )
    .expect("response");
    assert!(v.find_by_title("Buy milk").is_some());
}

#[test]
fn unknown_method_is_method_not_found() {
    let (_td, mut v) = vault();
    let r =
        handle_message(&mut v, r#"{"jsonrpc":"2.0","id":5,"method":"nope"}"#).expect("response");
    assert!(r.contains("-32601"));
    assert!(r.contains("\"error\""));
}

#[test]
fn notification_without_id_gets_no_response() {
    let (_td, mut v) = vault();
    let r = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    assert!(r.is_none());
}

#[test]
fn response_text_is_json_escaped() {
    let (_td, mut v) = vault();
    let _ = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"capture","arguments":{"args":"Say \"hi\""}}}"#,
    );
    let r = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"read","arguments":{"args":"inbox.org"}}}"#,
    )
    .expect("response");
    assert!(r.contains("\\\"hi\\\""), "quotes escaped in payload: {r}");
}
