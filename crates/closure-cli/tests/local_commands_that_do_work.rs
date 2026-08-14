//! The commands that read or change one file, and were never run.
//!
//! Attributing `closure-cli`'s remaining uncovered lines to functions
//! put these at the top: `eval`, `info`, `random`, `history`,
//! `logbook-append`, `edit-block`, `cron`. None of them needs a
//! network, a terminal or a subprocess — the earlier sweeps skipped
//! them because each wants a fixture shaped for it rather than the one
//! generic vault, which is a reason to write seven fixtures and not a
//! reason to leave seven commands unrun.
//!
//! Where a command has a real claim, that is what is asserted rather
//! than the exit code: `random` with the same seed picks the same
//! headline, `--dry-run` changes nothing, `--write` writes results
//! back, `logbook-append` leaves the file parseable.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().expect("spawn")
}

/// Run with `$XDG_CONFIG_HOME` pointed somewhere of our own.
///
/// `closure trust` writes there on purpose — "a vault is something
/// people can send you; the file that decides whether its code runs
/// must not be a file they can send". A test that let it reach the real
/// config would be granting the developer's account a trust it never
/// asked for, and would pass or fail depending on what was already in
/// there.
fn run_in(config_home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .expect("spawn")
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const ID_A: &str = "01LOCALCMD0000000000A";
const ID_B: &str = "01LOCALCMD0000000000B";

/// A vault with several headlines and a shell code block.
fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* First thing\n:PROPERTIES:\n:ID: {ID_A}\n:END:\nthe first body\n\
             \n* Second thing\n:PROPERTIES:\n:ID: {ID_B}\n:END:\nthe second body\n\
             \n#+NAME: hello\n#+BEGIN_SRC sh\necho hello-from-the-block\n#+END_SRC\n"
        ),
    )
    .expect("write");
    d
}

#[test]
fn info_describes_the_headline_with_that_id() {
    let d = vault();
    let file = d.path().join("notes.org");
    let out = run(&["info", file.to_str().unwrap(), ID_A]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("First thing"), "{t}");
}

