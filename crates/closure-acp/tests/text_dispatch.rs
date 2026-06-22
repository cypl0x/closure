//! V10b coverage: the text-mode dispatch path (`resolve_line`/`run`) and
//! `tools/call`, exercised hermetically.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_acp::{Outcome, handle_message, resolve_line, run};
use closure_core::{Registry, RenameHeadline};
use closure_store::Vault;
use tempfile::TempDir;

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("n.org"), "* TODO Ship\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn resolve_line_classifies_lines() {
    let r = registry();
    assert_eq!(
        resolve_line(&r, "rename-headline x"),
        Outcome::Found("rename-headline".to_owned())
    );
    assert_eq!(
        resolve_line(&r, "nope"),
        Outcome::Unknown("nope".to_owned())
    );
    assert_eq!(resolve_line(&r, ""), Outcome::Skip);
    assert_eq!(resolve_line(&r, "# comment"), Outcome::Skip);
}

#[test]
fn run_emits_ok_and_unknown_lines() {
    let r = registry();
    let input = b"rename-headline\nnope\n\n# c\n";
    let mut out = Vec::new();
    run(&r, &input[..], &mut out).expect("run");
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("OK rename-headline"));
    assert!(s.contains("UNKNOWN nope"));
}

#[test]
fn tools_call_routes_through_the_vault() {
    let (_d, mut v) = vault();
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list-files","args":""}}"#;
    let resp = handle_message(&mut v, req).expect("response");
    assert!(resp.contains("n.org"), "tool output: {resp}");
}
