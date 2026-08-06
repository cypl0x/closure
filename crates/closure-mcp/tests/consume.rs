//! "Consume MCP Servers (?) — Good fit with the LLM. 'closure Agent'
//! harness."
//!
//! closure already *is* an MCP server: an agent connects to it and
//! reads the vault. This is the other direction — closure as the
//! client, so a server someone else wrote (a filesystem, a browser, an
//! issue tracker) shows up as tools the assistant can call, on the
//! same line-delimited JSON-RPC the server half already speaks.
//!
//! Tested over a scripted transport rather than a real child process:
//! the wire is the part that can be wrong, and a test that spawns
//! `npx` is a test that fails on a machine with no network.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::io::Cursor;

/// A server that answers three requests in order, the way a real one
/// does over stdio.
fn scripted() -> Cursor<Vec<u8>> {
    let lines = [
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"files","version":"0.1"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"read_file","description":"Read a file from disk"},{"name":"write_file","description":"Write a file to disk"}]}}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"hello from the server"}]}}"#,
    ];
    Cursor::new(lines.join("\n").into_bytes())
}

#[test]
fn the_handshake_goes_out_before_anything_is_asked_for() {
    let mut sent = Vec::new();
    let conn = closure_mcp::Conn::connect("files", scripted(), &mut sent).expect("connect");
    assert_eq!(conn.server_name(), "files");
    let out = String::from_utf8(sent).unwrap();
    assert!(
        out.contains("\"method\":\"initialize\""),
        "no initialize went out:\n{out}"
    );
    assert!(
        out.contains("\"method\":\"notifications/initialized\""),
        "the server was never told the handshake finished:\n{out}"
    );
}

#[test]
fn its_tools_come_back_named_and_described() {
    let mut sent = Vec::new();
    let mut conn = closure_mcp::Conn::connect("files", scripted(), &mut sent).expect("connect");
    let tools = conn.tools().expect("tools/list");
    assert_eq!(tools.len(), 2, "{tools:?}");
    assert_eq!(tools[0].name, "files/read_file", "not qualified by server");
    assert_eq!(tools[0].description, "Read a file from disk");
    assert_eq!(tools[1].name, "files/write_file");
}

#[test]
fn calling_one_returns_the_text_the_server_sent() {
    let mut sent = Vec::new();
    let mut conn = closure_mcp::Conn::connect("files", scripted(), &mut sent).expect("connect");
    let _ = conn.tools().expect("tools/list");
    let out = conn
        .call("read_file", r#"{"path":"notes.org"}"#)
        .expect("tools/call");
    assert_eq!(out, "hello from the server");
    let wire = String::from_utf8(sent).unwrap();
    assert!(
        wire.contains("\"method\":\"tools/call\"") && wire.contains("\"name\":\"read_file\""),
        "the call did not name the tool:\n{wire}"
    );
}

#[test]
fn an_error_from_the_server_is_an_error_here() {
    let reply = concat!(
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"no such method"}}"#,
    );
    let mut sent = Vec::new();
    let mut conn =
        closure_mcp::Conn::connect("files", Cursor::new(reply.as_bytes().to_vec()), &mut sent)
            .expect("connect");
    let err = conn.tools().expect_err("an error reply is not a tool list");
    assert!(
        err.to_string().contains("no such method"),
        "the server's own words were dropped: {err}"
    );
}

#[test]
fn a_server_that_says_nothing_is_a_transport_error_not_a_hang() {
    let mut sent = Vec::new();
    let got = closure_mcp::Conn::connect("silent", Cursor::new(Vec::new()), &mut sent);
    assert!(got.is_err(), "a server that closed its pipe looked fine");
}
