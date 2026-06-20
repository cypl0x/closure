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
