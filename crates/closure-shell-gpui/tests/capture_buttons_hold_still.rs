//! "capture accept and cancel button weird position" — annotated on
//! the screenshot itself: "It moves :)".
//!
//! The accept/cancel pair sits immediately after the text you are
//! typing, so it slides right with every character. A button that is
//! somewhere else each time you look at it is a button you have to
//! find before you can click it, and the two here are the ones you
//! reach for when a capture has gone wrong.
//!
//! The cause is the one the prompt row already has a comment about:
//! `flex_1` on the field resolves against nothing unless the row it is
//! in has a width. The capture bar shrink-wrapped its content, so the
//! field was as wide as the text and the buttons began where the text
//! ended.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01CAPBTN00000000000000001
:END:
";

fn accept_x(vcx: &mut gpui::VisualTestContext) -> f32 {
    f32::from(
        vcx.debug_bounds("prompt-enter")
            .expect("the accept button is painted")
            .origin
            .x,
    )
}

#[gpui::test]
fn the_buttons_do_not_move_as_you_type(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("capture", cx));
    vcx.run_until_parked();
    let empty = accept_x(vcx);

    vcx.simulate_input("a capture whose title keeps going and going");
    vcx.run_until_parked();
    let typed = accept_x(vcx);

    assert!(
        (typed - empty).abs() < 1.0,
        "accept sat at {empty}px with an empty field and {typed}px with a full one — it moves"
    );
}

#[gpui::test]
fn the_buttons_sit_at_the_right_edge(cx: &mut gpui::TestAppContext) {
    // Where they do not move *to* matters as much: pinned to the left
    // of a wide window would also be still, and would still be wrong.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("capture", cx));
    vcx.run_until_parked();

    let cancel = vcx
        .debug_bounds("prompt-escape")
        .expect("the cancel button is painted");
    let window = vcx.debug_bounds("closure-root").expect("the root");
    let right_edge = f32::from(window.size.width);
    let cancel_right = f32::from(cancel.origin.x) + f32::from(cancel.size.width);
    assert!(
        right_edge - cancel_right < 24.0,
        "cancel ends {}px from the right edge of a {right_edge}px window",
        right_edge - cancel_right
    );
}

#[gpui::test]
fn a_long_title_still_leaves_them_where_they_were(cx: &mut gpui::TestAppContext) {
    // The field yields, the buttons do not: that is the rule the
    // narrow-window work established for the prompt row, and the
    // capture bar is the surface the report is about.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("capture", cx));
    vcx.run_until_parked();
    let before = accept_x(vcx);
    vcx.simulate_input(&"x".repeat(400));
    vcx.run_until_parked();
    let after = accept_x(vcx);
    assert!(
        (after - before).abs() < 1.0,
        "four hundred characters moved accept from {before}px to {after}px"
    );
}
