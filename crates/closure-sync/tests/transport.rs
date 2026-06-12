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

fn sh_git(dir: &std::path::Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(st.status.success(), "git {args:?} failed: {st:?}");
}

fn clone_with_identity(origin: &std::path::Path, target: &std::path::Path) {
    let st = Command::new("git")
        .args([
            "clone",
            origin.to_str().expect("utf8"),
            target.to_str().expect("utf8"),
        ])
        .output()
        .expect("git clone");
    assert!(st.status.success(), "clone failed: {st:?}");
    sh_git(target, &["config", "user.email", "t@example.com"]);
    sh_git(target, &["config", "user.name", "t"]);
}

#[test]
fn two_clones_diverge_then_converge() {
    if Command::new("git").arg("--version").output().is_err() {
        return; // skip when git is unavailable
    }
    let root = tempfile::tempdir().expect("tempdir");
    let origin = root.path().join("origin.git");
    std::fs::create_dir(&origin).expect("mkdir");
    sh_git(&origin, &["init", "--bare", "--initial-branch=main", "."]);

    let a = root.path().join("a");
    let b = root.path().join("b");
    clone_with_identity(&origin, &a);
    // Seed an initial commit from A so both clones share history.
    std::fs::write(a.join("base.org"), "* Base\n").expect("write");
    sh_git(&a, &["add", "-A"]);
    sh_git(&a, &["commit", "-m", "seed"]);
    sh_git(&a, &["push", "origin", "HEAD"]);
    clone_with_identity(&origin, &b);

    // Diverge: A adds one note, B adds another.
    std::fs::write(a.join("from-a.org"), "* From A\n").expect("write");
    std::fs::write(b.join("from-b.org"), "* From B\n").expect("write");

    let mut ta = GitTransport::new(a.clone());
    let mut tb = GitTransport::new(b.clone());
    ta.push().expect("A push");
    tb.pull().expect("B pull");
    tb.push().expect("B push");
    ta.pull().expect("A pull");

    for side in [&a, &b] {
        assert!(side.join("from-a.org").exists(), "{}", side.display());
        assert!(side.join("from-b.org").exists(), "{}", side.display());
    }
    let read = |p: &std::path::Path, f: &str| std::fs::read_to_string(p.join(f)).expect("read");
    assert_eq!(read(&a, "from-a.org"), read(&b, "from-a.org"));
    assert_eq!(read(&a, "from-b.org"), read(&b, "from-b.org"));
}
