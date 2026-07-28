//! A planning line between a heading and its drawer (I2).
//!
//! Org's own grammar puts the planning line first:
//!
//! ```org
//! * TODO Ship it
//! SCHEDULED: <2026-07-29 Wed>
//! :PROPERTIES:
//! :ID: 01H…
//! :END:
//! ```
//!
//! and that is exactly what `rewrite_headline_set_planning` writes,
//! because a planning line has to follow the header line to *be* one.
//! The parser captured the drawer only when it followed the header
//! immediately, so scheduling a note made its `:ID:` unreadable: the
//! next open found no id and minted a fresh one, breaking every
//! `id:` link into it, the stored `last_place`, and CRDT merges that
//! address blocks by id (I2).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::parse;

const WITH_PLANNING: &str = "\
* TODO Ship it
SCHEDULED: <2026-07-29 Wed>
:PROPERTIES:
:ID: 01HQDRAWER0000000000000001
:END:
body text
";

#[test]
fn the_drawer_is_read_through_the_planning_line() {
    let doc = parse(WITH_PLANNING).expect("parse");
    let h = doc.roots().first().expect("one headline");
    assert_eq!(
        h.id_property(),
        Some("01HQDRAWER0000000000000001"),
        "the id survives a planning line above the drawer"
    );
    assert_eq!(
        h.planning().and_then(|p| p.scheduled),
        Some("<2026-07-29 Wed>")
    );
}

#[test]
fn the_planning_line_is_not_eaten_by_the_drawer() {
    let doc = parse(WITH_PLANNING).expect("parse");
    // I1: whatever we classified, the file prints back byte-for-byte —
    // and the *printer* is the half that has to keep the planning line
    // above the drawer, because the drawer is a field and the body is
    // a list.
    assert_eq!(doc.source(), WITH_PLANNING);
    assert_eq!(closure_org::print(&doc), WITH_PLANNING);
}

#[test]
fn a_deadline_and_a_schedule_on_one_line_still_leave_the_drawer() {
    let src = "* TODO Both\nSCHEDULED: <2026-07-29 Wed> DEADLINE: <2026-08-01 Sat>\n\
               :PROPERTIES:\n:ID: 01HQDRAWER0000000000000002\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    let h = doc.roots().first().expect("one headline");
    assert_eq!(h.id_property(), Some("01HQDRAWER0000000000000002"));
    let planning = h.planning().expect("planning");
    assert_eq!(planning.scheduled, Some("<2026-07-29 Wed>"));
    assert_eq!(planning.deadline, Some("<2026-08-01 Sat>"));
    assert_eq!(doc.source(), src);
    assert_eq!(closure_org::print(&doc), src);
}

#[test]
fn a_prose_line_between_planning_and_drawer_is_not_a_drawer_position() {
    // Org is strict here, and so are we: the drawer has to follow the
    // planning line. Text in between makes the `:PROPERTIES:` block
    // ordinary body text, and it must not be read as an id.
    let src = "* TODO Loose\nSCHEDULED: <2026-07-29 Wed>\nsome prose\n\
               :PROPERTIES:\n:ID: 01HQDRAWER0000000000000003\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    let h = doc.roots().first().expect("one headline");
    assert_eq!(h.id_property(), None);
    assert_eq!(doc.source(), src);
    assert_eq!(closure_org::print(&doc), src);
}

#[test]
fn the_drawer_directly_under_the_header_still_works() {
    let src = "* TODO Plain\n:PROPERTIES:\n:ID: 01HQDRAWER0000000000000004\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    let h = doc.roots().first().expect("one headline");
    assert_eq!(h.id_property(), Some("01HQDRAWER0000000000000004"));
    assert_eq!(doc.source(), src);
    assert_eq!(closure_org::print(&doc), src);
}
