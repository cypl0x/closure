//! Where one file's headlines end and the next file's begin.
//!
//! Reported 2026-08-02: "because it's quite hard to see where a file
//! ends or starts, due to the flat hierachy. I am thinking of two ways
//! … 1. (colored) dividers at the beginning and the end of the headline
//! tree of a single file 2. colorize the headline items of each files
//! differently (that could be too colorful)."
//!
//! The second is the one they doubted themselves, and rightly: colour
//! already carries outline depth, and a second meaning on the same
//! channel would make neither readable. So: a divider, and the only
//! question left is where it goes. A row that *starts* a file is the
//! one place a rule can sit without needing a row of its own — which
//! matters because the outline is a uniform list whose indices are the
//! selection.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell, starts_file};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("a.org"), "* A one\n** A kid\n* A two\n").expect("write");
    fs::write(dir.path().join("b.org"), "* B one\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_very_first_row_starts_a_file() {
    let (_d, sh, app) = fixture();
    let rows = app.rows(&sh);
    assert!(starts_file(&rows, 0));
}

#[test]
fn a_row_whose_file_differs_from_the_one_above_starts_a_file() {
    let (_d, sh, app) = fixture();
    let rows = app.rows(&sh);
    let first_b = rows
        .iter()
        .position(|r| r.title == "B one")
        .expect("b.org is in the outline");
    assert!(starts_file(&rows, first_b), "the divider goes here");
}

#[test]
fn the_rest_of_a_files_rows_do_not() {
    let (_d, sh, app) = fixture();
    let rows = app.rows(&sh);
    for (i, r) in rows.iter().enumerate() {
        if r.title == "A one" || r.title == "B one" {
            continue;
        }
        assert!(!starts_file(&rows, i), "{} is mid-file", r.title);
    }
}

#[test]
fn exactly_one_row_per_file_starts_it() {
    let (_d, sh, app) = fixture();
    let rows = app.rows(&sh);
    let starts = (0..rows.len()).filter(|&i| starts_file(&rows, i)).count();
    assert_eq!(starts, 2, "two files, two dividers");
}

#[test]
fn an_index_past_the_end_starts_nothing() {
    // The renderer walks a visible window, not the whole list.
    let (_d, sh, app) = fixture();
    let rows = app.rows(&sh);
    assert!(!starts_file(&rows, rows.len()));
    assert!(
        !starts_file(&[], 0),
        "and an empty vault has no rows at all"
    );
}
