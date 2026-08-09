//! "Without filter messages scroll view doesn't follow cursor … As you
//! can see in the Screenshot the scrollbar doesn't follow the selected
//! element."
//!
//! Sixty messages in the popup, the thumb pinned at the top, and the
//! selection somewhere below the fold. Moving down walked the cursor
//! and left the view where it was, so past the tenth entry you were
//! choosing something you could not see.
//!
//! The palette had the fix already — `scroll_to_item` when its cursor
//! moves — and only the palette. Every one of these popups is drawn by
//! the same overlay from the same `picker_view`, so the reveal belongs
//! to the picker rather than to one surface of it.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01PICKSCROLL00000000001
:END:
";

#[gpui::test]
fn the_message_log_scrolls_to_what_is_selected(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    // Enough messages that the selection can leave the panel.
    // Sixty entries in the log, made the way the log fills up: every
    // command that says something leaves a line in it.
    view.update(vcx, |v, cx| {
        for _ in 0..60 {
            v.run_command("toggle-wrap", cx);
        }
        v.run_command("messages", cx);
    });
    vcx.run_until_parked();
    let top = view.update(vcx, |v, _| v.picker_scroll_offset());

    // Walk down past the fold.
    view.update(vcx, |v, cx| {
        for _ in 0..40 {
            v.press("down", false, false, cx);
        }
    });
    vcx.run_until_parked();
    let moved = view.update(vcx, |v, _| v.picker_scroll_offset());

    assert!(
        (moved - top).abs() > 1.0,
        "the view never moved: offset {top} before, {moved} after forty steps down"
    );
}

#[gpui::test]
fn the_palette_still_follows_its_own(cx: &mut gpui::TestAppContext) {
    // The one surface that already worked, so the generalisation
    // cannot have cost it.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("palette", cx));
    vcx.run_until_parked();
    let top = view.update(vcx, |v, _| v.picker_scroll_offset());
    view.update(vcx, |v, cx| {
        for _ in 0..40 {
            v.press("down", false, false, cx);
        }
    });
    vcx.run_until_parked();
    let moved = view.update(vcx, |v, _| v.picker_scroll_offset());
    assert!(
        (moved - top).abs() > 1.0,
        "the palette stopped following: {top} then {moved}"
    );
}
