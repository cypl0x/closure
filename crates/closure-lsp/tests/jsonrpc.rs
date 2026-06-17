//! J3: real LSP wire format — Content-Length-framed JSON-RPC — over
//! in-memory buffers (hermetic). documentSymbol uses `document_symbols`;
//! reads go through the vault; no mutation here (LSP is read-only).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_lsp::serve;
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n** Subtask\n* Personal wiki\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

/// Frame a JSON body the LSP way.
fn frame(json: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{json}", json.len())
}

/// Split framed output into the JSON bodies.
fn unframe(out: &[u8]) -> Vec<String> {
    let s = String::from_utf8(out.to_vec()).unwrap();
    let mut bodies = Vec::new();
    let mut rest = s.as_str();
    while let Some(hdr) = rest.find("Content-Length: ") {
        let after = &rest[hdr + "Content-Length: ".len()..];
        let (num, tail) = after.split_once("\r\n\r\n").unwrap();
        let len: usize = num.trim().parse().unwrap();
        bodies.push(tail[..len].to_owned());
        rest = &tail[len..];
    }
    bodies
}

#[test]
fn initialize_then_document_symbol_framed() {
    let (_d, v) = vault();
    let mut input = String::new();
    input.push_str(&frame(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#));
    input.push_str(&frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/documentSymbol","uri":"notes.org"}"#,
    ));
    let mut out: Vec<u8> = Vec::new();
    serve(&v, input.as_bytes(), &mut out).expect("serve");
    let bodies = unframe(&out);
    assert_eq!(bodies.len(), 2, "two framed responses: {bodies:?}");
    assert!(bodies[0].contains("\"id\":1") && bodies[0].contains("documentSymbolProvider"));
    assert!(bodies[1].contains("\"id\":2"), "{}", bodies[1]);
    // documentSymbol returns the headline names (TODO keyword stripped).
    assert!(bodies[1].contains("Ship parser"), "symbols: {}", bodies[1]);
    assert!(bodies[1].contains("Personal wiki"));
    assert!(bodies[1].contains("Subtask"));
}

#[test]
fn unknown_method_is_method_not_found() {
    let (_d, v) = vault();
    let input = frame(r#"{"jsonrpc":"2.0","id":5,"method":"frobnicate"}"#);
    let mut out: Vec<u8> = Vec::new();
    serve(&v, input.as_bytes(), &mut out).expect("serve");
    let bodies = unframe(&out);
    assert!(bodies[0].contains("-32601") && bodies[0].contains("\"id\":5"));
}
