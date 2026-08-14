//! The launcher binary, in the build everybody actually has.
//!
//! `main.rs` is four lines of logic and none of them had run. The
//! default build is Qt-free on purpose (I10 — the hermetic build must
//! not need system Qt), so the binary the workspace produces is
//! exactly the one that *cannot* open a window, and the only thing it
//! can do is say so clearly.
//!
//! That makes these two messages the entire user-facing behaviour of
//! this binary for anyone who has not opted into the feature. A wrong
//! or missing one is somebody staring at a program that exits with no
//! explanation.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_closure-shell-qt");

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(BIN).args(args).output().expect("spawn");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn with_no_vault_it_prints_the_usage_and_fails() {
    let (ok, text) = run(&[]);
    assert!(!ok, "exited successfully with no vault: {text}");
    assert!(
        text.contains("usage:"),
        "no usage line for a missing argument: {text}"
    );
    assert!(text.contains("closure-shell-qt"), "{text}");
}

#[test]
fn without_the_feature_it_says_which_feature_to_rebuild_with() {
    // The default build. "Rebuild with --features qt" is the whole
    // answer, and a message that only said "not supported" would leave
    // somebody with nowhere to go.
    let (ok, text) = run(&["/some/vault"]);
    assert!(!ok, "a Qt-free build claimed to open a window: {text}");
    #[cfg(not(feature = "qt"))]
    {
        assert!(text.contains("qt"), "{text}");
        assert!(
            text.contains("--features"),
            "the message does not say how to fix it: {text}"
        );
    }
}

#[test]
fn it_never_panics_whatever_it_is_given() {
    for args in [
        vec![],
        vec!["/nonexistent/vault"],
        vec![""],
        vec!["/tmp", "extra", "arguments"],
    ] {
        let (_, text) = run(&args);
        assert!(!text.contains("panicked"), "{args:?}: {text}");
    }
}
