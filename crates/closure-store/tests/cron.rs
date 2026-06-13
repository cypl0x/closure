//! Vault-level cron: read a `#+BEGIN_SRC closure-cron` block from a
//! vault file into parsed jobs (full command lines incl. args).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn vault_with(body: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("jobs.org"), body).expect("write");
    dir
}

#[test]
fn cron_jobs_parses_block_with_command_lines() {
    let td = vault_with(
        "* Schedule\n#+BEGIN_SRC closure-cron\n\
         0 9 * * * capture Daily standup\n\
         */15 * * * * search urgent\n#+END_SRC\n",
    );
    let v = Vault::open(td.path()).expect("open");
    let jobs = v.cron_jobs(&td.path().join("jobs.org")).expect("jobs");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].command, "capture Daily standup");
    assert_eq!(jobs[1].command, "search urgent");
}

#[test]
fn cron_jobs_empty_without_block() {
    let td = vault_with("* No jobs here\n");
    let v = Vault::open(td.path()).expect("open");
    assert!(v.cron_jobs(&td.path().join("jobs.org")).expect("jobs").is_empty());
}

#[test]
fn cron_jobs_unknown_file_errors() {
    let td = vault_with("* x\n");
    let v = Vault::open(td.path()).expect("open");
    assert!(v.cron_jobs(&td.path().join("nope.org")).is_err());
}

#[test]
fn cron_jobs_bad_spec_errors() {
    let td = vault_with("#+BEGIN_SRC closure-cron\nnot a cron line\n#+END_SRC\n");
    let v = Vault::open(td.path()).expect("open");
    assert!(v.cron_jobs(&td.path().join("jobs.org")).is_err());
}
