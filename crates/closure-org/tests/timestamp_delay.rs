//! `DEADLINE: <2026-08-20 Thu -2d>` — how long before it starts nagging.
//!
//! Org warns about a deadline for some days before it lands
//! (`org-deadline-warning-days`, 14 by default), and a `-2d` cooldown
//! on the timestamp narrows that for one task: "this one, only tell me
//! two days out".
//!
//! closure read neither. A deadline surfaced when its date arrived and
//! not a day sooner, which is the one moment a warning is no longer
//! useful — so the cooldown was not merely ignored, it had nothing to
//! narrow.
//!
//! The delay is deliberately not a repeater, even though both are a
//! sign and a unit in the same brackets. `+1w` says when the task comes
//! back; `-2d` says when to mention it. Sharing a parser would let one
//! acquire the other's rules, and `<2026-08-20 Thu +1w -2d>` is a
//! weekly task with a two-day warning — both, not either.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Unit, delay_of, parse_repeater};

#[test]
fn a_delay_is_read() {
    let d = delay_of("<2026-08-20 Thu -2d>").expect("a delay");
    assert_eq!(d.count, 2);
    assert_eq!(d.unit, Unit::Day);
}

#[test]
fn every_unit_org_writes() {
    for (src, count, unit) in [
        ("<2026-08-20 Thu -3d>", 3, Unit::Day),
        ("<2026-08-20 Thu -2w>", 2, Unit::Week),
        ("<2026-08-20 Thu -1m>", 1, Unit::Month),
        ("<2026-08-20 Thu -1y>", 1, Unit::Year),
    ] {
        let d = delay_of(src).unwrap_or_else(|| panic!("no delay in {src}"));
        assert_eq!((d.count, d.unit), (count, unit), "{src}");
    }
}

#[test]
fn a_timestamp_with_no_delay_has_none() {
    assert!(delay_of("<2026-08-20 Thu>").is_none());
    assert!(delay_of("<2026-08-20 Thu 10:00-11:00>").is_none());
}

#[test]
fn a_repeater_is_not_a_delay() {
    // The case that decides whether the two parsers can be one. They
    // cannot: `+1w` is a sign and a unit in the same brackets and means
    // something else entirely.
    assert!(delay_of("<2026-08-20 Thu +1w>").is_none());
    assert!(delay_of("<2026-08-20 Thu ++1m>").is_none());
    assert!(delay_of("<2026-08-20 Thu .+1d>").is_none());
}

#[test]
fn a_task_can_repeat_and_still_have_a_warning() {
    // Both, not either — a weekly task that says "two days out".
    let src = "<2026-08-20 Thu +1w -2d>";
    let r = parse_repeater(src).expect("a repeater");
    assert_eq!(r.count, 1);
    let d = delay_of(src).expect("a delay");
    assert_eq!(d.count, 2);
}

#[test]
fn a_negative_time_is_not_a_delay() {
    // `10:00-11:00` is a range and the `-11` in it is not a cooldown.
    // Getting this wrong would give every timed entry an eleven-unit
    // warning period out of nowhere.
    assert!(delay_of("<2026-08-20 Thu 10:00-11:00>").is_none());
}

#[test]
fn a_malformed_delay_is_not_one() {
    // I5: no panic, no guess.
    assert!(delay_of("<2026-08-20 Thu -d>").is_none());
    assert!(delay_of("<2026-08-20 Thu -2q>").is_none());
    assert!(delay_of("<2026-08-20 Thu ->").is_none());
}
