//! What the ACP loops do when the far end goes away.
//!
//! Both loops write to a `W: Write` and map every failure to
//! `AcpError::Transport`. Those arms had never run, because every test
//! writes into a `Vec<u8>`, which cannot fail.
//!
//! A closed pipe is not an exotic case for this crate: it is what
//! happens every single time the editor on the other end exits first,
//! which is the normal way one of these sessions ends. The loop has to
//! return the error rather than swallowing it and spinning on a
//! stream nobody is reading.
//!
//! `tools/list` is here too — it is the method a client calls before
//! anything else to find out what this agent can do, and it had no
//! test at all.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::{self, Write};

use closure_acp::{AcpError, handle_message, run, serve_jsonrpc};
use closure_core::{Registry, RenameHeadline};
use closure_store::Vault;
use tempfile::TempDir;

/// A writer that refuses, the way a pipe with no reader does.
struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the reader left"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "the reader left"))
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
fn the_text_loop_reports_a_write_that_fails_rather_than_spinning() {
    let r = registry();
    let err = run(&r, &b"rename-headline x\n"[..], &mut BrokenPipe)
        .expect_err("a broken pipe was ignored");
    assert!(matches!(err, AcpError::Transport(_)), "{err:?}");
}

#[test]
fn the_text_loop_reports_a_failing_write_for_an_unknown_command_too() {
    // The other writing arm. Only one of the two being checked would
    // leave a loop that hangs on exactly the inputs it does not
    // recognise.
    let r = registry();
    let err =
        run(&r, &b"no-such-command\n"[..], &mut BrokenPipe).expect_err("a broken pipe was ignored");
    assert!(matches!(err, AcpError::Transport(_)), "{err:?}");
}

#[test]
fn the_text_loop_reports_a_read_that_fails() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    let err = run(&r, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a failed read was treated as end of input");
    assert!(matches!(err, AcpError::Transport(_)), "{err:?}");
}

#[test]
fn a_line_that_is_only_whitespace_is_skipped_without_a_reply() {
    // The Skip arm. A blank line is what a client sends when a user
    // presses Enter, and answering it would put a spurious response
    // into a stream the client is matching up by order.
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
    assert!(matches!(err, AcpError::Transport(_)), "{err:?}");
}

#[test]
fn the_jsonrpc_loop_reports_a_read_that_fails() {
    let (_d, mut v) = vault();
    let mut out: Vec<u8> = Vec::new();
    let err = serve_jsonrpc(&mut v, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a failed read was treated as end of input");
    assert!(matches!(err, AcpError::Transport(_)), "{err:?}");
}

#[test]
fn a_notification_with_no_id_gets_no_response_line() {
    // The `continue` arm: a message that needs no reply must not put a
    // line on the wire, or every later response is matched to the
    // wrong request.
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
fn tools_list_describes_every_tool_with_a_schema() {
    // What a client asks before it asks anything else. A tool listed
    // without a schema, or a schema without the argument the call
    // takes, makes the tool uncallable — and nothing was checking that
    // the list is even well-formed.
    let (_d, mut v) = vault();
    let msg = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}";
    let resp = handle_message(&mut v, msg).expect("a response");

    assert!(resp.contains("\"tools\""), "{resp}");
    assert!(resp.contains("\"name\""), "{resp}");
    assert!(resp.contains("\"description\""), "{resp}");
    assert!(resp.contains("\"inputSchema\""), "{resp}");
    assert!(
        resp.contains("\"args\""),
        "the schema does not describe the argument a call takes: {resp}"
    );
    // The tool the other tests call has to be in the list it advertises.
    assert!(resp.contains("capture"), "{resp}");
}
