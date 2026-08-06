//! "[#C] Language Server Protocol (support)" — finishing it.
//!
//! "This could be helpful if we want to support something like
//! LanguageTool via LSP. Or even better org-edit-special on src blocks
//! and then fiddle with the source code."
//!
//! Six capabilities are advertised and six answer. What had not been
//! checked is the shape an *editor* sends: a `file:///absolute/path`
//! URI, which is what every LSP client puts on the wire — the MCP
//! bridge had exactly this bug and returned empty files for a whole
//! release.
//!
//! And the one thing an org language server is for and did not have:
//! `textDocument/definition`, so `[[id:01…]]` jumps to the headline it
//! names.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01LSPAAAAAAAAAAAAAAAAAAAAA\n:END:\n\
         links to [[id:01LSPBBBBBBBBBBBBBBBBBBBBB]] here\n\
         * Beta\n:PROPERTIES:\n:ID: 01LSPBBBBBBBBBBBBBBBBBBBBB\n:END:\nbeta body\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

/// The `file://` URI an editor would send for `notes.org`.
fn uri(dir: &tempfile::TempDir) -> String {
    format!("file://{}", dir.path().join("notes.org").display())
}

#[test]
fn document_symbols_work_from_an_absolute_uri() {
    // What an editor sends. A server that only understands its own
    // relative spelling answers "no symbols" for every open file.
    let (dir, mut v) = vault();
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"textDocument/documentSymbol",
            "params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
        uri(&dir)
    );
    let out = closure_lsp::handle_message_mut(&mut v, &msg).expect("a reply");
    assert!(out.contains("Alpha"), "no symbols for an open file: {out}");
    assert!(out.contains("Beta"), "{out}");
}

#[test]
fn hover_works_from_an_absolute_uri() {
    let (dir, mut v) = vault();
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/hover",
            "params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}}}}}}"#,
        uri(&dir)
    );
    let out = closure_lsp::handle_message_mut(&mut v, &msg).expect("a reply");
    assert!(
        out.contains("01LSPAAAAAAAAAAAAAAAAAAAAA"),
        "hover said nothing about the headline under the cursor: {out}"
    );
}

#[test]
fn definition_jumps_to_the_headline_a_link_names() {
    // The one an org language server is for: the cursor is on
    // `[[id:01LSPBBB…]]` and the answer is where Beta lives.
    let (dir, mut v) = vault();
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/definition",
            "params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":4,"character":20}}}}}}"#,
        uri(&dir)
    );
    let out = closure_lsp::handle_message_mut(&mut v, &msg).expect("a reply");
    assert!(out.contains("notes.org"), "no location: {out}");
    assert!(
        out.contains("\"line\":5") || out.contains("\"line\": 5"),
        "it did not land on Beta's headline: {out}"
    );
}

#[test]
fn definition_off_a_link_answers_nothing_rather_than_guessing() {
    // Line 4 is `links to [[id:…]] here`; character 2 is inside the
    // word "links". Note it is *not* a headline line — the cursor on a
    // headline resolves to that headline's own id, which is a jump to
    // where you already are and is the right answer to a different
    // question.
    let (dir, mut v) = vault();
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/definition",
            "params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":4,"character":2}}}}}}"#,
        uri(&dir)
    );
    let out = closure_lsp::handle_message_mut(&mut v, &msg).expect("a reply");
    assert!(out.contains("null"), "it invented a jump: {out}");
}

#[test]
fn every_capability_it_advertises_answers() {
    // The classic LSP defect: advertise a provider, and the editor
    // calls it and gets an error for the rest of the session.
    let (dir, mut v) = vault();
    let init = closure_lsp::handle_message_mut(
        &mut v,
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#,
    )
    .expect("initialize");
    let methods = [
        ("documentSymbolProvider", "textDocument/documentSymbol"),
        ("hoverProvider", "textDocument/hover"),
        ("completionProvider", "textDocument/completion"),
        ("diagnosticProvider", "textDocument/diagnostic"),
        ("referencesProvider", "textDocument/references"),
        ("renameProvider", "textDocument/rename"),
        ("definitionProvider", "textDocument/definition"),
    ];
    for (capability, method) in methods {
        if !init.contains(capability) {
            continue;
        }
        let msg = format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"{method}",
                "params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":0}},
                "newName":"x","context":{{"includeDeclaration":true}}}}}}"#,
            uri(&dir)
        );
        let out = closure_lsp::handle_message_mut(&mut v, &msg).expect("a reply");
        assert!(
            !out.contains("-32601"),
            "`{capability}` is advertised and `{method}` is method-not-found"
        );
    }
}
