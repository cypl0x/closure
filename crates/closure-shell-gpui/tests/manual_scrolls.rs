//! "`M-x manual` opens, the pane paints a scrollbar, and `C-d` / `C-u`
//! move nothing — so everything below the first screen of the manual is
//! unreachable."
//!
//! The kernel half is in `closure-shell-core/tests/pane_scrolls.rs`:
//! the cursor walked all along, and `pane_window` now says which rows a
//! viewport of a given height should show for it. This is the half that
//! decides whether any of that reaches the glass — the pane painted
//! every row it had, from row zero, forever, and highlighted none of
//! them, so pressing `j` in the manual changed nothing you could see.
//!
//! Rows carry their *absolute* index in the selector, so "it scrolled"
//! is a question a test can ask — but only in one direction. The
//! window's debug map keeps the last bounds each selector was painted
//! at and never forgets one, so "row zero is gone" is unobservable: it
//! is still in the map from the frame it opened on. A row that was
//! never painted before and is painted now is sound, and that is what
//! these ask.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "* One\n:PROPERTIES:\n:ID: 01HQMANUAL00000000000001\n:END:\nbody\n";

/// Open the manual the way the vim keymap does (`g K`), in a window
/// the size of a real one.
///
/// The default test window is maximized over gpui's stub display and
/// fits the whole manual, which is the one case where none of this
/// matters — the report is from a 1080x720 window, where it does not
/// come close to fitting.
fn open_manual(vcx: &mut gpui::VisualTestContext) {
    vcx.simulate_resize(gpui::size(gpui::px(1080.0), gpui::px(720.0)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("g shift-k");
    vcx.run_until_parked();
}

#[gpui::test]
fn the_manual_opens_at_its_first_row(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    assert!(
        vcx.debug_bounds("manual-row-0").is_some(),
        "the manual did not open, or its rows are not identifiable"
    );
}

#[gpui::test]
fn walking_down_scrolls_the_pane(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    // A 720px window fits about thirty-seven rows, so row forty is
    // below the fold when it opens...
    assert!(
        vcx.debug_bounds("manual-row-40").is_none(),
        "the pane is taller than the test assumes; nothing here is proven"
    );
    for _ in 0..40 {
        vcx.simulate_keystrokes("j");
    }
    vcx.run_until_parked();
    // ...and the cursor cannot walk onto it without the pane moving.
    assert!(
        vcx.debug_bounds("manual-row-40").is_some(),
        "forty rows down and the pane never followed the cursor"
    );
}

#[gpui::test]
fn half_a_page_scrolls_the_pane(cx: &mut gpui::TestAppContext) {
    // The chord in the report. It was bound, and it moved the outline's
    // selection behind the pane instead of the pane.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    assert!(vcx.debug_bounds("manual-row-40").is_none(), "see above");
    for _ in 0..5 {
        vcx.simulate_keystrokes("ctrl-d");
    }
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("manual-row-40").is_some(),
        "`C-d` five times and the pane has not moved"
    );
}

#[gpui::test]
fn the_row_under_the_cursor_is_marked(cx: &mut gpui::TestAppContext) {
    // Scrolling a pane whose cursor is invisible is still guesswork.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    vcx.simulate_keystrokes("j j j");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("manual-cursor").is_some(),
        "nothing on screen says which row the pane is on"
    );
}
