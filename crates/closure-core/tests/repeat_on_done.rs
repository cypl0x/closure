//! "Parsing a repeater is not the feature; the feature is that
//! finishing the weekly review schedules the next one instead of
//! closing it."
//!
//! `closure-org` can read `+1w` and work out what comes after it. That
//! made repeaters legible and changed nothing about what happens when
//! you press the key: a repeating task marked DONE stayed DONE, and the
//! series ended there.
//!
//! Org's answer is that a repeating task is never done, only done *this
//! time*: the date moves on, the keyword goes back to the first
//! not-done one, and `:LAST_REPEAT:` records when the last one was
//! finished. That is a mutation, so it is a command and I3 makes it
//! undoable — one `undo` puts the old date and the old keyword back
//! together, because they moved together.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command as _, Document, SetTodo};

const REPEATING: &str = "\
* TODO Weekly review
SCHEDULED: <2026-08-11 Tue +1w>
:PROPERTIES:
:ID: 01REPEAT000000000000001A
:END:
the body
";

const ONE_OFF: &str = "\
* TODO Write it up
SCHEDULED: <2026-08-11 Tue>
:PROPERTIES:
:ID: 01REPEAT000000000000002A
:END:
";

fn done(src: &str, id: &str) -> Document {
    let mut doc = Document::load_str(src).expect("parse");
    let cmd = SetTodo::new(BlockId::from_existing(id), Some("DONE".to_owned()));
    cmd.apply(&mut doc).expect("apply");
    doc
}

#[test]
fn finishing_a_repeating_task_schedules_the_next_one() {
    let doc = done(REPEATING, "01REPEAT000000000000001A");
    let src = doc.source();
    assert!(
        src.contains("<2026-08-18 Tue +1w>"),
        "the date did not move: {src}"
    );
    assert!(
        !src.contains("<2026-08-11 Tue +1w>"),
        "the old date is still there: {src}"
    );
}

#[test]
fn it_goes_back_to_todo_rather_than_staying_done() {
    // A repeating task is never done, only done this time.
    let doc = done(REPEATING, "01REPEAT000000000000001A");
    let src = doc.source();
    assert!(src.contains("* TODO Weekly review"), "{src}");
    assert!(!src.contains("* DONE"), "{src}");
}

#[test]
fn it_records_when_the_last_one_was_finished() {
    let doc = done(REPEATING, "01REPEAT000000000000001A");
    let src = doc.source();
    assert!(
        src.contains(":LAST_REPEAT:"),
        "nothing says the last one was ever done: {src}"
    );
}

#[test]
fn a_one_off_task_is_simply_done() {
    // Everything that has no repeater behaves exactly as before.
    let doc = done(ONE_OFF, "01REPEAT000000000000002A");
    let src = doc.source();
    assert!(src.contains("* DONE Write it up"), "{src}");
    assert!(src.contains("<2026-08-11 Tue>"), "the date moved: {src}");
}

#[test]
fn undo_puts_the_date_and_the_keyword_back_together() {
    // I3. They moved as one edit, so they come back as one.
    let mut doc = Document::load_str(REPEATING).expect("parse");
    let before = doc.source();
    let cmd = SetTodo::new(
        BlockId::from_existing("01REPEAT000000000000001A"),
        Some("DONE".to_owned()),
    );
    cmd.apply(&mut doc).expect("apply");
    doc.undo().expect("undo");
    assert_eq!(doc.source(), before, "undo did not restore the whole edit");
}

#[test]
fn clearing_a_keyword_does_not_repeat_anything() {
    // Only finishing repeats; taking the keyword off is not finishing.
    let mut doc = Document::load_str(REPEATING).expect("parse");
    let cmd = SetTodo::new(BlockId::from_existing("01REPEAT000000000000001A"), None);
    cmd.apply(&mut doc).expect("apply");
    assert!(
        doc.source().contains("<2026-08-11 Tue +1w>"),
        "{}",
        doc.source()
    );
}
