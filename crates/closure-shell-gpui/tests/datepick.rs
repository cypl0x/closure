//! Q3-V4 in the window: the calendar paints, and a click picks a day.
//!
//! The month arithmetic is the core's (`tests/planning.rs` in
//! closure-shell-core); this is the half only the window can be wrong
//! about — that the grid renders, and that a pointer lands on the day
//! under it.
//!
//! Run with `cargo test -p closure-shell-gpui --features gpui-test`.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{ModalSurface, month_name, test_window};

const VAULT: &str = "\
* TODO Ship it
:PROPERTIES:
:ID: 01HQGDATE00000000000000001
:END:
body
";

#[gpui::test]
fn the_calendar_opens_and_paints(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.set_today("2026-07-28");
            view.run_command("schedule", cx);
            assert_eq!(view.surface(), ModalSurface::DatePick);
            assert_eq!(view.picked_date(), "2026-07-28");
        })
        .expect("live");
    // The pane has to survive a real layout pass, not merely exist.
    cx.run_until_parked();
}

#[gpui::test]
fn clicking_a_day_moves_the_cursor_to_it(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.set_today("2026-07-28");
            view.run_command("schedule", cx);
            view.date_click(3, cx);
            assert_eq!(view.picked_date(), "2026-07-03");
        })
        .expect("live");
    cx.run_until_parked();
}

#[gpui::test]
fn the_window_writes_the_stamp_the_calendar_shows(cx: &mut gpui::TestAppContext) {
    let (dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.set_today("2026-07-28");
            view.run_command("schedule", cx);
            view.date_click(30, cx);
            view.press("enter", false, false, cx);
            assert_eq!(view.surface(), ModalSurface::Browse);
        })
        .expect("live");
    let src = std::fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("SCHEDULED: <2026-07-30 Thu>"), "{src}");
    // …and the id it was scheduled against is still the id (I2).
    assert!(src.contains(":ID: 01HQGDATE00000000000000001"), "{src}");
}

#[test]
fn every_month_has_a_name() {
    assert_eq!(month_name(1), "January");
    assert_eq!(month_name(12), "December");
    assert_eq!(month_name(13), "", "a month that is not one names nothing");
}
