#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{Registry, RenameHeadline};
use closure_mcp::{DispatchOutcome, resolve_line, run};

fn registry_with_rename() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

#[test]
fn known_command_resolves() {
    let reg = registry_with_rename();
    assert_eq!(
        resolve_line(&reg, "rename-headline target Foo"),
        DispatchOutcome::Found("rename-headline".into())
    );
}

#[test]
fn unknown_command_is_marked_unknown() {
    let reg = Registry::new();
    assert_eq!(
        resolve_line(&reg, "no-such-cmd"),
        DispatchOutcome::Unknown("no-such-cmd".into())
    );
}

#[test]
fn blank_and_comment_lines_skip() {
    let reg = Registry::new();
    assert_eq!(resolve_line(&reg, ""), DispatchOutcome::Skip);
    assert_eq!(resolve_line(&reg, "   "), DispatchOutcome::Skip);
    assert_eq!(resolve_line(&reg, "# a comment"), DispatchOutcome::Skip);
}

#[test]
fn run_emits_one_line_per_resolved_command() {
    let reg = registry_with_rename();
    let input = b"rename-headline\n# comment\nfoo\n\n" as &[u8];
    let mut output: Vec<u8> = Vec::new();
    run(&reg, input, &mut output).unwrap();
    let out = String::from_utf8(output).unwrap();
    assert_eq!(out, "OK rename-headline\nUNKNOWN foo\n");
}
