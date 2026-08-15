//! The rewriters, tested against the sentences in their own docs.
//!
//! Each of these carries a comment recording something somebody worked
//! out once — that an existing id is never replaced, that the body a
//! rewriter replaces must be the same span the reader captures, that
//! date arithmetic is exactly what a hand-rolled version gets wrong.
//! Those comments are the best index of where the bugs are, and after
//! `rewrite_headline_remove_property` ate a body, they are worth
//! reading as a list of tests nobody had written.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{
    headline_body_text, parse, rewrite_add_sibling_after, rewrite_headline_ensure_id,
    rewrite_headline_set_priority, shift_date,
};

const ID: &str = "01REWRITE00000000001";
const OTHER: &str = "01REWRITE00000000002";

// === priority ===

#[test]
fn a_priority_goes_after_the_todo_keyword_and_before_the_title() {
    let d = parse("* TODO Ship it :work:\n").expect("parses");
    let out = rewrite_headline_set_priority(&d, &[0], Some('A')).expect("set");
    assert_eq!(
        out.source(),
        "* TODO [#A] Ship it :work:\n",
        "{}",
        out.source()
    );
}

#[test]
fn a_priority_on_a_headline_with_no_keyword_goes_before_the_title() {
    let d = parse("* Ship it\n").expect("parses");
    let out = rewrite_headline_set_priority(&d, &[0], Some('A')).expect("set");
    assert_eq!(out.source(), "* [#A] Ship it\n", "{}", out.source());
}

#[test]
fn setting_a_priority_replaces_the_one_that_is_there() {
    // Not two cookies. `* TODO [#A] [#B] Ship it` is not a headline
    // anybody meant, and org would read the second as part of the title.
    let d = parse("* TODO [#B] Ship it\n").expect("parses");
    let out = rewrite_headline_set_priority(&d, &[0], Some('A')).expect("set");
    assert_eq!(out.source(), "* TODO [#A] Ship it\n", "{}", out.source());
}

#[test]
fn clearing_a_priority_leaves_the_title_and_tags_alone() {
    // "Title and tags are preserved verbatim" — including the single
    // space that separates them, which is where an off-by-one lands.
    let d = parse("* TODO [#A] Ship it :work:urgent:\n").expect("parses");
    let out = rewrite_headline_set_priority(&d, &[0], None).expect("clear");
    assert_eq!(
        out.source(),
        "* TODO Ship it :work:urgent:\n",
        "{}",
        out.source()
    );
}

#[test]
fn clearing_a_priority_that_is_not_there_changes_nothing() {
    let d = parse("* TODO Ship it\n").expect("parses");
    let out = rewrite_headline_set_priority(&d, &[0], None).expect("no-op");
    assert_eq!(out.source(), "* TODO Ship it\n");
}

#[test]
fn a_priority_on_a_path_that_is_not_there_is_refused() {
    let d = parse("* One\n").expect("parses");
    assert!(rewrite_headline_set_priority(&d, &[9], Some('A')).is_err());
}

// === ensure id ===

#[test]
fn an_id_is_given_a_fresh_drawer_when_there_is_none() {
    let d = parse("* Bare\nbody\n").expect("parses");
    let out = rewrite_headline_ensure_id(&d, &[0], ID).expect("ensure");
    let src = out.source();
    assert!(src.contains(":PROPERTIES:"), "{src}");
    assert!(src.contains(&format!(":ID: {ID}")), "{src}");
    assert!(src.contains("body"), "the body was displaced: {src}");
    // The drawer belongs immediately under the header line, not after
    // the body — org only reads it there.
    let lines: Vec<&str> = src.lines().collect();
    assert_eq!(lines[0], "* Bare");
    assert_eq!(lines[1], ":PROPERTIES:", "{src}");
}

#[test]
fn an_id_joins_a_drawer_that_already_exists() {
    let d = parse("* One\n:PROPERTIES:\n:CATEGORY: build\n:END:\n").expect("parses");
    let out = rewrite_headline_ensure_id(&d, &[0], ID).expect("ensure");
    let src = out.source();
    assert!(src.contains(&format!(":ID: {ID}")), "{src}");
    assert!(
        src.contains(":CATEGORY: build"),
        "the other entry went: {src}"
    );
    assert_eq!(
        src.matches(":PROPERTIES:").count(),
        1,
        "a second drawer: {src}"
    );
}

#[test]
fn an_id_that_is_already_there_is_never_replaced() {
    // I2, stated in the doc. Reissuing an id breaks every `id:` link
    // pointing at this headline, silently and permanently.
    let d = parse(&format!("* One\n:PROPERTIES:\n:ID: {ID}\n:END:\n")).expect("parses");
    let out = rewrite_headline_ensure_id(&d, &[0], OTHER).expect("no-op");
    let src = out.source();
    assert!(src.contains(ID), "the existing id was replaced: {src}");
    assert!(!src.contains(OTHER), "a second id was written: {src}");
}

// === add sibling ===

#[test]
fn a_sibling_lands_after_the_whole_subtree_at_the_same_level() {
    // "Immediately after the subtree rooted at path" — after the
    // children, not between the headline and its first child, which
    // would adopt them.
    let d = parse("* First\n** A child\n*** A grandchild\n* Last\n").expect("parses");
    let out = rewrite_add_sibling_after(&d, &[0], "Inserted").expect("insert");
    let lines: Vec<&str> = out.source().lines().collect();
    assert_eq!(
        lines,
        vec![
            "* First",
            "** A child",
            "*** A grandchild",
            "* Inserted",
            "* Last"
        ],
        "{lines:?}"
    );
}

