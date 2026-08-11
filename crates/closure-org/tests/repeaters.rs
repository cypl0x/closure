//! "`+1w`, `.+1d`, `++1m` on SCHEDULED/DEADLINE. A task tool without
//! repeaters is not a task tool."
//!
//! A timestamp was a verbatim string: closure could tell you a headline
//! was scheduled and hand you back the text between the brackets.
//! `<2026-08-11 Tue +1w>` therefore meant exactly what
//! `<2026-08-11 Tue>` meant, which is that the weekly review, the rent,
//! and every habit anyone keeps in org were one-off tasks that fell
//! silent the day they were done.
//!
//! Org has three, and they differ only in what "next" means when you
//! are late — which is the whole reason there are three:
//!
//! - `+1w` counts from the date written, so a task done three weeks
//!   late still lands on the next date in the original series.
//! - `++1w` counts from the date written but keeps going until it is
//!   in the future, so it skips the ones you missed.
//! - `.+1w` counts from *today*, so a task done three weeks late is
//!   next due a week from now.
//!
//! Parsing them is this file. Advancing a task's date when it is marked
//! done is a command, and is its own item.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{RepeatKind, Repeater, Unit, advance, parse_repeater};

#[test]
fn a_plain_repeater_is_read() {
    let r = parse_repeater("<2026-08-11 Tue +1w>").expect("a repeater");
    assert_eq!(
        r,
        Repeater {
            kind: RepeatKind::FromWritten,
            count: 1,
            unit: Unit::Week
        }
    );
}

#[test]
fn the_three_kinds_are_told_apart() {
    let kind = |ts: &str| parse_repeater(ts).map(|r| r.kind);
    assert_eq!(
        kind("<2026-08-11 Tue +2d>"),
        Some(RepeatKind::FromWritten),
        "+"
    );
    assert_eq!(
        kind("<2026-08-11 Tue ++2d>"),
        Some(RepeatKind::CatchUp),
        "++"
    );
    assert_eq!(
        kind("<2026-08-11 Tue .+2d>"),
        Some(RepeatKind::FromToday),
        ".+"
    );
}

#[test]
fn every_unit_org_has() {
    let unit = |ts: &str| parse_repeater(ts).map(|r| r.unit);
    assert_eq!(unit("<2026-08-11 Tue +3h>"), Some(Unit::Hour));
    assert_eq!(unit("<2026-08-11 Tue +3d>"), Some(Unit::Day));
    assert_eq!(unit("<2026-08-11 Tue +3w>"), Some(Unit::Week));
    assert_eq!(unit("<2026-08-11 Tue +3m>"), Some(Unit::Month));
    assert_eq!(unit("<2026-08-11 Tue +3y>"), Some(Unit::Year));
}

#[test]
fn a_timestamp_without_one_has_none() {
    assert_eq!(parse_repeater("<2026-08-11 Tue>"), None);
    assert_eq!(parse_repeater("[2026-08-11 Tue]"), None);
    assert_eq!(parse_repeater("not a timestamp"), None);
}

#[test]
fn a_deadline_or_a_delay_is_not_a_repeater() {
    // `-2d` is a warning period on a DEADLINE, not a repeat.
    assert_eq!(parse_repeater("<2026-08-11 Tue -2d>"), None);
    // Both together: the repeater is still found.
    assert_eq!(
        parse_repeater("<2026-08-11 Tue +1w -2d>").map(|r| r.unit),
        Some(Unit::Week)
    );
}

#[test]
fn advancing_by_a_week_moves_the_date_and_keeps_the_rest() {
    let next = advance("<2026-08-11 Tue +1w>", "2026-08-11").expect("advanced");
    assert_eq!(next, "<2026-08-18 Tue +1w>");
}

#[test]
fn advancing_recomputes_the_day_name() {
    // The day name is derived, so a date that moves must not keep the
    // name it had — an org file that says Tue on a Wednesday is one
    // Emacs will quietly disagree with.
    let next = advance("<2026-08-11 Tue +3d>", "2026-08-11").expect("advanced");
    assert_eq!(next, "<2026-08-14 Fri +3d>");
}

#[test]
fn a_plain_repeat_lands_on_the_next_in_the_series_even_when_late() {
    // `+1w` from a date three weeks ago is one week after that date,
    // late or not: the series is the point.
    let next = advance("<2026-07-21 Tue +1w>", "2026-08-11").expect("advanced");
    assert_eq!(next, "<2026-07-28 Tue +1w>");
}

#[test]
fn a_catch_up_repeat_skips_what_was_missed() {
    // `++1w` keeps adding until it is in the future.
    let next = advance("<2026-07-21 Tue ++1w>", "2026-08-11").expect("advanced");
    assert_eq!(next, "<2026-08-18 Tue ++1w>");
}

#[test]
fn a_from_today_repeat_counts_from_today() {
    let next = advance("<2026-07-21 Tue .+1w>", "2026-08-11").expect("advanced");
    assert_eq!(next, "<2026-08-18 Tue .+1w>");
}

#[test]
fn months_and_years_roll_over() {
    assert_eq!(
        advance("<2026-12-15 Tue +1m>", "2026-12-15").as_deref(),
        Some("<2027-01-15 Fri +1m>")
    );
    assert_eq!(
        advance("<2026-02-29 Sun +1y>", "2026-02-29").as_deref(),
        Some("<2027-02-28 Sun +1y>"),
        "a day that does not exist next year clamps to the last of the month"
    );
}

#[test]
fn a_timestamp_with_no_repeater_does_not_advance() {
    assert_eq!(advance("<2026-08-11 Tue>", "2026-08-11"), None);
}
