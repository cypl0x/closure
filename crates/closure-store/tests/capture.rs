//! org-capture: templated append into a vault file.
//!
//! Invariants: existing bytes are preserved as an exact prefix (I1),
//! every captured entry carries a fresh `:ID:` (I2), and the vault's
//! in-memory document mirrors the file on disk after the capture.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use closure_store::{CaptureTemplate, Vault};
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    dir
}

fn todo_template(target: &str) -> CaptureTemplate {
    CaptureTemplate {
        target: PathBuf::from(target),
        headline_prefix: "TODO ".to_owned(),
        body: String::new(),
    }
}

#[test]
fn capture_appends_to_existing_file_preserving_bytes() {
    let td = write_vault(&[("a.org", "* A\nbody\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    v.capture(&todo_template("a.org"), "Buy milk").expect("capture");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.starts_with("* A\nbody\n"), "old bytes must be a prefix");
    assert!(disk.contains("Buy milk"));
    assert!(disk.contains(":ID:"));
}

#[test]
fn capture_entry_has_todo_keyword_and_title() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id = v.capture(&todo_template("a.org"), "Buy milk").expect("capture");
    let (h, path) = v.find_by_id(&id).expect("captured headline resolvable");
    assert_eq!(h.title(), "Buy milk");
    assert_eq!(h.todo(), Some("TODO"));
    assert!(path.ends_with("a.org"));
}

#[test]
fn capture_creates_missing_target_file() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 1);
    v.capture(&todo_template("inbox.org"), "First").expect("capture");
    assert_eq!(v.len(), 2, "new file joins the vault");
    assert!(td.path().join("inbox.org").exists());
}

#[test]
fn capture_inserts_separating_newline_when_missing() {
    let td = write_vault(&[("a.org", "* A")]);
    let mut v = Vault::open(td.path()).expect("open");
    v.capture(&todo_template("a.org"), "Next").expect("capture");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.starts_with("* A\n"), "got {disk:?}");
    assert!(!disk.contains("* A* "), "headlines must not merge");
}

#[test]
fn capture_includes_template_body() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let tpl = CaptureTemplate {
        target: PathBuf::from("a.org"),
        headline_prefix: String::new(),
        body: "- [ ] first step\n".to_owned(),
    };
    v.capture(&tpl, "Plan").expect("capture");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    assert!(disk.contains("- [ ] first step"));
}

#[test]
fn capture_ids_are_unique() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let id1 = v.capture(&todo_template("a.org"), "One").expect("capture");
    let id2 = v.capture(&todo_template("a.org"), "Two").expect("capture");
    assert_ne!(id1, id2);
}

#[test]
fn capture_keeps_vault_and_disk_in_sync() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    v.capture(&todo_template("a.org"), "Synced").expect("capture");
    let disk = fs::read_to_string(td.path().join("a.org")).expect("read");
    let mem = v
        .document(&td.path().join("a.org"))
        .expect("doc cached")
        .source();
    assert_eq!(mem, disk);
}

#[test]
fn captured_file_reopens_cleanly() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    v.capture(&todo_template("a.org"), "Reopen me").expect("capture");
    let reopened = Vault::open(td.path()).expect("reopen");
    assert_eq!(reopened.len(), 1);
    assert!(reopened.find_by_title("Reopen me").is_some());
}
