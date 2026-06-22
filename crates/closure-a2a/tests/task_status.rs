//! V8b: A2A task lifecycle. A delegated task carries a state
//! (submitted → working → done/failed) so a caller can poll progress.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_a2a::{Task, TaskState, handle_message};
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("notes.org"), "* TODO Ship\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn submitted_task_starts_in_submitted_state() {
    let t = Task::submit("list-files");
    assert_eq!(t.state, TaskState::Submitted);
}

#[test]
fn running_a_good_task_reaches_done() {
    let (_d, mut v) = vault();
    let mut t = Task::submit("list-files");
    t.run(&mut v);
    assert_eq!(t.state, TaskState::Done);
    assert!(t.result.contains("notes.org"), "carries the result");
}

#[test]
fn running_a_failing_task_reaches_failed() {
    let (_d, mut v) = vault();
    let mut t = Task::submit("read does-not-exist.org");
    t.run(&mut v);
    assert_eq!(t.state, TaskState::Failed, "ERROR result → Failed");
}

#[test]
fn protocol_task_delegate_carries_state() {
    let (_d, mut v) = vault();
    let resp = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":1,"method":"task/delegate","params":{"task":"list-files"}}"#,
    )
    .expect("response");
    assert!(
        resp.contains("\"state\":\"done\""),
        "state in response: {resp}"
    );
    assert!(resp.contains("notes.org"), "result text: {resp}");
}
