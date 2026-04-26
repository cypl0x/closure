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

#[test]
fn backend_for_known_languages() {
    assert!(closure_eval::backend_for("shell").is_some());
    assert!(closure_eval::backend_for("sh").is_some());
    assert!(closure_eval::backend_for("BASH").is_some());
    assert!(closure_eval::backend_for("python").is_some());
    assert!(closure_eval::backend_for("py").is_some());
    assert!(closure_eval::backend_for("rust").is_none());
}

#[test]
fn shell_backend_respects_timeout() {
    let b = ShellBackend;
    let err = b
        .eval_with_timeout("sleep 5", std::time::Duration::from_millis(100))
        .unwrap_err();
    assert!(matches!(err, closure_eval::EvalError::Timeout(_)));
}

#[test]
fn shell_backend_completes_within_timeout() {
    let b = ShellBackend;
    let out = b
        .eval_with_timeout("echo ok", std::time::Duration::from_secs(5))
        .expect("eval");
    assert_eq!(out.exit, 0);
    assert_eq!(out.stdout.trim_end(), "ok");
}
