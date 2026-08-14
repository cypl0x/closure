//! What the A2A loops do when the far end goes away, and the states a
//! task can be in.
//!
//! The same gap as `closure-acp`: both loops map every write failure to
//! `A2aError::Transport` and those arms had never run, because every
//! test writes into a `Vec<u8>` and a `Vec` cannot fail. A closed pipe
//! is not exotic — it is what happens whenever the peer exits first,
//! which is the ordinary way one of these sessions ends.
//!
//! `TaskState::as_str` is here for a different reason. Three of its
//! four tokens go on the wire in delegation replies and only one was
//! ever produced by a test. A state that stringified wrongly would be
//! read by the other agent as a different state, and "failed" arriving
//! as "done" is the one that loses work quietly.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::{self, Write};

use closure_a2a::{A2aError, TaskState, run, serve_jsonrpc};
use closure_core::{Registry, RenameHeadline};
use closure_store::Vault;
use tempfile::TempDir;

/// A writer that refuses, the way a pipe with no reader does.
struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the peer left"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the peer left"))
    }
}

/// A reader that fails instead of reaching end of input.
struct FailingRead;

impl io::Read for FailingRead {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the connection dropped mid-line"))
    }
}

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

#[test]
fn the_text_loop_reports_a_write_that_fails() {
    let r = registry();
    let err = run(&r, &b"rename-headline x\n"[..], &mut BrokenPipe)
        .expect_err("a broken pipe was ignored");
    assert!(matches!(err, A2aError::Transport(_)), "{err:?}");
}

#[test]
fn the_text_loop_reports_a_failing_write_for_an_unknown_command_too() {
    let r = registry();
    let err =
        run(&r, &b"no-such-command\n"[..], &mut BrokenPipe).expect_err("a broken pipe was ignored");
    assert!(matches!(err, A2aError::Transport(_)), "{err:?}");
}

#[test]
fn the_text_loop_reports_a_read_that_fails() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    let err = run(&r, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a failed read was treated as end of input");
    assert!(matches!(err, A2aError::Transport(_)), "{err:?}");
}

#[test]
fn a_blank_line_is_skipped_without_a_reply() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    run(&r, &b"\n   \n\t\n"[..], &mut out).expect("run");
    assert!(
        out.is_empty(),
        "blank lines produced output: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn the_jsonrpc_loop_reports_a_write_that_fails() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";
    let err = serve_jsonrpc(&mut v, input.as_bytes(), &mut BrokenPipe)
        .expect_err("a broken pipe was ignored");
    assert!(matches!(err, A2aError::Transport(_)), "{err:?}");
}

#[test]
fn the_jsonrpc_loop_reports_a_read_that_fails() {
    let (_d, mut v) = vault();
    let mut out: Vec<u8> = Vec::new();
    let err = serve_jsonrpc(&mut v, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a failed read was treated as end of input");
    assert!(matches!(err, A2aError::Transport(_)), "{err:?}");
}

#[test]
fn a_notification_with_no_id_gets_no_response_line() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}\n";
    let mut out: Vec<u8> = Vec::new();
    serve_jsonrpc(&mut v, input.as_bytes(), &mut out).expect("serve");
    assert!(
        String::from_utf8_lossy(&out).trim().is_empty(),
        "a notification was answered: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn every_task_state_has_its_own_wire_token() {
    // All four, and all four distinct. Two states sharing a token
    // would be indistinguishable to the agent on the other end.
    let all = [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::Done,
        TaskState::Failed,
    ];
    let tokens: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
    assert_eq!(tokens, ["submitted", "working", "done", "failed"]);

    let mut unique = tokens.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), tokens.len(), "two states share a token");
}
