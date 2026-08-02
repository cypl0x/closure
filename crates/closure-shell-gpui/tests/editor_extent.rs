//! "editor buffer doesn't render the scrollable text to the correct
//! extent", with a screenshot: a 269-line buffer whose editor stops
//! painting a third of the way up the pane, leaving the bottom of the
//! window empty while there is plenty of text left to show.
//!
//! The viewport is a window of *logical line numbers* —
//! `scroll_start .. scroll_start + view` — and every line in it that
//! happens to be folded is skipped without being replaced. A folded
//! property drawer is four lines, so a note with a dozen children
//! spends fifty slots of that window painting nothing at all, and the
//! pane runs out of budget long before it runs out of room.
//!
//! `view` counts *rows the pane can show*. The window has to be filled
//! with that many painted rows, not that many indices.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;

use closure_shell_gpui::visual_window;

/// A note whose children each carry a property drawer — folded when
/// the buffer opens, which is what opens the holes.
fn drawered(children: usize) -> String {
    let mut org = String::from("* Top\n:PROPERTIES:\n:ID: 01HQEXTN00000000000000000\n:END:\n");
    for i in 0..children {
        let _ = writeln!(
            org,
            "** Child {i}\n:PROPERTIES:\n:ID: 01HQEXTN{i:018}\n:END:\nbody of child {i}"
        );
    }
    org
}

/// A note of the same length with nothing folded in it.
fn plain(lines: usize) -> String {
    let mut org = String::from("* Top\n:PROPERTIES:\n:ID: 01HQEXTN00000000000000000\n:END:\n");
    for i in 0..lines {
        let _ = writeln!(org, "line {i}");
    }
    org
}

#[gpui::test]
fn a_buffer_full_of_folded_drawers_still_fills_the_pane(cx: &mut gpui::TestAppContext) {
    // The report. Every folded line used to cost a slot of the
    // viewport and paint nothing, so the more drawers a note had the
    // emptier the bottom of the window became.
    let (_dir, view, vcx) = visual_window(cx, &drawered(60));
    view.update(vcx, |v, cx| v.run_command("toggle-file-view", cx));
    vcx.run_until_parked();
    let rows = view.update(vcx, |v, _cx| v.body_view());
    let drawn = view.update(vcx, |v, _cx| v.painted_rows());
    assert!(
        drawn >= rows,
        "pane holds {rows} rows and only {drawn} were painted"
    );
}

#[gpui::test]
fn a_buffer_with_nothing_folded_is_unchanged(cx: &mut gpui::TestAppContext) {
    // The case that always worked, kept as cover: filling the window
    // must not start painting *more* than the pane can show.
    let (_dir, view, vcx) = visual_window(cx, &plain(400));
    view.update(vcx, |v, cx| v.run_command("toggle-file-view", cx));
    vcx.run_until_parked();
    let rows = view.update(vcx, |v, _cx| v.body_view());
    let drawn = view.update(vcx, |v, _cx| v.painted_rows());
    assert!(
        drawn >= rows,
        "pane holds {rows} rows and only {drawn} were painted"
    );
    assert!(
        drawn <= rows + 2,
        "painted {drawn} rows into a pane that holds {rows}"
    );
}

#[gpui::test]
fn a_short_buffer_paints_what_it_has_and_no_more(cx: &mut gpui::TestAppContext) {
    // Filling the window must not invent lines past the end of the
    // buffer.
    let (_dir, view, vcx) = visual_window(cx, &plain(3));
    view.update(vcx, |v, cx| v.run_command("toggle-file-view", cx));
    vcx.run_until_parked();
    let drawn = view.update(vcx, |v, _cx| v.painted_rows());
    assert!(drawn > 0, "nothing painted at all");
    assert!(drawn < 20, "painted {drawn} rows for a 3 line note");
}
