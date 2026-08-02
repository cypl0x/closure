//! "commands like `:! xdg-open .` freezes the whole app"
//!
//! `xdg-open` exits almost immediately. What it leaves behind is a
//! file manager holding the *write end* of the pipe it inherited — so
//! the read end never sees EOF, and every path that collects a
//! command's output by reading to EOF waits for a program the user
//! opened on purpose and will close in ten minutes.
//!
//! `run_bounded` polls the child and honours a deadline, which looks
//! like enough and is not: once the child exits it *joins* the drain
//! threads, and those are exactly the reads that never finish. The
//! deadline never even comes into it, because the child did exit.
//!
//! So the escape stops waiting on the pipe once the process it started
//! is gone. Whatever arrived by then is the output; a grandchild that
//! keeps the pipe open for its own reasons is not the shell's problem
//! and must never be the user's.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::{Duration, Instant};

use closure_eval::shell_escape;

/// Generous enough that a slow machine does not fail the suite, tight
/// enough that a hang is unmistakable.
const PATIENCE: Duration = Duration::from_secs(5);

#[test]
fn a_command_that_prints_and_exits_gives_its_output() {
    let out = shell_escape("echo hello", Duration::from_secs(5)).expect("ran");
    assert_eq!(out.stdout.trim(), "hello");
    assert_eq!(out.exit, 0);
}

#[test]
fn stderr_comes_back_too() {
    let out = shell_escape("echo oops >&2", Duration::from_secs(5)).expect("ran");
    assert_eq!(out.stderr.trim(), "oops");
}

#[test]
fn a_failing_command_reports_its_code_rather_than_erroring() {
    let out = shell_escape("exit 3", Duration::from_secs(5)).expect("ran");
    assert_eq!(out.exit, 3);
}

#[test]
fn a_child_that_leaves_the_pipe_open_does_not_hang_the_shell() {
    // The report, reduced to its mechanism: a command that exits at
    // once but leaves a grandchild holding the inherited pipe. This is
    // exactly `xdg-open .`, and it is what hung.
    let started = Instant::now();
    let out = shell_escape(
        "sh -c 'sleep 30' & echo started; exit 0",
        Duration::from_secs(5),
    )
    .expect("ran");
    assert!(
        started.elapsed() < PATIENCE,
        "waited {:?} on a grandchild's pipe",
        started.elapsed()
    );
    assert!(out.stdout.contains("started"), "{out:?}");
}

#[test]
fn a_command_that_never_exits_is_still_bounded() {
    // The other half: the child itself running forever. The deadline
    // is what covers this one, and it has to actually fire.
    let started = Instant::now();
    let err = shell_escape("sleep 30", Duration::from_millis(300));
    assert!(err.is_err(), "a runaway command has to be cut off");
    assert!(
        started.elapsed() < PATIENCE,
        "waited {:?}",
        started.elapsed()
    );
}

#[test]
fn output_written_before_the_grandchild_is_not_lost() {
    // Bounding the wait must not cost the output that did arrive —
    // that would trade a hang for a lie.
    let out = shell_escape(
        "echo first; echo second; sh -c 'sleep 30' & exit 0",
        Duration::from_secs(5),
    )
    .expect("ran");
    assert!(out.stdout.contains("first"), "{out:?}");
    assert!(out.stdout.contains("second"), "{out:?}");
}