#[test]
fn a_sibling_of_a_child_is_a_child() {
    let d = parse("* Root\n** First child\n** Second child\n").expect("parses");
    let out = rewrite_add_sibling_after(&d, &[0, 0], "Inserted").expect("insert");
    let lines: Vec<&str> = out.source().lines().collect();
    assert_eq!(
        lines,
        vec!["* Root", "** First child", "** Inserted", "** Second child"],
        "{lines:?}"
    );
}

#[test]
fn a_sibling_after_the_last_headline_goes_at_the_end() {
    let d = parse("* Only\nbody\n").expect("parses");
    let out = rewrite_add_sibling_after(&d, &[0], "Inserted").expect("insert");
    assert!(out.source().ends_with("* Inserted\n"), "{:?}", out.source());
    assert!(out.source().contains("body"), "{}", out.source());
}

// === the body span ===

#[test]
fn the_body_text_excludes_the_planning_line_and_the_drawer() {
    // The bug this function exists to prevent, written in its doc: the
    // reader captured the SCHEDULED: line and the rewriter did not
    // replace it, so setting a body and undoing wrote the planning line
    // back a second time.
    let src = "* One\n\
               SCHEDULED: <2026-06-20 Sat>\n\
               :PROPERTIES:\n:ID: 01REWRITE00000000001\n:END:\n\
               the real body\n";
    let d = parse(src).expect("parses");
    let body = headline_body_text(&d, &[0]).expect("a body");
    assert!(body.contains("the real body"), "{body:?}");
    assert!(
        !body.contains("SCHEDULED:"),
        "the planning line is in the body: {body:?}"
    );
    assert!(
        !body.contains(":PROPERTIES:"),
        "the drawer is in the body: {body:?}"
    );
    assert!(!body.contains(":ID:"), "{body:?}");
}

#[test]
fn the_body_of_a_headline_with_none_is_empty_rather_than_absent() {
    let d = parse("* One\n** A child\n").expect("parses");
    let body = headline_body_text(&d, &[0]).expect("a body");
    assert!(body.trim().is_empty(), "{body:?}");
}

#[test]
fn the_body_stops_before_the_first_child() {
    let d = parse("* One\nmy body\n** A child\nthe child's body\n").expect("parses");
    let body = headline_body_text(&d, &[0]).expect("a body");
    assert!(body.contains("my body"), "{body:?}");
    assert!(
        !body.contains("A child"),
        "the child is in the body: {body:?}"
    );
    assert!(!body.contains("the child's body"), "{body:?}");
}

#[test]
fn the_body_of_a_path_that_is_not_there_is_none() {
    let d = parse("* One\n").expect("parses");
    assert!(headline_body_text(&d, &[9]).is_none());
}

// === date arithmetic ===

#[test]
fn a_date_moves_forward_and_back() {
    assert_eq!(shift_date("2026-06-20", 1).as_deref(), Some("2026-06-21"));
    assert_eq!(shift_date("2026-06-20", -1).as_deref(), Some("2026-06-19"));
    assert_eq!(shift_date("2026-06-20", 0).as_deref(), Some("2026-06-20"));
}

#[test]
fn two_weeks_before_the_first_of_march_is_the_case_the_doc_names() {
    // Its own comment: "two weeks before the first of March is exactly
    // the arithmetic a hand-rolled version gets wrong". 2026 is not a
    // leap year, so this lands in February.
    assert_eq!(shift_date("2026-03-01", -14).as_deref(), Some("2026-02-15"));
    // And a leap year, where the same sum lands a day earlier.
    assert_eq!(shift_date("2024-03-01", -14).as_deref(), Some("2024-02-16"));
}

#[test]
fn a_shift_crosses_a_month_and_a_year_boundary() {
    assert_eq!(shift_date("2026-01-31", 1).as_deref(), Some("2026-02-01"));
    assert_eq!(shift_date("2026-12-31", 1).as_deref(), Some("2027-01-01"));
    assert_eq!(shift_date("2026-01-01", -1).as_deref(), Some("2025-12-31"));
}

#[test]
fn the_twenty_ninth_of_february_exists_only_in_a_leap_year() {
    assert_eq!(shift_date("2024-02-28", 1).as_deref(), Some("2024-02-29"));
    assert_eq!(shift_date("2026-02-28", 1).as_deref(), Some("2026-03-01"));
}

#[test]
fn a_large_shift_is_still_exact() {
    // A year of days, so an accumulated rounding error would show.
    assert_eq!(shift_date("2026-01-01", 365).as_deref(), Some("2027-01-01"));
    assert_eq!(shift_date("2024-01-01", 366).as_deref(), Some("2025-01-01"));
}

#[test]
fn something_that_is_not_a_date_is_not_shifted() {
    for bad in [
        "",
        "not a date",
        "2026-06",
        "2026",
        "2026-06-20-01",
        "x-06-20",
        "2026-xx-20",
    ] {
        assert_eq!(shift_date(bad, 1), None, "`{bad}` was shifted");
    }
}
