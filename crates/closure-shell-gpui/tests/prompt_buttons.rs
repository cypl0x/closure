//! "capture child weird position of accept and cancel."
//!
//! The screenshot shows them mid-bar, wherever the text you are typing
//! happens to end:
//!
//!     + child  inbox.org  closure  ▶ my text ◀ [✓ accept RET] [✕ cancel Esc]
//!
//! Reproduced on :1 by making the window 580px wide, which is what the
//! screenshot is: at that width the history hint (`M-p 1 back`) and the
//! label segments push the buttons past the right edge, and *cancel is
//! clipped off the screen* — an action you cannot click on a prompt you
//! opened by accident.
//!
//! The footer already learned this: "a flex row that cannot shrink
//! grows instead … the hints go in their own shrinkable, clipped
//! group". The prompt bar never did. The rule is which things yield:
//! the field and the two buttons are load-bearing, the decorations are
//! not.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* Alpha
:PROPERTIES:
:ID: 01PROMPTAAAAAAAAAAAAAAAAAA
:END:
";

/// Where the *last* button's right edge is, and where the bar's is.
///
/// The last one is cancel: accept ends 104px short of the edge because
/// cancel is sitting there, which is how the first version of this
/// test managed to fail against a correct layout.
fn edges(vcx: &mut gpui::VisualTestContext) -> (f32, f32) {
    let button = vcx
        .debug_bounds("prompt-escape")
        .expect("the cancel button is painted");
    // Capture draws its own bar rather than the prompt row, so the
    // measurement takes whichever is on screen.
    let bar = vcx
        .debug_bounds("prompt-row")
        .or_else(|| vcx.debug_bounds("capture-bar"))
        .expect("a prompt bar is painted");
    (
        f32::from(button.origin.x) + f32::from(button.size.width),
        f32::from(bar.origin.x) + f32::from(bar.size.width),
    )
}

/// The width the report was taken at.
fn narrow(vcx: &gpui::VisualTestContext) {
    vcx.simulate_resize(gpui::size(gpui::px(580.0), gpui::px(400.0)));
    vcx.run_until_parked();
}

#[gpui::test]
fn cancel_is_reachable_in_a_narrow_window(cx: &mut gpui::TestAppContext) {
    // The bug: at 580px the cancel button was painted past the right
    // edge. A prompt you opened by accident and cannot click out of.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    narrow(vcx);
    view.update(vcx, |v, cx| v.run_command("add-sibling", cx));
    vcx.run_until_parked();

    let cancel = vcx
        .debug_bounds("prompt-escape")
        .expect("the cancel button is painted");
    let bar = vcx.debug_bounds("prompt-row").expect("the prompt row");
    let right = f32::from(cancel.origin.x) + f32::from(cancel.size.width);
    let edge = f32::from(bar.origin.x) + f32::from(bar.size.width);
    assert!(
        right <= edge + 0.5,
        "cancel ends at {right}px and the window ends at {edge}px — it is off screen"
    );
}

#[gpui::test]
fn accept_and_cancel_sit_at_the_end_of_the_bar(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("add-sibling", cx));
    vcx.run_until_parked();
    let (button_right, bar_right) = edges(vcx);
    assert!(
        (bar_right - button_right).abs() < 24.0,
        "the buttons end at {button_right}px in a bar that ends at {bar_right}px"
    );
}

#[gpui::test]
fn they_do_not_move_as_you_type(cx: &mut gpui::TestAppContext) {
    // The complaint, as a measurement: a button that walks across the
    // bar while you type is one you cannot aim at.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("add-sibling", cx));
    vcx.run_until_parked();
    let (empty, _) = edges(vcx);

    vcx.simulate_input("a much longer title than before");
    vcx.run_until_parked();
    let (typed, _) = edges(vcx);

    assert!(
        (typed - empty).abs() < 1.0,
        "the accept button moved from {empty}px to {typed}px as the text grew"
    );
}

#[gpui::test]
fn the_same_holds_for_the_capture_bar(cx: &mut gpui::TestAppContext) {
    // `capture` has its own bar, and the report was about capture.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("capture", cx));
    vcx.run_until_parked();
    let (button_right, bar_right) = edges(vcx);
    assert!(
        (bar_right - button_right).abs() < 24.0,
        "capture's buttons end at {button_right}px in a bar that ends at {bar_right}px"
    );
}
