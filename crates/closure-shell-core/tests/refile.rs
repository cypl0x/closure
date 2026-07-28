//! Q3-V1/V2 in the shell: a refile target picker, and archiving.
//!
//! Filing a capture where it belongs is the move that makes an inbox an
//! inbox. The store does the move; this is the half that has to ask
//! *where* — a fuzzy picker over every headline in the vault, showing
//! the file and the path so two notes with the same title are
//! distinguishable.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const INBOX: &str = "\
* TODO Buy milk
:PROPERTIES:
:ID: 01HQSREFILE000000000000001
:END:
";

const PROJECT: &str = "\
* Errands
:PROPERTIES:
:ID: 01HQSREFILE000000000000010
:END:
* Work
:PROPERTIES:
:ID: 01HQSREFILE000000000000011
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("inbox.org"), INBOX).expect("write");
    fs::write(dir.path().join("project.org"), PROJECT).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_today("2026-07-28");
    (dir, Shell::new(vault), app)
}

#[test]
fn v1_refile_opens_a_picker_over_every_other_headline() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSREFILE000000000000001"));
    app.run(&mut shell, "refile");

    assert_eq!(app.surface(), ModalSurface::Refile);
    let rows = app.refile_rows(&shell);
    assert!(
        rows.iter().any(|r| r.title == "Errands"),
        "targets: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.title == "Buy milk"),
        "not itself: {rows:?}"
    );
    assert!(
        rows.iter().all(|r| !r.path.is_empty()),
        "each says which file it is in: {rows:?}"
    );
}

#[test]
fn v1_typing_filters_the_targets_and_enter_files_it() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSREFILE000000000000001"));
    app.run(&mut shell, "refile");
    for c in "err".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(
        app.refile_rows(&shell)
            .iter()
            .filter(|r| r.matches_filter)
            .count(),
        1,
        "only Errands matches"
    );
    app.on_key(&mut shell, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Browse);
    let project = fs::read_to_string(dir.path().join("project.org")).expect("read");
    assert!(project.contains("** TODO Buy milk"), "{project}");
    let inbox = fs::read_to_string(dir.path().join("inbox.org")).expect("read");
    assert!(!inbox.contains("Buy milk"), "{inbox}");
}

#[test]
fn v1_escape_files_nothing() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSREFILE000000000000001"));
    app.run(&mut shell, "refile");
    app.on_key(&mut shell, "escape", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Browse);
    let inbox = fs::read_to_string(dir.path().join("inbox.org")).expect("read");
    assert!(inbox.contains("Buy milk"), "still where it was: {inbox}");
}

#[test]
fn v1_with_nothing_selected_there_is_nothing_to_file() {
    let (_d, mut shell, mut app) = fixture();
    app.clear_selection();
    app.run(&mut shell, "refile");
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!app.status().is_empty(), "it says why");
}

#[test]
fn v2_archive_moves_the_subtree_and_says_where_it_went() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSREFILE000000000000001"));
    app.run(&mut shell, "archive");

    let inbox = fs::read_to_string(dir.path().join("inbox.org")).expect("read");
    assert!(!inbox.contains("Buy milk"), "{inbox}");
    let archived = fs::read_to_string(dir.path().join("inbox.org_archive")).expect("read");
    assert!(archived.contains("Buy milk"), "{archived}");
    assert!(
        archived.contains(":ARCHIVE_TIME: 2026-07-28"),
        "stamped with the day the shell was told about: {archived}"
    );
    assert!(app.status().contains("archive"), "status: {}", app.status());
}

#[test]
fn v2_archiving_with_nothing_selected_is_a_no_op() {
    let (dir, mut shell, mut app) = fixture();
    app.clear_selection();
    app.run(&mut shell, "archive");
    let inbox = fs::read_to_string(dir.path().join("inbox.org")).expect("read");
    assert!(inbox.contains("Buy milk"), "{inbox}");
}
