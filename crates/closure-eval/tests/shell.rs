#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{Backend, ShellBackend};

#[test]
fn shell_backend_echoes() {
    let b = ShellBackend;
    let out = b.eval("echo hi").expect("eval");
    assert_eq!(out.exit, 0);
    assert_eq!(out.stdout.trim_end(), "hi");
}

#[test]
fn shell_backend_non_zero_exit() {
    let b = ShellBackend;
    let out = b.eval("exit 3").expect("eval");
    assert_eq!(out.exit, 3);
}

#[test]
fn shell_backend_captures_stderr() {
    let b = ShellBackend;
    let out = b.eval("echo err 1>&2").expect("eval");
    assert_eq!(out.exit, 0);
    assert_eq!(out.stderr.trim_end(), "err");
}
