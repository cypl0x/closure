//! "[#C] MCP server" — finishing it.
//!
//! The bridge answers the seven methods a client asks for. What it had
//! not been tested against is a *client's* JSON: the arguments of a
//! `tools/call` live in `params.arguments`, not at the top level, and
//! a document scanned for a bare `"name"` finds whichever one comes
//! first — which in a real envelope is not always the tool's.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01MCPAAAAAAAAAAAAAAAAAAAAA\n:END:\nthe body\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

#[test]
fn a_tools_call_in_the_shape_a_client_sends_it() {
    // What an MCP client actually puts on the wire: the tool name and
    // its arguments are inside `params`, and the arguments are an
    // object under the key `arguments`.
    let (_d, mut v) = vault();
    let msg = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call",
        "params":{"name":"list-files","arguments":{}}}"#;
    let out = closure_mcp::handle_message(&mut v, msg).expect("a reply");
    assert!(out.contains("notes.org"), "the tool did not run: {out}");
}

#[test]
fn an_argument_reaches_the_tool() {
    let (_d, mut v) = vault();
    let msg = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call",
        "params":{"name":"read","arguments":{"args":"notes.org"}}}"#;
    let out = closure_mcp::handle_message(&mut v, msg).expect("a reply");
    assert!(out.contains("the body"), "the argument was dropped: {out}");
}

#[test]
fn a_failing_tool_is_marked_as_an_error() {
    // MCP has a place to say "this went wrong" — `isError` on the
    // result — and a client that cannot tell success from failure will
    // feed the failure back to the model as an answer.
    let (_d, mut v) = vault();
    let msg = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call",
        "params":{"name":"read","arguments":{"args":"nope.org"}}}"#;
    let out = closure_mcp::handle_message(&mut v, msg).expect("a reply");
    assert!(
        out.contains("isError"),
        "a failed tool call reads as success: {out}"
    );
}

#[test]
fn ping_is_answered() {
    // The one method every MCP client uses to find out whether the
    // server is still there.
    let (_d, mut v) = vault();
    let msg = r#"{"jsonrpc":"2.0","id":10,"method":"ping"}"#;
    let out = closure_mcp::handle_message(&mut v, msg).expect("a reply");
    assert!(
        !out.contains("Method not found") && !out.contains("-32601"),
        "ping is unimplemented: {out}"
    );
}

#[test]
fn a_notification_gets_no_reply() {
    // `notifications/initialized` has no id, and JSON-RPC says a
    // notification is never answered. Answering one is a protocol
    // error the client will complain about.
    let (_d, mut v) = vault();
    let msg = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert!(closure_mcp::handle_message(&mut v, msg).is_none());
}

#[test]
fn a_resource_is_read_by_the_uri_the_listing_gave() {
    let (_d, mut v) = vault();
    let list = closure_mcp::handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#,
    )
    .expect("a listing");
    assert!(list.contains("notes.org"), "{list}");
    let read = closure_mcp::handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":2,"method":"resources/read",
            "params":{"uri":"closure://notes.org"}}"#,
    )
    .expect("a read");
    assert!(read.contains("the body"), "the uri did not resolve: {read}");
}
