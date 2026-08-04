//! "The panel currently just show the date in ISO8601 format. Please
//! add the time to both."
//!
//! Both being `created` (which comes out of the ULID, and so is exact
//! to the millisecond) and `saved` (the file's mtime). A date alone
//! answers "which day" and nothing else — which for a vault you work
//! in all day is the question you never have.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{Shell, ulid_date};
use closure_store::Vault;

#[test]
fn a_ulid_carries_its_time_of_day() {
    // 01ARZ3NDEKTSV4RRFFQ69G5FAV is the ULID spec's own example. Its
    // ten timestamp characters decode to 1469922850259 ms.
    let stamp = ulid_date("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("a ULID has a timestamp");
    assert_eq!(stamp, "2016-07-30 23:54:10");
}

#[test]
fn midnight_is_not_left_off() {
    // A timestamp landing exactly on 00:00:00 must still print it, or
    // the one row a day where the field is shortest reads as a bug.
    let stamp = ulid_date("01ARWHKC000000000000000000").expect("a ULID has a timestamp");
    assert!(
        stamp.ends_with(" 00:00:00"),
        "midnight lost its time: {stamp}"
    );
}

#[test]
fn something_that_is_not_a_ulid_still_has_no_date() {
    assert_eq!(ulid_date("not-a-ulid"), None);
    assert_eq!(ulid_date(""), None);
}

#[test]
fn the_panel_shows_the_time_the_file_was_saved() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n:END:\nbody\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = closure_shell_core::ModalApp::new(closure_config::InputMode::Doom);
    app.select(0, &shell);
    let detail = app.detail(&shell).expect("a row is selected");

    let created = detail.created.expect("created");
    assert_eq!(created, "2016-07-30 23:54:10");
    let modified = detail.modified.expect("modified");
    // The mtime is now, whenever now is — so this asserts the *shape*,
    // which is the thing that was missing.
    assert_eq!(
        modified.len(),
        "2026-08-04 12:34:56".len(),
        "no time of day in the saved field: {modified}"
    );
    assert!(modified.contains(':'), "{modified}");
}
