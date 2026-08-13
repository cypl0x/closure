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
    //
    // Rewritten 2026-08-13. It asserted that five presses reach
    // `manual-row-40`, and that threshold quietly encoded the defect
    // beside it: `C-d` was halving the *outline's* height, which on a
    // 720px window is the pane's whole page, so five presses covered
    // seventy-five rows. With the pane halving its own page they cover
    // half that, and row 40 is no longer in reach — which is the
    // intended behaviour, not a regression.
    //
    // The claim the test is actually making is "the pane moves", so it
    // now asks that directly and stays true whatever the step is.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    let before = view.update(vcx, |v, _| v.app().pane_cursor());
    for _ in 0..5 {
        vcx.simulate_keystrokes("ctrl-d");
    }
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _| v.app().pane_cursor());
    assert!(
        after > before,
        "`C-d` five times and the pane has not moved"
    );
    // …and it moved by pages, not by rows: five half-pages is a long
    // way down a list this size.
    assert!(
        after - before >= 20,
        "five `C-d` moved only {} rows — that is a line step, not a page",
        after - before
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

#[gpui::test]
fn a_long_line_wraps_instead_of_running_off_the_edge(cx: &mut gpui::TestAppContext) {
    // "Manual rows run off the right edge mid-word — and a command's
    // keys are at the end of its line, so what gets cut is exactly what
    // the manual exists to show."
    //
    // Bounds cannot see this directly: the row stays inside the pane
    // and the *text* overflows it. What a wrapped row does do is get
    // taller, so in a narrow window the long rows must be taller than
    // the short ones — and in a wide one they need not be.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    vcx.simulate_resize(gpui::size(gpui::px(760.0), gpui::px(720.0)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("g shift-k");
    vcx.run_until_parked();
    let mut heights: Vec<f32> = Vec::new();
    for n in 0..20usize {
        let sel: &'static str = Box::leak(format!("manual-row-{n}").into_boxed_str());
        if let Some(b) = vcx.debug_bounds(sel) {
            heights.push(f32::from(b.size.height));
        }
    }
    assert!(
        heights.len() > 5,
        "too few rows painted to tell: {heights:?}"
    );
    let tallest = heights.iter().copied().fold(f32::MIN, f32::max);
    let shortest = heights.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        tallest > shortest * 1.5,
        "every row is the same height in a 760px window, so nothing is \
         wrapping and the long ones are being cut: {heights:?}"
    );
}

#[gpui::test]
fn the_cursor_stays_on_screen_as_the_pane_pages(cx: &mut gpui::TestAppContext) {
    // "The pane shows about half the text it has room for."
    //
    // The complaint was that the rows look double-spaced, and measuring
    // it turned up something worse: `pane_view` counted the viewport in
    // 18px lines while a row painted at 27.5, so a 720px window was
    // handed 37 rows, could show about 24, and `C-d` paged past the
    // difference — content skipped rather than merely spaced out.
    //
    // Rows wrap now, so their height is not a constant and no
    // arithmetic can predict it. What can be held is the property that
    // matters: wherever the cursor goes, it is somewhere you can see.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    for _ in 0..4 {
        vcx.simulate_keystrokes("ctrl-d");
        vcx.run_until_parked();
        let at = vcx
            .debug_bounds("manual-cursor")
            .expect("the cursor is painted somewhere");
        assert!(
            at.top() >= gpui::px(0.0) && at.bottom() <= viewport.height,
            "the cursor is at {:?}..{:?} in a {:?} window — paging moved it \
             somewhere nobody can see, which is where the skipped rows went",
            at.top(),
            at.bottom(),
            viewport.height
        );
    }
}

#[gpui::test]
fn a_pane_pages_by_its_own_height_not_the_outlines(cx: &mut gpui::TestAppContext) {
    // `half-page-down` computes `outline_view.height / 2` and moves the
    // *pane* cursor by it. That is half the outline's viewport — a count
    // of single-line rows in a different list — while pane rows wrap and
    // the pane holds far fewer.
    //
    // The distinguishing observable is the size of the step, not where
    // the cursor lands: with a conservative page the cursor can still be
    // on screen while having stepped by the wrong number. So this counts
    // the rows the pane actually painted and requires the step to be
    // about half of *that*.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    open_manual(vcx);

    let mut painted = 0usize;
    for i in 0..120 {
        let selector: &'static str = Box::leak(format!("manual-row-{i}").into_boxed_str());
        if vcx.debug_bounds(selector).is_some() {
            painted += 1;
        }
    }
    assert!(painted > 4, "only {painted} rows painted; nothing to halve");

    let before = view.update(vcx, |v, _| v.app().pane_cursor());
    vcx.simulate_keystrokes("ctrl-d");
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _| v.app().pane_cursor());
    let step = after - before;

    assert!(step > 0, "`C-d` did not move the pane cursor");
    // Half, not all. Measured before the fix: a pane painting 15 rows
    // stepped 15 — `outline_view.height / 2` happened to equal the
    // pane's whole page, so every `C-d` skipped half the manual. That
    // is the user-visible half of this bug, and `step <= painted` is
    // too weak to catch it: 15 <= 15 passes.
    assert!(
        step * 2 <= painted + 1,
        "`C-d` stepped {step} rows in a pane showing {painted} — a half \
         page is a whole one, so half the text is skipped each press"
    );
}
