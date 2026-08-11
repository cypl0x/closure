//! L3: diagnostics. Ranged problems for a document: dead `id:` links
//! (target missing in the vault), duplicate `:ID:` values across the
//! vault, and `closure-config` block validation errors.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_lsp::{DiagnosticCode, Severity, diagnostics, handle_message};
use closure_store::Vault;

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn dead_id_link_is_an_error_diagnostic() {
    let (_d, v) = vault_with(&[("a.org", "* A\n")]);
    let src = "see [[id:01HXZZZZZZZZZZZZZZZZZZZZZZ]] gone\n";
    let diags = diagnostics(src, &v);
    let d = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::DeadLink)
        .expect("dead link flagged");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.line, 0);
    assert!(
        d.message.contains("01HXZZZZZZZZZZZZZZZZZZZZZZ"),
        "{}",
        d.message
    );
    assert!(d.end_char > d.start_char, "non-empty range");
}

#[test]
fn live_id_link_is_clean() {
    let (_d, v) = vault_with(&[("a.org", &format!("* A\n:PROPERTIES:\n:ID: {ID}\n:END:\n"))]);
    let src = format!("see [[id:{ID}]] ok\n");
    assert!(
        diagnostics(&src, &v)
            .iter()
            .all(|d| d.code != DiagnosticCode::DeadLink)
    );
}

#[test]
fn duplicate_id_across_vault_is_flagged() {
    let dupe = format!("* X\n:PROPERTIES:\n:ID: {ID}\n:END:\n");
    let (_d, v) = vault_with(&[("a.org", &dupe), ("b.org", &dupe)]);
    let diags = diagnostics(&dupe, &v);
    let d = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::DuplicateId)
        .expect("duplicate id flagged");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.line, 2, "the :ID: line");
}

#[test]
fn unique_id_is_clean() {
    let (_d, v) = vault_with(&[("a.org", &format!("* X\n:PROPERTIES:\n:ID: {ID}\n:END:\n"))]);
    let src = format!("* X\n:PROPERTIES:\n:ID: {ID}\n:END:\n");
    assert!(
        diagnostics(&src, &v)
            .iter()
            .all(|d| d.code != DiagnosticCode::DuplicateId)
    );
}

#[test]
fn bad_config_value_is_a_diagnostic() {
    let (_d, v) = vault_with(&[("a.org", "* A\n")]);
    let src = "#+BEGIN_SRC closure-config\ninput_mode = nonsense\n#+END_SRC\n";
    let diags = diagnostics(src, &v);
    let d = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::Config)
        .expect("config error flagged");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.line, 1, "the offending key line in the document");
}

#[test]
fn unknown_widget_reference_is_a_diagnostic() {
    let src = "#+BEGIN: closure-widget :name p\n{{ghost}}\n#+END:\n";
    let (_d, v) = vault_with(&[("a.org", src)]);
    let diags = diagnostics(src, &v);
    let d = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::Widget)
        .expect("widget error flagged");
    assert_eq!(d.severity, Severity::Error);
    assert!(d.message.contains("ghost"), "{}", d.message);
}

#[test]
fn clean_document_has_no_diagnostics() {
    let (_d, v) = vault_with(&[("a.org", "* A\n")]);
    assert!(diagnostics("* just a headline\nbody\n", &v).is_empty());
}

#[test]
fn protocol_textdocument_diagnostic_returns_full_report() {
    // a.org carries a dead id: link; pull diagnostics over its uri.
    let (_d, v) = vault_with(&[("a.org", "see [[id:01HXZZZZZZZZZZZZZZZZZZZZZZ]]\n")]);
    let req = "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"textDocument/diagnostic\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://a.org\"}}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("\"id\":9"), "echoes id: {resp}");
    assert!(resp.contains("\"kind\":\"full\""), "full report: {resp}");
    assert!(
        resp.contains("\"code\":\"dead-link\""),
        "carries the dead link: {resp}"
    );
    assert!(resp.contains("\"severity\":1"), "error severity: {resp}");
}

#[test]
fn protocol_initialize_advertises_diagnostics() {
    let (_d, v) = vault_with(&[("a.org", "* A\n")]);
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(
        resp.contains("diagnosticProvider"),
        "advertises diagnostics: {resp}"
    );
}

// === Q5-O2: dead footnote references are diagnosed. ===

