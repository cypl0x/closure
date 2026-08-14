//! Every command, handed something it cannot use.
//!
//! The remaining uncovered regions in `closure-cli` are mostly not
//! whole functions — they are the `.map_err(|e| format!(...))` closures
//! hanging off lines that *are* covered. A line-level reading calls
//! those "never executed" and makes it look like sixty-five commands
//! were never run; what was never run is the error arm of a command
//! that always succeeded in every test written for it.
//!
//! That distinction matters beyond the number. Each of those closures
//! is a sentence a user can be shown, and not one of them had ever been
//! produced. A `format!` that panics, names the wrong path, or says
//! nothing useful is invisible until the day something goes wrong,
//! which is the worst day to discover it.
//!
//! The table is not written by hand. This asks the binary what shape
//! each subcommand has — `closure <cmd> --help` names its positionals —
//! and feeds the first one something broken. A command added tomorrow
//! is swept tomorrow without anybody remembering to add it here.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

/// Commands this sweep must not run, and why.
///
/// Every entry is a thing that does not return on its own: a server, a
/// window, a terminal UI, or a watch loop. They are covered where it is
/// possible to cover them — `serve` in
/// `socket_and_registry_commands.rs`, the stdio servers in
/// `stdio_and_setup_commands.rs` — and a sweep that spawned one would
/// hang the suite rather than fail it, which is much harder to
/// diagnose.
const DOES_NOT_RETURN: &[&str] = &[
    "tui", "gpui", "egui", "serve", "mcp", "acp", "lsp", "a2a", "chat", "watch", "sniff", "build",
    "manual", "help",
];

/// Ask the binary which positionals a subcommand takes.
fn positionals(cmd: &str) -> Vec<String> {
    let out = Command::new(BIN)
        .args([cmd, "--help"])
        .output()
        .expect("spawn --help");
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(usage) = text.lines().find(|l| l.trim_start().starts_with("Usage:")) else {
        return Vec::new();
    };
    // `Usage: closure info <FILE> <ID>` -> ["<FILE>", "<ID>"]
    usage
        .split_whitespace()
        .filter(|w| w.starts_with('<') || w.starts_with('['))
        .map(|w| w.trim_matches(['<', '>', '[', ']']).to_owned())
        .collect()
}

/// Every subcommand `closure --help` lists.
fn subcommands() -> Vec<String> {
    let out = Command::new(BIN).arg("--help").output().expect("spawn");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in text.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if line.starts_with("Options:") {
                break;
            }
            let t = line.trim_start();
            if line.starts_with("  ")
                && let Some(name) = t.split_whitespace().next()
                && name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            {
                names.push(name.to_owned());
            }
        }
    }
    assert!(names.len() > 80, "only found {} subcommands", names.len());
    names
}

/// Plausible-looking filler for a positional this sweep is not aiming
/// at, so the command reaches its own code instead of clap's usage
/// error.
fn filler(name: &str) -> String {
    match name {
        "ID" | "FROM" | "TO" => "01NOSUCHIDATALL000000".to_owned(),
        "INDEX" | "N" | "SEED" | "DEPTH" => "0".to_owned(),
        _ => "x".to_owned(),
    }
}

fn run_with_timeout(args: &[String]) -> Option<String> {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(_status) = child.try_wait().expect("wait") {
            let out = child.wait_with_output().expect("output");
            return Some(format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run every sweepable command with `bad` in its first positional.
fn sweep(bad: &str, label: &str) {
    let mut ran = 0;
    let mut hung = Vec::new();
    let mut panicked = Vec::new();

    for cmd in subcommands() {
        if DOES_NOT_RETURN.contains(&cmd.as_str()) {
            continue;
        }
        let pos = positionals(&cmd);
        if pos.is_empty() {
            continue;
        }
        let mut args = vec![cmd.clone(), bad.to_owned()];
        for p in pos.iter().skip(1) {
            args.push(filler(p));
        }

        match run_with_timeout(&args) {
            None => hung.push(cmd.clone()),
            Some(text) => {
                if text.contains("panicked at") || text.contains("RUST_BACKTRACE") {
                    panicked.push(format!(
                        "{cmd}: {}",
                        text.lines().take(3).collect::<Vec<_>>().join(" | ")
                    ));
                }
            }
        }
        ran += 1;
    }

    assert!(ran > 70, "{label}: only swept {ran} commands");
    assert!(hung.is_empty(), "{label}: these never returned: {hung:?}");
    assert!(
        panicked.is_empty(),
        "{label}: these panicked instead of reporting:\n{}",
        panicked.join("\n")
    );
}

#[test]
fn no_command_panics_on_a_path_that_is_not_there() {
    // The ordinary typo. Every one of these opens something.
    sweep("/nonexistent/path/for/the/error/sweep", "absent path");
}

#[test]
fn no_command_panics_when_given_a_directory_instead_of_a_file() {
    // A different errno from "not there", and a different arm in some
    // of these — `read_to_string` on a directory is EISDIR, which is
    // the error a shell glob produces when it matches one thing too
    // many.
    let d = tempfile::tempdir().expect("tempdir");
    sweep(&d.path().to_string_lossy(), "a directory");
}

#[test]
fn no_command_panics_on_a_file_that_is_not_utf8() {
    // The one input that fails at `read_to_string` rather than at the
    // parser, so it reaches a different closure. Org is permissive
    // enough that almost nothing else does.
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("notes.org");
    fs::write(&f, [0xff, 0xfe, 0x00, 0x80, 0x81]).expect("write");
    sweep(&f.to_string_lossy(), "not utf-8");
}

#[test]
fn no_command_panics_on_an_empty_file() {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("notes.org");
    fs::write(&f, "").expect("write");
    sweep(&f.to_string_lossy(), "an empty file");
}

#[test]
fn every_swept_command_says_something_when_it_fails() {
    // A command that exits non-zero and prints nothing leaves the user
    // with an exit code and no idea which of its arguments was wrong.
    let mut silent = Vec::new();
    for cmd in subcommands() {
        if DOES_NOT_RETURN.contains(&cmd.as_str()) {
            continue;
        }
        let pos = positionals(&cmd);
        if pos.is_empty() {
            continue;
        }
        let mut args = vec![
            cmd.clone(),
            "/nonexistent/path/for/the/error/sweep".to_owned(),
        ];
        for p in pos.iter().skip(1) {
            args.push(filler(p));
        }
        let out = Command::new(BIN)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("spawn");
        if !out.status.success() && out.stdout.is_empty() && out.stderr.is_empty() {
            silent.push(cmd);
        }
    }
    assert!(
        silent.is_empty(),
        "these failed without saying why: {silent:?}"
    );
}
