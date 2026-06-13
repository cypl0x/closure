//! Vault-level code block evaluation: run a block, attach
//! #+RESULTS: span-preserving, keep disk and memory in sync.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use closure_store::Vault;
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    dir
}

fn file(td: &TempDir) -> PathBuf {
    td.path().join("a.org")
}

#[test]
fn eval_block_attaches_results_on_disk_and_in_memory() {
    let td = write_vault(&[(
        "a.org",
        "* H\n#+BEGIN_SRC shell\necho ran\n#+END_SRC\ntail\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "ran");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("#+RESULTS:\n: ran\n"));
    assert!(disk.contains("tail\n"), "bytes after block survive (I1)");
    let mem = v.document(&file(&td)).expect("doc").source();
    assert_eq!(mem, disk);
}

#[test]
fn eval_block_respects_var_header_args() {
    let td = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC shell :var who=world\necho \"hi $who\"\n#+END_SRC\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "hi world");
}

#[test]
fn eval_block_silent_skips_results() {
    let td = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC shell :results silent\necho quiet\n#+END_SRC\n",
    )]);
    let before = fs::read_to_string(file(&td)).expect("read");
    let mut v = Vault::open(td.path()).expect("open");
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "quiet");
    assert_eq!(
        fs::read_to_string(file(&td)).expect("read"),
        before,
        "silent leaves the file untouched"
    );
}

#[test]
fn eval_block_out_of_range_errors() {
    let td = write_vault(&[("a.org", "* no blocks\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(v.eval_block(&file(&td), 0).is_err());
}

#[test]
fn eval_block_unknown_language_errors() {
    let td = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC brainfuck\n+++\n#+END_SRC\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(v.eval_block(&file(&td), 0).is_err());
}

#[test]
fn eval_block_reeval_replaces_results() {
    let td = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC shell\necho once\n#+END_SRC\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    v.eval_block(&file(&td), 0).expect("eval");
    v.eval_block(&file(&td), 0).expect("re-eval");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert_eq!(disk.matches("#+RESULTS:").count(), 1, "no duplication");
}

#[test]
fn set_block_content_replaces_and_persists() {
    let td = write_vault(&[(
        "a.org",
        "* H\n#+BEGIN_SRC shell\necho old\n#+END_SRC\ntail\n",
    )]);
    let mut v = Vault::open(td.path()).expect("open");
    v.set_block_content(&file(&td), 0, "echo new\n").expect("set");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("echo new\n"));
    assert!(!disk.contains("echo old"));
    assert!(disk.contains("tail\n"), "neighbour bytes survive (I1)");
    let mem = v.document(&file(&td)).expect("doc").source();
    assert_eq!(mem, disk);
}

#[test]
fn set_block_content_out_of_range_errors() {
    let td = write_vault(&[("a.org", "* no blocks\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert!(v.set_block_content(&file(&td), 0, "x\n").is_err());
}