#[test]
fn dead_footnote_reference_is_reported() {
    let dir = tempfile::tempdir().expect("tmp");
    let src = "* Note\nBody with a ref[fn:alive] and a dead one[fn:ghost].\n\n[fn:alive] the definition\n";
    std::fs::write(dir.path().join("n.org"), src).expect("write");
    let vault = closure_store::Vault::open(dir.path()).expect("open");
    let diags = closure_lsp::diagnostics(src, &vault);
    let foot: Vec<_> = diags
        .iter()
        .filter(|d| d.code == closure_lsp::DiagnosticCode::Footnote)
        .collect();
    assert_eq!(foot.len(), 1, "exactly the ghost: {foot:?}");
    assert!(foot[0].message.contains("ghost"), "{}", foot[0].message);
    assert_eq!(foot[0].line, 1, "on the referencing line");
}

#[test]
fn defined_footnotes_are_clean() {
    let dir = tempfile::tempdir().expect("tmp");
    let src = "* Note\nfine[fn:a]\n\n[fn:a] def\n";
    std::fs::write(dir.path().join("n.org"), src).expect("write");
    let vault = closure_store::Vault::open(dir.path()).expect("open");
    assert!(
        closure_lsp::diagnostics(src, &vault)
            .iter()
            .all(|d| d.code != closure_lsp::DiagnosticCode::Footnote)
    );
}

// === precise ranges (2026-08-11) ===
//
// A composition error used to be reported at the first
// `#+begin: closure-widget` line in the file, whatever the error was
// and wherever it happened. In an editor that underlines the wrong
// text: the block's own header, several lines above a reference that
// is perfectly visible. The error already knows which name, which
// argument and which value went wrong, so the diagnostic points at it.

#[test]
fn an_unknown_reference_is_marked_at_the_reference() {
    let src = "#+BEGIN: closure-widget :name p\nsome prose\nand then {{ghost}} here\n#+END:\n";
    let (_d, v) = vault_with(&[("a.org", src)]);
    let d = diagnostics(src, &v)
        .into_iter()
        .find(|d| d.code == DiagnosticCode::Widget)
        .expect("widget error flagged");
    assert_eq!(d.line, 2, "the line the reference is on");
    let line = src.lines().nth(2).unwrap();
    let at = line.find("{{ghost}}").unwrap();
    assert_eq!(
        d.start_char as usize, at,
        "marked `{line}` at the wrong column"
    );
    assert_eq!(d.end_char as usize, at + "{{ghost}}".len());
}

#[test]
fn a_bad_argument_is_marked_at_the_value() {
    let src = "#+BEGIN: closure-widget :name card :inputs count:number\n[{{count}}]\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{card count=banana}}\n#+END:\n";
    let (_d, v) = vault_with(&[("a.org", src)]);
    let d = diagnostics(src, &v)
        .into_iter()
        .find(|d| d.code == DiagnosticCode::Widget)
        .expect("widget error flagged");
    assert_eq!(d.line, 4, "the call site's line");
    let line = src.lines().nth(4).unwrap();
    let at = line.find("banana").unwrap();
    assert_eq!(
        d.start_char as usize, at,
        "marked `{line}` at the wrong column"
    );
}

#[test]
fn an_unknown_argument_is_marked_at_the_argument() {
    let src = "#+BEGIN: closure-widget :name card :inputs title\n[{{title}}]\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{card titel=Today}}\n#+END:\n";
    let (_d, v) = vault_with(&[("a.org", src)]);
    let d = diagnostics(src, &v)
        .into_iter()
        .find(|d| d.code == DiagnosticCode::Widget)
        .expect("widget error flagged");
    assert_eq!(d.line, 4);
    let line = src.lines().nth(4).unwrap();
    assert_eq!(d.start_char as usize, line.find("titel").unwrap());
}

#[test]
fn a_cycle_is_marked_at_a_reference_in_the_ring() {
    let src = "#+BEGIN: closure-widget :name a\n{{b}}\n#+END:\n\
               #+BEGIN: closure-widget :name b\n{{a}}\n#+END:\n";
    let (_d, v) = vault_with(&[("a.org", src)]);
    let d = diagnostics(src, &v)
        .into_iter()
        .find(|d| d.code == DiagnosticCode::Widget)
        .expect("widget error flagged");
    // Not the BEGIN line, which is where every one of these used to go.
    let marked = src.lines().nth(d.line as usize).unwrap();
    assert!(
        !marked
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("#+begin:"),
        "still pointing at the block header: {marked}"
    );
}
