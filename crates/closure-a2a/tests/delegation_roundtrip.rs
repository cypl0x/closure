//! A2A task delegation round-trip test (agent A posts, B executes on separate vault).
//!
//! TDD written FIRST. Must fail until impl.
//! Per ROADMAP: "task delegation — agent A posts a task, agent B (separate vault)
//! executes via its registry, returns result; round-trip test"
//! I8: execution only through registered commands / Vault::run_tool surface.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_a2a::{delegate_task, resolve_line};
use closure_core::{Registry, RenameHeadline};
use closure_store::Vault;
use tempfile::TempDir;

fn registry_with_rename() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

fn empty_vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    // minimal org so vault opens; capture will create inbox.org
    fs::write(dir.path().join("notes.org"), "* Seed\n").expect("seed");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn resolve_line_compat_for_a2a_registry() {
    let reg = registry_with_rename();
    let out = resolve_line(&reg, "rename-headline id New");
    assert!(
        format!("{:?}", out).contains("rename-headline") || format!("{:?}", out).contains("Found")
    );
}

#[test]
fn delegate_task_executes_capture_on_target_vault() {
    let (_td, mut b) = empty_vault();
    // A posts a capture task to B (separate vault)
    let result = delegate_task(&mut b, "capture Buy milk via A2A");
    // result comes from run_tool
    assert!(!result.is_empty());
    // side effect: B now has the captured headline (via its registry surface)
    assert!(
        b.find_by_title("Buy milk via A2A").is_some(),
        "delegated capture must appear in B: {result}"
    );
}

#[test]
fn a2a_roundtrip_delegation_from_a_to_b() {
    // Full roundtrip: A prepares/posts task, B (separate) receives+executes, result to A, mutation only in B
    let (_td_a, a) = empty_vault();
    let (_td_b, mut b) = empty_vault();

    // task line as would be posted by A
    let task = "capture Task from A to B";
    let result = delegate_task(&mut b, task);

    // A sees result (string returned across "wire")
    assert!(
        result.contains("Task from A to B") || !result.starts_with("ERROR"),
        "A received usable result: {result}"
    );

    // B (receiver) mutated, A did not
    assert!(
        b.find_by_title("Task from A to B").is_some(),
        "B executed the delegated task"
    );
    assert!(
        a.find_by_title("Task from A to B").is_none(),
        "A's vault untouched"
    );
}

#[test]
fn delegate_unknown_task_returns_error_text() {
    let (_td, mut b) = empty_vault();
    let result = delegate_task(&mut b, "no-such-tool-xyz 123");
    assert!(
        result.contains("ERROR") || result.contains("unknown") || result.contains("no such"),
        "unknown task surfaces error: {result}"
    );
}

#[test]
fn delegate_via_registry_check() {
    // Exercise that delegation path can/does consult a registry (even if run_tool is the exec surface)
    let reg = registry_with_rename();
    let (_td, mut b) = empty_vault();
    // resolve first (simulates A2A peer doing discovery before delegate)
    let decision = resolve_line(&reg, "rename-headline");
    assert!(format!("{:?}", decision).contains("Found"));
    // then delegate a known (via run_tool which is registry backed)
    let _ = delegate_task(&mut b, "capture Delegated after resolve");
    assert!(b.find_by_title("Delegated after resolve").is_some());
}
