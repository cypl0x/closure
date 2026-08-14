//! The MCP loops on a stream that fails, and the `LIST` verb.
//!
//! The same gap the ACP and A2A crates had, and it matters most here:
//! MCP is the protocol an editor speaks to closure, so the far end
//! going away is the ordinary end of a session rather than an
//! exception. Every write is mapped to `McpError::Transport` and not
//! one of those arms had run, because every test writes into a
//! `Vec<u8>` and a `Vec` cannot fail.
//!
//! `LIST` is here because it had no test at all and it is how a client
//! discovers what closure can do. A `LIST` that answered nothing, or
//! answered in a different order each time, is a client that cannot
//! build a menu.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::{self, Write};

use closure_core::{Registry, RenameHeadline, SetTodo};
use closure_mcp::{DispatchOutcome, McpError, resolve_line, run, serve_jsonrpc};
use closure_store::Vault;
use tempfile::TempDir;

struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the editor left"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the editor left"))
    }
}

struct FailingRead;

impl io::Read for FailingRead {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the connection dropped mid-line"))
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r.register(Box::new(SetTodo::new_placeholder()));
    r
}

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn a_write_that_fails_stops_the_loop_for_a_known_command() {
    let r = registry();
    let err = run(&r, &b"rename-headline x\n"[..], &mut BrokenPipe)
        .expect_err("a broken pipe was ignored");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}

#[test]
fn a_write_that_fails_stops_the_loop_for_an_unknown_one_too() {
    let r = registry();
    let err =
        run(&r, &b"no-such-command\n"[..], &mut BrokenPipe).expect_err("a broken pipe was ignored");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}

#[test]
fn a_write_that_fails_partway_through_a_list_is_reported() {
    // `LIST` writes one line per command, so it is the only arm that
    // can fail after having already written something. A loop that
    // ignored the error here would keep writing the rest of the list
    // into a closed pipe.
    let r = registry();
    let err = run(&r, &b"LIST\n"[..], &mut BrokenPipe).expect_err("a broken pipe was ignored");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}

#[test]
fn a_read_that_fails_is_not_treated_as_end_of_input() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    let err = run(&r, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a dropped connection looked like a clean exit");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}

#[test]
fn list_names_every_registered_command_in_a_stable_order() {
    // A client builds its menu from this. Ordering that varied between
    // runs would reshuffle somebody's menu for no reason.
    let r = registry();
    let mut first: Vec<u8> = Vec::new();
    run(&r, &b"LIST\n"[..], &mut first).expect("list");
    let mut second: Vec<u8> = Vec::new();
    run(&r, &b"LIST\n"[..], &mut second).expect("list again");

    let text = String::from_utf8(first.clone()).expect("utf8");
    assert!(text.contains("rename-headline"), "{text}");
    assert!(text.contains("set-todo"), "{text}");
    assert_eq!(first, second, "two LISTs disagreed");

    let mut sorted: Vec<&str> = text.lines().collect();
    let original = sorted.clone();
    sorted.sort_unstable();
    assert_eq!(original, sorted, "the list is not sorted: {text}");
}

#[test]
fn resolve_line_skips_blanks_and_comments_and_spots_list() {
    // The Skip arm covers two different things — an empty line and a
    // comment — and a protocol that answered either would put a
    // spurious line into a stream the client matches up by order.
    let r = registry();
    for quiet in ["", "   ", "\t", "# a comment", "  # indented comment"] {
        assert_eq!(
            resolve_line(&r, quiet),
            DispatchOutcome::Skip,
            "{quiet:?} was not skipped"
        );
    }
    assert_eq!(resolve_line(&r, "LIST"), DispatchOutcome::List);
    assert_eq!(
        resolve_line(&r, "rename-headline whatever"),
        DispatchOutcome::Found("rename-headline".to_owned())
    );
}

#[test]
fn comments_and_blanks_produce_no_output_at_all() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    run(&r, &b"\n# a comment\n   \n"[..], &mut out).expect("run");
    assert!(
        out.is_empty(),
        "quiet lines produced output: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn the_jsonrpc_loop_reports_a_write_that_fails() {
    let (_d, mut v) = vault();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";
    let err = serve_jsonrpc(&mut v, input.as_bytes(), &mut BrokenPipe)
        .expect_err("a broken pipe was ignored");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}

#[test]
fn the_jsonrpc_loop_reports_a_read_that_fails() {
    let (_d, mut v) = vault();
    let mut out: Vec<u8> = Vec::new();
    let err = serve_jsonrpc(&mut v, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a dropped connection looked like a clean exit");
    assert!(matches!(err, McpError::Transport(_)), "{err:?}");
}