#[test]
fn info_fails_on_an_id_that_is_not_there() {
    // Every one of these looks an id up, and a typo is the ordinary way
    // to get a wrong one. Failing is right; panicking is not.
    let d = vault();
    let file = d.path().join("notes.org");
    let out = run(&["info", file.to_str().unwrap(), "01NOSUCHIDATALL000000"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("panicked"));
}

#[test]
fn random_with_the_same_seed_picks_the_same_headline() {
    // The entire point of taking a seed. If it drifted, "today's
    // headline" would change every time you looked at it.
    let d = vault();
    let dir = d.path().to_str().unwrap();
    let a = run(&["random", dir, "2026-08-14"]);
    let b = run(&["random", dir, "2026-08-14"]);
    assert!(a.status.success(), "{}", text(&a));
    assert_eq!(a.stdout, b.stdout, "same seed gave two different picks");
}

#[test]
fn random_on_an_empty_vault_is_not_a_panic() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(d.path().join("empty.org"), "").expect("write");
    let out = run(&["random", d.path().to_str().unwrap(), "2026-08-14"]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

/// A vault whose shell blocks this test is allowed to run.
///
/// Untrusted is the default and the right one: `eval` refused every
/// block until told otherwise, which is the gate working. Trusting
/// happens in a config home belonging to the test.
fn trusted() -> (TempDir, TempDir) {
    let v = vault();
    let cfg = tempfile::tempdir().expect("tempdir");
    let out = run_in(cfg.path(), &["trust", v.path().to_str().unwrap(), "shell"]);
    assert!(out.status.success(), "trust failed: {}", text(&out));
    (v, cfg)
}

#[test]
fn eval_refuses_an_untrusted_vault_rather_than_running_it() {
    // The gate itself, asserted so that trusting in the tests below
    // cannot quietly become the only thing they prove.
    let d = vault();
    let cfg = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("notes.org");
    let out = run_in(cfg.path(), &["eval", file.to_str().unwrap()]);
    assert!(!out.status.success(), "ran code from an untrusted vault");
    assert!(text(&out).contains("trust"), "{}", text(&out));
}

#[test]
fn eval_runs_a_code_block_and_prints_what_it_said() {
    let (d, cfg) = trusted();
    let file = d.path().join("notes.org");
    let out = run_in(cfg.path(), &["eval", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("hello-from-the-block"),
        "eval printed no output from the block: {}",
        text(&out)
    );
}

#[test]
fn eval_of_one_named_block_runs_that_block() {
    let (d, cfg) = trusted();
    let file = d.path().join("notes.org");
    let out = run_in(
        cfg.path(),
        &["eval", file.to_str().unwrap(), "--block", "hello"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("hello-from-the-block"),
        "{}",
        text(&out)
    );
}

#[test]
fn eval_with_write_puts_the_results_in_the_file() {
    // The claim `--write` makes. Without this the flag could do nothing
    // and every test would still pass.
    let (d, cfg) = trusted();
    let file = d.path().join("notes.org");
    let before = fs::read_to_string(&file).expect("read");
    let out = run_in(cfg.path(), &["eval", file.to_str().unwrap(), "--write"]);
    assert!(out.status.success(), "{}", text(&out));
    let after = fs::read_to_string(&file).expect("read");
    assert_ne!(before, after, "--write wrote nothing");
    assert!(after.contains("#+RESULTS:"), "{after}");

    // And what it wrote is still a document closure can read.
    let check = run(&["parse", file.to_str().unwrap()]);
    assert!(check.status.success(), "{}", text(&check));
}

#[test]
fn eval_of_a_block_index_that_is_not_there_fails_without_panicking() {
    let (d, cfg) = trusted();
    let file = d.path().join("notes.org");
    let out = run_in(
        cfg.path(),
        &["eval", file.to_str().unwrap(), "--block", "999"],
    );
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn logbook_append_adds_the_entry_and_leaves_the_file_readable() {
    let d = vault();
    let file = d.path().join("notes.org");
    let out = run(&[
        "logbook-append",
        file.to_str().unwrap(),
        ID_A,
        "a note about the work",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let after = fs::read_to_string(&file).expect("read");
    assert!(after.contains("a note about the work"), "{after}");
    let check = run(&["parse", file.to_str().unwrap()]);
    assert!(
        check.status.success(),
        "the logbook write left a file closure cannot parse: {}",
        text(&check)
    );
}

#[test]
fn logbook_append_refuses_an_id_that_is_not_there_and_changes_nothing() {
    let d = vault();
    let file = d.path().join("notes.org");
    let before = fs::read_to_string(&file).expect("read");
    let out = run(&[
        "logbook-append",
        file.to_str().unwrap(),
        "01NOSUCHIDATALL000000",
        "nope",
    ]);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(before, fs::read_to_string(&file).expect("read"));
}

#[test]
fn edit_block_prints_the_block_at_that_index() {
    let d = vault();
    let out = run(&["edit-block", d.path().to_str().unwrap(), "notes.org", "0"]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn edit_block_with_an_index_past_the_end_fails_rather_than_panicking() {
    let d = vault();
    let out = run(&[
        "edit-block",
        d.path().to_str().unwrap(),
        "notes.org",
        "9999",
    ]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("panicked"));
}

/// A vault that records what it is asked to do.
///
/// Off by default, which is why the first version of these tests found
/// an empty journal and a `--grep` that narrowed nothing: there was
/// nothing to narrow. The switch is per-vault and lives in its
/// `config.org`.
fn recording_vault() -> TempDir {
    let d = vault();
    fs::write(
        d.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nrecord_commands = true\n#+END_SRC\n",
    )
    .expect("write config");
    d
}

#[test]
fn history_lists_the_journal_and_grep_narrows_it() {
    let d = recording_vault();
    let dir = d.path().to_str().unwrap();
    // Give the journal something to hold.
    let cap = run(&["capture", dir, "a captured thought"]);
    assert!(cap.status.success(), "{}", text(&cap));

    let all = run(&["history", dir]);
    assert!(all.status.success(), "{}", text(&all));
    assert!(
        !all.stdout.is_empty(),
        "nothing was journalled, so the comparison below proves nothing"
    );

    let hit = run(&["history", dir, "--grep", "captured"]);
    assert!(hit.status.success(), "{}", text(&hit));
    let miss = run(&["history", dir, "--grep", "zzz-nothing-matches-this"]);
    assert!(miss.status.success(), "{}", text(&miss));
    assert!(
        miss.stdout.len() < all.stdout.len(),
        "grep for nothing returned as much as no grep at all"
    );
}

#[test]
fn history_replay_dry_run_changes_nothing() {
    // The whole promise of --dry-run, and the one that costs notes if
    // it is wrong.
    let d = recording_vault();
    let dir = d.path().to_str().unwrap();
    run(&["capture", dir, "a captured thought"]);
    let before: Vec<_> = fs::read_dir(d.path())
        .expect("read dir")
        .filter_map(|e| {
            let e = e.ok()?;
            Some((e.path(), e.metadata().ok()?.len()))
        })
        .collect();

    let out = run(&["history", dir, "--replay", "--dry-run"]);
    assert!(out.status.success(), "{}", text(&out));

    let after: Vec<_> = fs::read_dir(d.path())
        .expect("read dir")
        .filter_map(|e| {
            let e = e.ok()?;
            Some((e.path(), e.metadata().ok()?.len()))
        })
        .collect();
    assert_eq!(before, after, "--dry-run changed the vault");
}

#[test]
fn cron_fires_only_the_jobs_whose_time_has_come() {
    // `--at` exists so this is deterministic. Without it the test would
    // pass or fail depending on the minute it ran in.
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("jobs.org"),
        "* Jobs\n#+BEGIN_SRC closure-cron\n0 9 * * * capture morning-job\n\
         30 17 * * * capture evening-job\n#+END_SRC\n",
    )
    .expect("write");

    let out = run(&[
        "cron",
        d.path().to_str().unwrap(),
        "jobs.org",
        // Concrete numbers, not a pattern: `--at` says what time it
        // *is*, and the jobs carry the patterns. `*` here is rejected
        // with "bad time field", which is right — a wildcard for the
        // current minute means nothing.
        "--at",
        "0 9 14 8 5",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("morning-job"), "the 09:00 job did not fire: {t}");
    assert!(
        !t.contains("evening-job"),
        "the 17:30 job fired at 09:00: {t}"
    );
}

#[test]
fn cron_on_a_file_with_no_jobs_is_quiet_and_succeeds() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(d.path().join("jobs.org"), "* Nothing scheduled here\n").expect("write");
    let out = run(&["cron", d.path().to_str().unwrap(), "jobs.org"]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}
