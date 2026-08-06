//! The server answered about the file on disk, whatever the editor had
//! on screen.
//!
//! LSP's whole premise is that the client owns the buffer: it sends
//! `didOpen` with the text and `didChange` with every edit, precisely
//! because the interesting state is the one you have not saved yet. A
//! server that re-reads the path instead answers about a version of
//! the file that only agrees with yours between saves.
//!
//! Found while building the embedded-src-block client, where it is not
//! a subtlety at all: a `#+BEGIN_SRC rust` block is handed over as a
//! document that has no path, so "read it from disk" answers about
//! nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_lsp::Overlay;
use closure_store::Vault;

const ON_DISK: &str = "* Saved headline\n:PROPERTIES:\n:ID: 01OVERLAY000000000000001\n:END:\n";

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), ON_DISK).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

fn ask(vault: &mut Vault, overlay: &mut Overlay, msg: &str) -> Option<String> {
    closure_lsp::handle_message_with(vault, overlay, msg)
}

#[test]
fn a_symbol_query_sees_what_the_editor_opened_not_what_is_saved() {
    let (_d, mut v) = vault();
    let mut overlay = Overlay::default();
    ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file://a.org","languageId":"org","version":1,"text":"* Unsaved headline\n"}}}"#,
    );
    let reply = ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file://a.org"}}}"#,
    )
    .expect("a reply");
    assert!(
        reply.contains("Unsaved headline"),
        "the server answered about the file on disk: {reply}"
    );
}

#[test]
fn an_edit_replaces_what_was_opened() {
    let (_d, mut v) = vault();
    let mut overlay = Overlay::default();
    ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file://a.org","text":"* One\n"}}}"#,
    );
    ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file://a.org"},"contentChanges":[{"text":"* Two\n"}]}}"#,
    );
    let reply = ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file://a.org"}}}"#,
    )
    .expect("a reply");
    assert!(reply.contains("Two") && !reply.contains("One"), "{reply}");
}

#[test]
fn closing_it_goes_back_to_the_file_on_disk() {
    let (_d, mut v) = vault();
    let mut overlay = Overlay::default();
    ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file://a.org","text":"* Unsaved headline\n"}}}"#,
    );
    ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":"file://a.org"}}}"#,
    );
    let reply = ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file://a.org"}}}"#,
    )
    .expect("a reply");
    assert!(
        reply.contains("Saved headline"),
        "a closed document kept overriding the file: {reply}"
    );
}

#[test]
fn a_document_that_was_never_opened_is_still_read_from_the_vault() {
    let (_d, mut v) = vault();
    let mut overlay = Overlay::default();
    let reply = ask(
        &mut v,
        &mut overlay,
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file://a.org"}}}"#,
    )
    .expect("a reply");
    assert!(reply.contains("Saved headline"), "{reply}");
}
