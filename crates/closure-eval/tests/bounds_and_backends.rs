//! The bounds a block runs under, and what happens at them.
//!
//! `Bounds` is the thing standing between a vault and a `while true`
//! in somebody's note: a wall-clock deadline and a cap on retained
//! output. Both were reachable and neither had a test that reached
//! them — 144 of `closure-eval`'s 643 lines were unexecuted, and the
//! timeout and truncation paths were most of that.
//!
//! These are the paths where "it worked when I tried it" is worth
//! least. A runaway block is not a case anybody exercises by hand, and
//! it is the case that decides whether closure can be trusted to open
//! a file somebody else wrote.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::Duration;

use closure_eval::{Backend as _, Bounds, EvalError, ShellBackend};

#[test]
fn a_block_that_finishes_returns_its_output() {
    let out = ShellBackend
        .eval_bounded("echo hello", Bounds::default())
        .expect("ran");
    assert!(out.stdout.contains("hello"), "{out:?}");
}

#[test]
fn a_block_that_runs_too_long_is_killed() {
    // The whole reason `Bounds` exists. Without this, a `while true` in
    // a note somebody sent you hangs the window that opened it.
    let bounds = Bounds {
        timeout: Duration::from_millis(200),
        ..Bounds::default()
    };
    let err = ShellBackend
        .eval_bounded("sleep 30", bounds)
        .expect_err("a sleep of 30s finished inside 200ms");
    assert!(matches!(err, EvalError::Timeout(_)), "{err:?}");
}

#[test]
fn the_timeout_error_says_how_long_it_waited() {
    // An error that says only "timeout" leaves the reader wondering
    // whether the limit is theirs to change.
    let bounds = Bounds {
        timeout: Duration::from_millis(150),
        ..Bounds::default()
    };
    let err = ShellBackend.eval_bounded("sleep 30", bounds).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("150") || msg.contains("0.15"), "{msg}");
}

#[test]
fn output_beyond_the_cap_does_not_come_back_whole() {
    // `yes` produces bytes faster than anything can consume them. The
    // cap is what stops a block filling memory with somebody's mistake.
    let bounds = Bounds {
        timeout: Duration::from_secs(5),
        max_output: 4096,
    };
    match ShellBackend.eval_bounded("yes closure", bounds) {
        Ok(out) => assert!(
            out.stdout.len() <= 64 * 1024,
            "kept {} bytes of an endless stream",
            out.stdout.len()
        ),
        // Killing it is an equally correct answer to an endless stream.
        Err(EvalError::Timeout(_)) => {}
        Err(e) => panic!("unexpected: {e:?}"),
    }
}

#[test]
fn a_failing_command_reports_rather_than_pretending() {
    let out = ShellBackend
        .eval_bounded("exit 3", Bounds::default())
        .expect("a non-zero exit is still a completed run");
    assert_ne!(out.exit, 0, "{out:?}");
}

#[test]
fn stderr_is_kept_separately_from_stdout() {
    // A block whose diagnostics were folded into its output would make
    // every result that mentioned a warning unusable as a value.
    let out = ShellBackend
        .eval_bounded("echo out; echo err 1>&2", Bounds::default())
        .expect("ran");
    assert!(out.stdout.contains("out"), "{out:?}");
    assert!(out.stderr.contains("err"), "{out:?}");
    assert!(!out.stdout.contains("err"), "{out:?}");
}

#[test]
fn the_default_bounds_are_the_ones_documented() {
    // They are a promise about what closure will let a note do, so a
    // change to either should be deliberate enough to update a test.
    let d = Bounds::default();
    assert_eq!(d.timeout, Duration::from_secs(10));
    assert_eq!(d.max_output, 10 * 1024 * 1024);
}

#[test]
fn a_language_nobody_has_a_backend_for_is_not_run() {
    // I5 and the trust check together: an unknown language must be a
    // refusal, not a shell-out with the source as the command.
    assert!(closure_eval::backend_for("brainfuck").is_none());
}

#[test]
fn the_language_name_is_canonicalised_before_it_is_matched() {
    // `sh`, `bash`, `shell` are one backend, and a trust list naming
    // one should not be bypassed by writing another.
    let a = closure_eval::canonical_language("bash");
    let b = closure_eval::canonical_language("sh");
    assert_eq!(a, b, "bash and sh canonicalise differently");
}

#[test]
fn an_untrusted_language_is_refused_before_it_runs() {
    assert!(!closure_eval::eval_allowed(&[], "sh"));
    assert!(closure_eval::eval_allowed(&["sh".to_owned()], "sh"));
}
