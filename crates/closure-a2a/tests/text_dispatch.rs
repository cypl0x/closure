//! V10b coverage: A2A text-mode dispatch (`resolve_line`/`run`) and
//! `agent/card` / `tools`-style routing, exercised hermetically.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_a2a::{Outcome, handle_message, resolve_line, run};
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
    assert_eq!(resolve_line(&r, "  "), Outcome::Skip);
}

#[test]
fn run_emits_ok_and_unknown_lines() {
    let r = registry();
    let mut out = Vec::new();
    run(&r, &b"rename-headline\nnope\n"[..], &mut out).expect("run");
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("OK rename-headline") && s.contains("UNKNOWN nope"));
}

#[test]
fn agent_card_lists_skills() {
    // Rewritten 2026-08-06: this asserted the card advertised
    // `task/delegate`, which is the *transport*. An agent reading that
    // learned it could delegate a task and nothing about what a task
    // may be. The skills are the tools now, which is what was being
    // asked.
    let (_d, mut v) = vault();
    let resp = handle_message(&mut v, r#"{"jsonrpc":"2.0","id":1,"method":"agent/card"}"#)
        .expect("response");
    for skill in closure_a2a::SKILLS {
        assert!(
            resp.contains(skill.id),
            "{} missing from card: {resp}",
            skill.id
        );
    }
}

#[test]
fn unknown_method_is_method_not_found() {
    let (_d, mut v) = vault();
    let resp = handle_message(&mut v, r#"{"jsonrpc":"2.0","id":2,"method":"frobnicate"}"#)
        .expect("response");
    assert!(resp.contains("-32601"), "method not found: {resp}");
}
