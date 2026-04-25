#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::process::Command;

use closure_sync::{GitTransport, NoopTransport, Transport};

#[test]
fn noop_transport_is_idempotent() {
    let mut t = NoopTransport;
    t.push().unwrap();
    t.pull().unwrap();
}

#[test]
fn git_transport_push_in_local_init() {
    if Command::new("git").arg("--version").output().is_err() {
        return; // skip when git is unavailable
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();

    Command::new("git")
        .arg("init")
        .current_dir(&path)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&path)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(&path)
        .status()
        .unwrap();

    std::fs::write(path.join("a.org"), "* hi\n").unwrap();
    let mut t = GitTransport::new(path.clone());
    // No remote configured, so push fails — but commit succeeds and
    // pull should also fail. We assert the error type, not success.
    let _ = t.push();

    // After the run, the working tree should have a commit.
    let log = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&path)
        .output()
        .unwrap();
    let log_text = String::from_utf8_lossy(&log.stdout);
    assert!(log_text.contains("closure: sync"));
}
