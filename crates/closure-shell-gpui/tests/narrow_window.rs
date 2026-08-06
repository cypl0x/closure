//! "The window does not reflow below about 600px wide."
//!
//! Filed while fixing the prompt bar, whose buttons were the first
//! thing you saw go. The cause is under all of it: the rail (146px, or
//! 38 docked) plus the outline's 220px minimum plus the detail pane's
//! own minimum want more than a narrow window has, so everything is
//! clipped at the right edge rather than getting narrower.
//!
//! What should give, in order of what it costs to lose:
//!
//! 1. the detail pane — the outline is what you navigate with, and the
//!    detail is a preview of the row you are on;
//! 2. the rail's labels — the icons still say where the panes are, and
//!    the tooltip still names them;
//! 3. the outline's minimum — a narrow list of titles still lists.
//!
//! Each step is only taken when the one before it was not enough.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* Alpha
:PROPERTIES:
:ID: 01NARROWAAAAAAAAAAAAAAAAAA
:END:
body
";

fn at(vcx: &gpui::VisualTestContext, w: f32) {
    vcx.simulate_resize(gpui::size(gpui::px(w), gpui::px(600.0)));
    vcx.run_until_parked();
}

/// Where a pane's right edge is.
///
/// Only ever asked of panes that are painted in the frame being
/// measured: gpui's debug-bounds map keeps the last value an element
/// was painted at, so an element that has *stopped* being painted
/// still answers, with a stale number. That is why "the detail pane is
/// gone" is measured as "the outline took the whole row" rather than
/// as its absence from the map.
fn right(vcx: &mut gpui::VisualTestContext, sel: &'static str) -> f32 {
    let b = vcx.debug_bounds(sel).expect("the pane is painted");
    f32::from(b.origin.x) + f32::from(b.size.width)
}

#[gpui::test]
fn nothing_is_painted_past_the_right_edge_at_any_width(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    for w in [
        1400.0, 1000.0, 800.0, 692.0, 600.0, 500.0, 366.0, 300.0, 200.0,
    ] {
        at(vcx, w);
        for sel in ["rail-column", "outline-pane"] {
            let edge = right(vcx, sel);
            assert!(
                edge <= w + 1.0,
                "at {w}px the {sel} reaches {edge}px in a {w}px window"
            );
        }
    }
}

#[gpui::test]
fn the_detail_pane_is_the_first_thing_to_go(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    at(vcx, 1400.0);
    let edge = right(vcx, "outline-pane");
    assert!(
        edge < 1399.0,
        "the outline reaches {edge}px of a 1400px window — the detail pane is gone at a width that fits it"
    );
    at(vcx, 600.0);
    let edge = right(vcx, "outline-pane");
    assert!(
        (edge - 600.0).abs() < 1.0,
        "the outline stops at {edge}px of a 600px window — the detail pane is still taking width the window has not got"
    );
}

#[gpui::test]
fn the_rail_docks_itself_before_the_outline_suffers(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    at(vcx, 1400.0);
    let wide = f32::from(
        vcx.debug_bounds("rail-column")
            .expect("the rail")
            .size
            .width,
    );
    at(vcx, 340.0);
    let narrow = f32::from(
        vcx.debug_bounds("rail-column")
            .expect("the rail is still there")
            .size
            .width,
    );
    assert!(
        narrow < wide * 0.5,
        "the rail is still {narrow}px wide in a 340px window (was {wide}px)"
    );
    // And the outline has not been made to pay for it yet.
    let outline = f32::from(
        vcx.debug_bounds("outline-pane")
            .expect("the outline")
            .size
            .width,
    );
    assert!(
        outline >= 219.0,
        "the outline is {outline}px — below its own minimum while the rail still had width to give"
    );
}

#[gpui::test]
fn the_rail_comes_back_when_the_window_does(cx: &mut gpui::TestAppContext) {
    // Reflow is a response to the width, not a setting it changes: a
    // window you narrowed and widened again is the window you had.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    at(vcx, 1400.0);
    let before = f32::from(vcx.debug_bounds("rail-column").expect("rail").size.width);
    at(vcx, 340.0);
    at(vcx, 1400.0);
    let after = f32::from(vcx.debug_bounds("rail-column").expect("rail").size.width);
    assert!(
        (after - before).abs() < 0.5,
        "the rail stayed docked at {after}px after the window came back to {before}px"
    );
    let edge = right(vcx, "outline-pane");
    assert!(
        edge < 1399.0,
        "the outline still owns the row out to {edge}px — the detail pane did not come back"
    );
}

#[gpui::test]
fn the_outline_gives_up_its_minimum_only_last(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    at(vcx, 200.0);
    let outline = vcx
        .debug_bounds("outline-pane")
        .expect("the outline is still the pane you navigate with");
    let w = f32::from(outline.size.width);
    assert!(
        w > 0.0 && w < 220.0,
        "the outline is {w}px in a 200px window — it has to go below its minimum here"
    );
}

#[gpui::test]
fn the_file_column_gives_the_title_its_width_back(cx: &mut gpui::TestAppContext) {
    // Measured on :1 at 280px: the outline had the row to itself and
    // was still spending 70 of its 242 pixels on `notes.org`, so the
    // titles came out as "Read:", "Hin", "Flo". Which file a headline
    // is in is worth a column when there is a column to spare.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);

    at(vcx, 1400.0);
    let title = right(vcx, "title-0");
    let pane = right(vcx, "outline-pane");
    assert!(
        title < pane - 40.0,
        "the title reaches {title}px of a {pane}px outline — the file column is gone at a width that fits it"
    );

    // 700 is the case that made the rule what it is: the outline is
    // not the narrow one here, the *window* is, and the outline was
    // squeezed to 228px to keep the preview — with 60 of those still
    // going to the file name.
    for w in [700.0, 280.0] {
        at(vcx, w);
        let title = right(vcx, "title-0");
        let pane = right(vcx, "outline-pane");
        assert!(
            title > pane - 40.0,
            "at {w}px the title stops at {title}px of a {pane}px outline — the file column is still taking the room"
        );
    }
}

#[gpui::test]
fn the_outline_shrinks_before_the_preview_starves(cx: &mut gpui::TestAppContext) {
    // Between "everything fits" and "the detail pane goes" there is a
    // band where it is still painted and is not a preview: the outline
    // holds the width it was dragged to and the detail takes what is
    // left, which at 700px was 128 pixels of hyphenated fragments. The
    // outline is the one with slack in it — it has a minimum, and
    // above that minimum it is a preference.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    for w in [900.0_f32, 800.0, 700.0] {
        at(vcx, w);
        let side = f32::from(
            vcx.debug_bounds("side-pane")
                .expect("the detail pane is painted at this width")
                .size
                .width,
        );
        assert!(
            side >= 320.0,
            "at {w}px the detail pane is {side}px — kept, but too narrow to be a preview"
        );
    }
}
