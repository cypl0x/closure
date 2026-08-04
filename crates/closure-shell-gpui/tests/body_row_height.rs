//! "weird editor (font) bug: halfed lines … this org content is
//! 'stable' broken … It changes depending of which part of the file I
//! am looking at."
//!
//! Every line in the buffer was drawn clipped, top half over bottom
//! half of its neighbour. The rows state their height —
//! `.h(px(body_row_h(zoom)))` — and the viewport is counted in that
//! same number, so it looked settled. But a definite height in a flex
//! column is still *shrinkable*: `flex-shrink` defaults to 1. Once the
//! content is taller than the pane, every row gives up a share of
//! itself, and the text inside gets squeezed out of its own box.
//!
//! That is why it depended on what was on screen. The org file that
//! reproduces it carries two inline pictures, each `IMAGE_ROWS` tall;
//! with those in view the column overflows and everything visible
//! shrinks, and with them scrolled away it looks fine.
//!
//! A row is exactly one row tall. That is what the caret, the
//! scrollbar and the page-down arithmetic all assume.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{body_row_h, visual_window};

/// Enough lines to overflow the pane, with pictures among them — the
/// shape of the file in the report.
fn tall_note() -> String {
    let mut org = String::from("* Note\n:PROPERTIES:\n:ID: 01ROWH00000000000000000A\n:END:\n");
    for i in 0..60 {
        use std::fmt::Write as _;
        let _ = writeln!(org, "line {i} of the body");
    }
    org.push_str("[[file:assets/one.png]]\n");
    org.push_str("more text after the picture\n");
    org.push_str("[[file:assets/two.png]]\n");
    org.push_str("last line\n");
    org
}

#[gpui::test]
fn every_row_is_exactly_one_row_tall(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, &tall_note());
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();

    let expected = view.update(vcx, |v, _cx| body_row_h(v.zoom()));
    let mut checked = 0;
    for ln in 0..40 {
        let selector: &'static str = Box::leak(format!("body-row-{ln}").into_boxed_str());
        let Some(bounds) = vcx.debug_bounds(selector) else {
            continue;
        };
        checked += 1;
        let h = f32::from(bounds.size.height);
        assert!(
            (h - expected).abs() < 0.5,
            "row {ln} is {h}px in a {expected}px row — the lines are being squeezed"
        );
    }
    assert!(checked > 5, "only {checked} rows were painted at all");
}
