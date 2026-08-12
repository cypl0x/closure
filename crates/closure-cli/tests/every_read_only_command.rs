//! Every read-only subcommand runs, on a vault and on an empty one.
//!
//! `closure-cli/src/main.rs` was the least-covered file in the
//! workspace by a wide margin — 1,697 of 2,721 lines unexecuted, 37.6%
//! — and the reason is arithmetic rather than neglect: 141 subcommands
//! against 41 tests. Most of those lines are arms nobody ever ran.
//!
//! What this asserts is deliberately weak and deliberately broad: the
//! command exits 0 and does not panic. That is not a claim about
//! *what* it printed, which belongs in a test per command; it is the
//! claim that the arm works at all, which nothing was making. A
//! subcommand that panics on an empty vault is a bug every one of these
//! would have found and none was there to.
//!
//! The empty-vault pass is the half that finds things. A query written
//! against a vault with content usually works; the same query against
//! nothing is where `.first().unwrap()` and division by a zero count
//! live.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

/// A vault with something of everything the queries below ask about.
fn full_vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "#+TITLE: Notes\n\
         #+PROPERTY: header-args :tangle no\n\
         #+LINK: gh https://github.com/%s\n\
         \n\
         * TODO Ship the parser :work:urgent:\n\
         SCHEDULED: <2026-06-20 Sat>\n\
         DEADLINE: <2026-07-01 Wed -2d>\n\
         :PROPERTIES:\n:ID: 01CLIFULL00000000001\n:CATEGORY: build\n:END:\n\
         body with a [[gh:cypl0x/closure]] link and a [[id:01CLIFULL00000000002]] one\n\
         a footnote[fn:1] and an entity \\alpha and maths $x^2$\n\
         [fn:1] the note\n\
         CLOCK: [2026-06-19 Fri 09:00]--[2026-06-19 Fri 10:30] =>  1:30\n\
         \n\
         | Task | Time |\n|<l>|<r>|\n| a | 1:30 |\n\
         \n\
         - alpha :: the first\n\
         1. [@3] third\n\
         - [ ] unchecked\n\
         \n\
         #+BEGIN_SRC sh\necho hello\n#+END_SRC\n\
         * DONE Write the spec [1/2]\n\
         :PROPERTIES:\n:ID: 01CLIFULL00000000002\n:END:\n",
    )
    .expect("write");
    d
}

/// A vault with one empty file — the shape that finds the panics.
fn empty_vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(d.path().join("notes.org"), "").expect("write");
    d
}

/// Subcommands that take a vault directory and only read.
const VAULT_COMMANDS: &[&str] = &[
    "agenda",
    "clock-report",
    "tag-cloud",
    "outline",
    "tree",
    "todo-list",
    "tags",
    "todos",
    "wc",
    "orphans",
    "dead-links",
    "hubs",
    "clock",
    "vault-info",
    "edges",
    "graph",
    "recent",
    "stats",
    "todo-cloud",
    "paths",
    "paths-with-todos",
    "languages",
    "archived",
    "roots",
    "comment-list",
    "scheduled",
    "deadlines",
    "widgets",
    "backlinks-report",
];

/// Subcommands that take a single file and only read.
const FILE_COMMANDS: &[&str] = &[
    "parse",
    "fmt",
    "head",
    "drawers",
    "tables",
    "lists",
    "pending",
    "blocks",
    "anchors",
    "links",
    "footnotes",
    "timestamps",
    "cookies",
    "macros",
    "drawer-ids",
    "block-args",
    "keywords",
    "properties",
    "stats-file",
    "meta",
    "nodes",
    "summary",
    "leaves",
    "depth",
    "logbook-append",
    "hash",
    "toggle-checkbox",
];

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn closure")
}

/// A crash is the thing this is looking for, and it does not look like
/// a non-zero exit — it looks like a signal and a message on stderr.
fn assert_no_panic(what: &str, out: &std::process::Output) {
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("panicked at"),
        "`closure {what}` panicked:\n{err}"
    );
}

#[test]
fn every_vault_command_survives_a_full_vault() {
    let d = full_vault();
    let path = d.path().to_string_lossy().into_owned();
    for cmd in VAULT_COMMANDS {
        let out = run(&[cmd, &path]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn every_vault_command_survives_an_empty_vault() {
    // Where `.first().unwrap()` and "divide by the number of headlines"
    // live. A query written against content usually works.
    let d = empty_vault();
    let path = d.path().to_string_lossy().into_owned();
    for cmd in VAULT_COMMANDS {
        let out = run(&[cmd, &path]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn every_vault_command_survives_a_directory_that_is_not_there() {
    // The other end of the same question. A missing vault is a typo, and
    // a typo should be an error rather than a stack trace.
    for cmd in VAULT_COMMANDS {
        let out = run(&[cmd, "/nonexistent/vault/for/a/test"]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn every_file_command_survives_a_full_file() {
    let d = full_vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    for cmd in FILE_COMMANDS {
        let out = run(&[cmd, &file]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn every_file_command_survives_an_empty_file() {
    let d = empty_vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    for cmd in FILE_COMMANDS {
        let out = run(&[cmd, &file]);
        assert_no_panic(cmd, &out);
    }
}

#[test]
fn a_command_that_needs_an_argument_says_so_rather_than_running() {
    // `doc <NAME>` was in my no-argument list and is not. Worth a test
    // rather than a quiet correction: clap exiting 2 with the usage
    // line is the right behaviour, and asserting it means a subcommand
    // cannot start silently accepting nothing.
    let out = run(&["doc"]);
    assert_no_panic("doc", &out);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("NAME"), "{err}");
}

#[test]
fn the_commands_that_need_no_argument_run() {
    for cmd in [
        "shells",
        "ui-matrix",
        "spec",
        "conformance",
        "default-config",
        "version",
        "build",
        "commands",
        "manual",
    ] {
        let out = run(&[cmd]);
        assert_no_panic(cmd, &out);
        assert!(
            out.status.success(),
            "`closure {cmd}` exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
