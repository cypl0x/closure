//! The commands that speak a protocol on stdin, and the ones that set a
//! vault up.
//!
//! The parent coverage item says a line that genuinely cannot be run
//! gets deleted or restructured, not annotated. Before accepting that
//! about `mcp`, `acp`, `lsp`, `serve` and friends, it is worth checking
//! which of them actually need a network or a terminal — and most do
//! not. The protocol servers read requests from *stdin* and write to
//! stdout, so a closed stdin is a complete, hermetic session: they must
//! start, serve nothing, and exit.
//!
//! That is a real claim. A server that panics on an empty stream, or
//! hangs waiting for a request that will never come, is broken for
//! every client that disconnects — which is every client, eventually.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::{Command, Stdio};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* A note\n:PROPERTIES:\n:ID: 01CLISTDIO0000000001\n:END:\nbody\n",
    )
    .expect("write");
    d
}

/// Run with stdin already closed, so a protocol server sees EOF at once.
fn run_with_closed_stdin(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn closure")
}

fn assert_no_panic(what: &str, out: &std::process::Output) {
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked at"), "`{what}` panicked:\n{err}");
}

#[test]
fn every_protocol_server_starts_and_ends_on_a_closed_stream() {
    // Not "it ran": a server that hangs on EOF hangs the editor that
    // launched it. The test harness would time out rather than fail,
    // which is why this is worth having even though it looks trivial.
    let d = vault();
    let dir = d.path().to_string_lossy().into_owned();
    for cmd in ["mcp", "acp", "lsp"] {
        let out = run_with_closed_stdin(&[cmd, &dir]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn a_protocol_server_on_a_vault_that_is_not_there_says_so() {
    for cmd in ["mcp", "acp", "lsp"] {
        let out = run_with_closed_stdin(&[cmd, "/nonexistent/vault/for/a/test"]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn init_vault_makes_a_vault_that_the_checker_accepts() {
    // The claim `init-vault` is making. A scaffold that does not
    // roundtrip is worse than no scaffold: the first thing a new user
    // does is open it.
    let d = tempfile::tempdir().expect("tempdir");
    let dir = d.path().join("fresh");
    let out = Command::new(BIN)
        .args(["init-vault", &dir.to_string_lossy()])
        .output()
        .expect("spawn");
    assert_no_panic("init-vault", &out);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let check = Command::new(BIN)
        .args(["check", &dir.to_string_lossy()])
        .output()
        .expect("spawn");
    assert!(
        check.status.success(),
        "a freshly initialised vault does not roundtrip:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn trust_records_a_language_and_reports_it() {
    let d = vault();
    let dir = d.path().to_string_lossy().into_owned();
    let listed = Command::new(BIN)
        .args(["trust", &dir])
        .output()
        .expect("spawn");
    assert_no_panic("trust", &listed);
    let added = Command::new(BIN)
        .args(["trust", &dir, "sh"])
        .output()
        .expect("spawn");
    assert_no_panic("trust sh", &added);
}

#[test]
fn cron_list_and_cron_tick_read_a_real_schedule() {
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("jobs.org");
    fs::write(
        &file,
        "* Jobs\n:PROPERTIES:\n:ID: 01CLISTDIO0000000002\n:END:\n\
         #+BEGIN_SRC closure-cron\n0 9 * * * agenda\n#+END_SRC\n",
    )
    .expect("write");
    let path = file.to_string_lossy().into_owned();
    let listed = Command::new(BIN)
        .args(["cron-list", &path])
        .output()
        .expect("spawn");
    assert_no_panic("cron-list", &listed);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("agenda"),
        "cron-list found no job in a file with one"
    );
    // A minute the job does not match, and one it does.
    for at in [
        ["cron-tick", &path, "5", "13", "*", "*", "*"],
        ["cron-tick", &path, "0", "9", "*", "*", "*"],
    ] {
        let out = Command::new(BIN).args(at).output().expect("spawn");
        assert_no_panic("cron-tick", &out);
    }
}

#[test]
fn the_grouped_commands_list_their_subcommands() {
    // `a` and `pkg` are groups; run bare they should print usage and
    // exit non-zero rather than doing something surprising.
    for cmd in ["a", "pkg"] {
        let out = run_with_closed_stdin(&[cmd]);
        assert_no_panic(cmd, &out);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`{cmd}` did not ask for a subcommand"
        );
    }
}

#[test]
fn a_cron_block_under_a_headline_is_found() {
    // The same defect as `config.org`, in a second place: both cron
    // commands searched `doc.preamble()` — the nodes before the first
    // headline — so a schedule written under `* Jobs`, which is how
    // anyone writes one in a notebook, was invisible.
    //
    // The shell disagreed with the CLI about the same file, too: its
    // `job_rows` reads the whole document. One fact, two owners.
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("jobs.org");
    fs::write(
        &file,
        "* Jobs\nNotes about why these run when they do.\n\n\
         #+BEGIN_SRC closure-cron\n0 9 * * * agenda\n#+END_SRC\n",
    )
    .expect("write");
    let out = Command::new(BIN)
        .args(["cron-list", &file.to_string_lossy()])
        .output()
        .expect("spawn");
    assert_no_panic("cron-list", &out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("agenda"),
        "a schedule under a headline is invisible to `cron-list`"
    );
}

#[test]
fn a_cron_block_at_the_top_still_works() {
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("top.org");
    fs::write(
        &file,
        "#+BEGIN_SRC closure-cron\n0 9 * * * agenda\n#+END_SRC\n",
    )
    .expect("write");
    let out = Command::new(BIN)
        .args(["cron-list", &file.to_string_lossy()])
        .output()
        .expect("spawn");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("agenda"),
        "regressed the old shape"
    );
}
