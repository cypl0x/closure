//! LLM-facing vault tool surface: one text line in, one text result
//! out. Mutations route through the same kernel-command vault methods
//! as every other shell (I8).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

fn id_of(v: &Vault, title: &str) -> String {
    v.find_by_title(title).expect("exists").0.id().to_string()
}

#[test]
fn list_files_names_every_path() {
    let (_td, mut v) = vault();
    let out = v.run_tool("list-files");
    assert!(out.contains("notes.org"));
}

#[test]
fn read_returns_file_source() {
    let (_td, mut v) = vault();
    let out = v.run_tool("read notes.org");
    assert!(out.contains("* TODO Ship parser"));
}

#[test]
fn read_missing_file_is_error_text() {
    let (_td, mut v) = vault();
    let out = v.run_tool("read nope.org");
    assert!(out.starts_with("ERROR"), "got {out}");
}

#[test]
fn search_lists_matching_titles_with_ids() {
    let (_td, mut v) = vault();
    let out = v.run_tool("search wiki");
    assert!(out.contains("Personal wiki"));
    assert!(!out.contains("Ship parser"));
}

#[test]
fn capture_appends_and_reports_id() {
    let (_td, mut v) = vault();
    let out = v.run_tool("capture Buy milk");
    assert!(out.starts_with("OK"), "got {out}");
    assert!(v.find_by_title("Buy milk").is_some());
}

#[test]
fn rename_changes_title() {
    let (_td, mut v) = vault();
    let id = id_of(&v, "Personal wiki");
    let out = v.run_tool(&format!("rename {id} Shared wiki"));
    assert!(out.starts_with("OK"), "got {out}");
    assert!(v.find_by_title("Shared wiki").is_some());
}

#[test]
fn set_property_writes_drawer() {
    let (_td, mut v) = vault();
    let id = id_of(&v, "Ship parser");
    let out = v.run_tool(&format!("set-property {id} EFFORT 2d"));
    assert!(out.starts_with("OK"), "got {out}");
    let (h, _) = v.find_by_title("Ship parser").expect("still there");
    assert_eq!(h.property("EFFORT"), Some("2d"));
}

#[test]
fn unknown_tool_is_error_with_help() {
    let (_td, mut v) = vault();
    let out = v.run_tool("frobnicate");
    assert!(out.starts_with("ERROR"));
    assert!(out.contains("list-files"), "error names available tools");
}

#[test]
fn malformed_args_are_error_text_not_panic() {
    let (_td, mut v) = vault();
    assert!(v.run_tool("rename").starts_with("ERROR"));
    assert!(v.run_tool("set-property onlyid").starts_with("ERROR"));
    assert!(v.run_tool("capture").starts_with("ERROR"));
}
