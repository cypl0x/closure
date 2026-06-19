//! C1b: resource bounds on the interpreter backends. A runaway block is
//! killed at a wall-clock deadline; a flood of stdout is truncated to a
//! cap instead of buffering unboundedly. Hermetic via `/bin/sh`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use closure_eval::{Backend, Bounds, ShellBackend};

#[test]
fn infinite_loop_is_killed_at_timeout() {
    let bounds = Bounds {
        timeout: Duration::from_millis(300),
        max_output: 1 << 20,
    };
    let start = Instant::now();
    let res = ShellBackend.eval_bounded("while true; do :; done\n", bounds);
    let elapsed = start.elapsed();
    assert!(
        matches!(res, Err(closure_eval::EvalError::Timeout(_))),
        "runaway block must be killed, got {res:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "killed promptly, took {elapsed:?}"
    );
}

#[test]
fn huge_stdout_is_truncated_to_cap() {
    let cap = 4096;
    let bounds = Bounds {
        timeout: Duration::from_secs(10),
        max_output: cap,
    };
    let start = Instant::now();
    // 50 MB of zeros: terminates, but must not be buffered whole.
    let out = ShellBackend
        .eval_bounded("head -c 50000000 /dev/zero\n", bounds)
        .expect("should complete, truncated");
    assert!(
        out.stdout.len() <= cap,
        "stdout truncated to cap, got {} bytes",
        out.stdout.len()
    );
    assert!(
        start.elapsed() < Duration::from_secs(8),
        "did not hang draining the flood"
    );
}

#[test]
fn normal_output_under_caps_is_unchanged() {
    let out = ShellBackend
        .eval_bounded("printf hello\n", Bounds::default())
        .expect("eval");
    assert_eq!(out.stdout, "hello");
    assert_eq!(out.exit, 0);
}
