//! `<2026-08-12 Wed 10:00-11:00>` is a span, not a date.
//!
//! The most common thing on a calendar, and closure read it as a
//! string: a one-hour meeting and an all-day "sometime Wednesday" were
//! the same value to everything downstream, so nothing could sort by
//! when a thing starts or say how long it runs.
//!
//! Deliberately a reading rather than a new type. `stamp_minutes` and
//! `stamp_days` already existed for `CLOCK:` lines and knew nothing
//! about timestamps; this connects them. A parallel notion of "a time"
//! would be the second set of rules about midnight and about what a
//! malformed stamp means, and the two would disagree eventually.
//!
//! Everything here is a view (I1): the source keeps its own bytes, and
//! `span_of` never rewrites a stamp into a normal form.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{find_timestamps, span_of};

#[test]
fn a_range_gives_a_start_and_an_end() {
    let ts = find_timestamps("meet <2026-08-12 Wed 10:00-11:00> about it");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.start_minutes, Some(600));
    assert_eq!(span.end_minutes, Some(660));
    assert_eq!(span.minutes(), Some(60));
}

#[test]
fn a_plain_time_starts_and_does_not_end() {
    // `<2026-08-12 Wed 10:00>` says when it begins and nothing about
    // how long. Reporting a zero-length span would be an answer where
    // there is none.
    let ts = find_timestamps("<2026-08-12 Wed 10:00>");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.start_minutes, Some(600));
    assert_eq!(span.end_minutes, None);
    assert_eq!(span.minutes(), None);
}

#[test]
fn a_date_with_no_time_has_no_minutes_at_all() {
    let ts = find_timestamps("<2026-08-12 Wed>");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.start_minutes, None);
    assert_eq!(span.end_minutes, None);
    assert_eq!(span.minutes(), None);
}

#[test]
fn the_date_comes_back_too() {
    let ts = find_timestamps("<2026-08-12 Wed 10:00-11:00>");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.date, "2026-08-12");
}

#[test]
fn a_repeater_after_the_range_does_not_confuse_it() {
    // `<2026-08-12 Wed 10:00-11:00 +1w>` is a weekly meeting. The
    // repeater is a different question and must not be read as part of
    // the end time.
    let ts = find_timestamps("<2026-08-12 Wed 10:00-11:00 +1w>");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.start_minutes, Some(600));
    assert_eq!(span.end_minutes, Some(660));
}

#[test]
fn an_inactive_stamp_reads_the_same_way() {
    // The brackets say whether it reaches the agenda, not what the
    // time means.
    let ts = find_timestamps("[2026-08-12 Wed 09:15-09:45]");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.minutes(), Some(30));
}

#[test]
fn a_range_that_ends_before_it_starts_reports_no_length() {
    // Rather than a negative one, or a wrap to the next day: org does
    // not say what `11:00-10:00` means, and inventing an answer here
    // would put a made-up number into an agenda.
    let ts = find_timestamps("<2026-08-12 Wed 11:00-10:00>");
    let span = span_of(ts[0].content).expect("a span");
    assert_eq!(span.minutes(), None);
}

#[test]
fn a_malformed_time_is_not_a_time() {
    // I5: a bad stamp yields no reading, never a panic.
    for content in ["2026-08-12 Wed 25:00", "2026-08-12 Wed ab:cd"] {
        let span = span_of(content).expect("still a dated stamp");
        assert_eq!(span.start_minutes, None, "{content}");
    }
}

#[test]
fn something_that_is_not_a_timestamp_is_not_one() {
    assert!(span_of("not a stamp at all").is_none());
}

#[test]
fn the_source_is_never_rewritten() {
    // I1. Reading a span must not normalise `10:00-11:00` into
    // anything, in the stamp or anywhere else.
    let src = "meet <2026-08-12 Wed 10:00-11:00 +1w> about it";
    let ts = find_timestamps(src);
    let _ = span_of(ts[0].content);
    assert_eq!(ts[0].content, "2026-08-12 Wed 10:00-11:00 +1w");
}
