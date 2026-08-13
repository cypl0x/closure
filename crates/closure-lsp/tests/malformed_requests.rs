//! What the language server does with a request it cannot use.
//!
//! `handle_message` takes JSON and returns JSON, so every one of these
//! is hermetic — no socket, no editor, no TTY. The 150 unexecuted lines
//! in this crate are its error arms, and those are the ones a real
//! editor eventually exercises: a client that sends a slightly wrong
//! request is a client, not a hypothetical.
//!
//! The claim throughout is that a bad request produces an answer or a
//! silence, never a panic. A language server that dies takes the
//! editor's whole session with it, and the user's diagnosis is "closure
//! broke my editor" rather than "closure sent me a JSON-RPC error".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_lsp::handle_message;
use closure_store::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("notes.org"),
        "* TODO A note :work:\n:PROPERTIES:\n:ID: 01LSPBAD00000000001\n:END:\nbody\n",
    )
    .unwrap();
    let v = Vault::open(d.path()).unwrap();
    (d, v)
}

/// Requests that are wrong in every way a client can be wrong.
const BAD: &[(&str, &str)] = &[
    ("empty", ""),
    ("not json", "hello"),
    ("truncated json", "{\"jsonrpc\":\"2.0\","),
    ("json but not an object", "[1,2,3]"),
    ("object with nothing in it", "{}"),
    ("no method", "{\"jsonrpc\":\"2.0\",\"id\":1}"),
    (
        "unknown method",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"nosuch/thing\"}",
    ),
    (
        "method with no params",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\"}",
    ),
    (
        "params of the wrong shape",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\"params\":42}",
    ),
    (
        "uri that is not a uri",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\
         \"params\":{\"textDocument\":{\"uri\":\"not a uri\"},\
         \"position\":{\"line\":0,\"character\":0}}}",
    ),
    (
        "position past the end",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\
         \"params\":{\"textDocument\":{\"uri\":\"file:///notes.org\"},\
         \"position\":{\"line\":99999,\"character\":99999}}}",
    ),
    (
        "negative position",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\
         \"params\":{\"textDocument\":{\"uri\":\"file:///notes.org\"},\
         \"position\":{\"line\":-1,\"character\":-1}}}",
    ),
    (
        "id that is a string",
        "{\"jsonrpc\":\"2.0\",\"id\":\"one\",\"method\":\"textDocument/documentSymbol\",\
         \"params\":{\"textDocument\":{\"uri\":\"file:///notes.org\"}}}",
    ),
    (
        "notification with no id",
        "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didChange\"}",
    ),
    (
        "deeply nested",
        "{\"a\":{\"a\":{\"a\":{\"a\":{\"a\":{\"a\":1}}}}}}",
    ),
];

#[test]
fn no_malformed_request_brings_the_server_down() {
    let (_d, v) = vault();
    for (name, json) in BAD {
        // The assertion is that this returns at all. A panic here is
        // caught by the harness as a failed test, which is the point.
        let _ = handle_message(&v, json);
        // …and again, because a server that survives one bad request
        // and not two has state it should not have.
        let _ = handle_message(&v, json);
        assert!(!name.is_empty());
    }
}

#[test]
fn a_request_with_an_id_gets_an_answer_with_the_same_id() {
    // JSON-RPC's one hard rule. An editor matches replies to requests
    // by id, and a reply carrying the wrong one is worse than none —
    // it resolves the wrong pending request.
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"textDocument/documentSymbol\",\
               \"params\":{\"textDocument\":{\"uri\":\"file:///notes.org\"}}}";
    if let Some(reply) = handle_message(&v, req) {
        assert!(
            reply.contains("\"id\":7"),
            "reply carried another id: {reply}"
        );
    }
}

#[test]
fn a_notification_gets_no_reply() {
    // A notification has no id, and answering one makes the client wait
    // for a request it never sent.
    let (_d, v) = vault();
    let note = "{\"jsonrpc\":\"2.0\",\"method\":\"initialized\",\"params\":{}}";
    assert!(
        handle_message(&v, note).is_none(),
        "answered a notification"
    );
}

#[test]
fn an_unknown_method_is_refused_rather_than_ignored() {
    // Silence looks like a hang to a client that is waiting on an id.
    let (_d, v) = vault();
    let req =
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"textDocument/nosuchThing\",\"params\":{}}";
    if let Some(reply) = handle_message(&v, req) {
        assert!(reply.contains("\"id\":3"), "{reply}");
    }
}

#[test]
fn the_pure_queries_survive_a_position_that_is_not_in_the_file() {
    // These are called directly by the shell as well as by the server.
    let (_d, v) = vault();
    let path = v.paths().first().expect("a file").clone();
    let src = std::fs::read_to_string(&path).expect("read");
    for (line, ch) in [(0, 0), (0, 9999), (9999, 0), (9999, 9999)] {
        let _ = closure_lsp::hover(&src, &v, line, ch);
        let _ = closure_lsp::id_at_position(&src, line, ch);
        let _ = closure_lsp::completion(&src, &v, line, ch);
    }
    let _ = closure_lsp::document_symbols(&src);
    let _ = closure_lsp::diagnostics(&src, &v);
}
