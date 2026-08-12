//! Every writing subcommand changes the file, and leaves it parseable.
//!
//! The read-only pass took `main.rs` from 37.6% to 51.9%; what it could
//! not reach is the half that writes. Those arms need a fixture each
//! and an assertion about the file afterwards, which is the point:
//! "exits 0" is a much weaker claim for a command whose job is to
//! change a document than for one whose job is to print it.
//!
//! So each case asserts three things. The command succeeds; the file
//! *changed*; and the result still parses, because a rewrite that
//! produces something closure cannot read back is the one failure mode
//! that loses somebody's notes rather than merely annoying them. I1
//! says the parse is byte-exact, so `check` is the honest gate on that.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

const ID_A: &str = "01CLIWRITE0000000001";
const ID_B: &str = "01CLIWRITE0000000002";

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            // A checkbox *before* the first headline: `toggle-checkbox`
            // takes the Nth preamble list item, not one in a body, and
            // a fixture without one made it fail with "no headline at
            // the given path" — which is a true message about a
            // question I was not asking.
            "- [ ] a preamble box\n\
             * TODO First thing :work:\n\
             :PROPERTIES:\n:ID: {ID_A}\n:END:\n\
             the body\n\
             - [ ] a box\n\
             ** A child\n\
             :PROPERTIES:\n:ID: {ID_B}\n:END:\n"
        ),
    )
    .expect("write");
    d
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn closure")
}

/// Run one writing command and hold it to all three claims.
fn changes_the_file(what: &str, args: &[&str]) {
    let d = vault();
    let file = d.path().join("notes.org");
    let before = fs::read_to_string(&file).expect("read");
    let full: Vec<String> = args
        .iter()
        .map(|a| match *a {
            "<FILE>" => file.to_string_lossy().into_owned(),
            "<VAULT>" => d.path().to_string_lossy().into_owned(),
            other => other.to_owned(),
        })
        .collect();
    let borrowed: Vec<&str> = full.iter().map(String::as_str).collect();
    let out = run(&borrowed);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked at"), "`{what}` panicked:\n{err}");
    assert!(out.status.success(), "`{what}` failed: {err}");

    let after = fs::read_to_string(&file).expect("read back");
    assert_ne!(
        before, after,
        "`{what}` reported success and changed nothing"
    );

    // The claim that matters: closure can still read what it wrote.
    let check = run(&["check", &d.path().to_string_lossy()]);
    assert!(
        check.status.success(),
        "`{what}` left a vault that does not roundtrip:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn rename_changes_the_title() {
    changes_the_file("rename", &["rename", "<FILE>", ID_A, "Renamed"]);
}

#[test]
fn set_todo_changes_the_keyword() {
    changes_the_file("set-todo", &["set-todo", "<FILE>", ID_A, "DONE"]);
}

#[test]
fn set_priority_changes_the_cookie() {
    changes_the_file("set-priority", &["set-priority", "<FILE>", ID_A, "A"]);
}

#[test]
fn set_tags_changes_the_tags() {
    changes_the_file("set-tags", &["set-tags", "<FILE>", ID_A, "home,urgent"]);
}

#[test]
fn set_property_writes_a_drawer_entry() {
    changes_the_file(
        "set-property",
        &["set-property", "<FILE>", ID_A, "CATEGORY", "build"],
    );
}

#[test]
fn set_body_replaces_the_body() {
    changes_the_file("set-body", &["set-body", "<FILE>", ID_A, "a new body"]);
}

#[test]
fn promote_moves_a_child_up_a_level() {
    changes_the_file("promote", &["promote", "<FILE>", ID_B]);
}

#[test]
fn demote_moves_a_headline_down_a_level() {
    changes_the_file("demote", &["demote", "<FILE>", ID_B]);
}

#[test]
fn add_sibling_inserts_a_headline() {
    changes_the_file("add-sibling", &["add-sibling", "<FILE>", ID_A, "A new one"]);
}

#[test]
fn comment_toggles_the_keyword() {
    changes_the_file("comment", &["comment", "<FILE>", ID_A]);
}

#[test]
fn archive_marks_the_subtree() {
    changes_the_file("archive", &["archive", "<FILE>", ID_A]);
}

#[test]
fn toggle_checkbox_flips_a_box() {
    changes_the_file("toggle-checkbox", &["toggle-checkbox", "<FILE>", "0"]);
}

#[test]
fn capture_adds_an_entry_to_the_vault() {
    // Takes a vault rather than a file, and writes wherever capture
    // files things — so the assertion is about the vault roundtripping,
    // which `changes_the_file` also checks.
    let d = vault();
    let out = run(&["capture", &d.path().to_string_lossy(), "Captured thing"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked at"), "capture panicked:\n{err}");
    assert!(out.status.success(), "capture failed: {err}");
    let check = run(&["check", &d.path().to_string_lossy()]);
    assert!(check.status.success(), "capture broke the roundtrip");
}

#[test]
fn removing_a_headline_takes_it_out() {
    changes_the_file("remove", &["remove", "<FILE>", ID_B]);
}

#[test]
fn a_writing_command_given_an_id_that_is_not_there_fails_rather_than_crashing() {
    // The shape that finds a panic: every one of these looks an id up,
    // and an id that is not in the file is the ordinary typo.
    let d = vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");
    for args in [
        vec!["rename", &file, "01NOSUCH000000000000", "x"],
        vec!["set-todo", &file, "01NOSUCH000000000000", "DONE"],
        vec!["promote", &file, "01NOSUCH000000000000"],
        vec!["remove", &file, "01NOSUCH000000000000"],
    ] {
        let out = run(&args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked at"), "{args:?} panicked:\n{err}");
        assert!(!out.status.success(), "{args:?} reported success");
    }
    // …and left the file alone, which is the half a non-zero exit does
    // not promise.
    let after = fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert_eq!(before, after, "a failed write still changed the file");
}
