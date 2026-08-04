//! "editor UI jitter in editor when which-key opens … And closed.
//! Moves the whole editor pane."
//!
//! Two screenshots of the same buffer, one with the which-key panel up
//! and one without, and the text sits at different heights in each.
//!
//! Same shape as the toast strip, which this codebase already learned:
//! as a flex child, a panel takes its height out of the column, so
//! everything above it moves whenever it appears or goes away. The
//! toasts were fixed by deferring and anchoring them — "a message
//! landing on top of the thing you are reading is the complaint", and
//! the editor jumping under it is the other half of the same
//! complaint.
//!
//! which-key opens on a chord you are *mid-way through typing*. The
//! text moving under the caret at that moment is the worst possible
//! time for it.
//!
//! Measured before the fix, in this harness: the pane went from 962px
//! to 739.5px. On :1, the same buffer showed 27 lines with which-key
//! closed and 14 with it open.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

/// Enough lines to overflow the pane, so the count of painted rows is
/// a measurement rather than a constant. A short note cannot show this
/// bug at all: with slack in the column, the rows keep their places
/// and only the empty space below them changes.
fn tall_note() -> String {
    let mut org =
        String::from("* Long note\n:PROPERTIES:\n:ID: 01JITTER00000000000000000A\n:END:\n");
    for i in 0..80 {
        use std::fmt::Write as _;
        let _ = writeln!(org, "line {i} of the body");
    }
    org
}

/// Where the first body row sits, and how many rows the pane has room
/// for — which is the thing that changes when a panel takes height out
/// of the column.
fn geometry(vcx: &mut gpui::VisualTestContext) -> (f32, f32) {
    // The pane's own bounds, not the rows inside it. Counting painted
    // rows looked like the obvious measurement and is not one: the
    // harness paints what the element asks for rather than what fits,
    // so the count stayed put while the pane on screen lost half its
    // height. On :1 the same buffer showed 27 lines with which-key
    // closed and 14 with it open.
    let pane = vcx
        .debug_bounds("body-pane")
        .expect("the body pane is painted");
    (f32::from(pane.origin.y), f32::from(pane.size.height))
}

#[gpui::test]
fn opening_which_key_does_not_move_the_editor(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, &tall_note());
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let closed = geometry(vcx);

    view.update(vcx, |v, cx| v.run_command("toggle-which-key", cx));
    vcx.run_until_parked();
    let open = geometry(vcx);

    assert!(
        (open.0 - closed.0).abs() < 0.5,
        "the first line moved from y={} to y={} when which-key opened",
        closed.0,
        open.0
    );
    assert!(
        (open.1 - closed.1).abs() < 0.5,
        "the editor pane changed height when which-key opened: \
         {}px before, {}px after",
        closed.1,
        open.1
    );
}

#[gpui::test]
fn closing_it_again_puts_nothing_back(cx: &mut gpui::TestAppContext) {
    // The other half: a panel that only pushes on the way in would
    // still pull on the way out.
    let (_dir, view, vcx) = visual_window(cx, &tall_note());
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let before = geometry(vcx);

    for _ in 0..2 {
        view.update(vcx, |v, cx| v.run_command("toggle-which-key", cx));
        vcx.run_until_parked();
    }
    let after = geometry(vcx);
    assert!(
        (after.0 - before.0).abs() < 0.5 && (after.1 - before.1).abs() < 0.5,
        "a round trip through which-key moved the editor: {before:?} -> {after:?}"
    );
}

#[gpui::test]
fn which_key_is_still_painted_when_it_is_open(cx: &mut gpui::TestAppContext) {
    // Not moving the editor by not being there would be a poor fix.
    let (_dir, view, vcx) = visual_window(cx, &tall_note());
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    view.update(vcx, |v, cx| v.run_command("toggle-which-key", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("which-key-panel").is_some(),
        "which-key is open and nothing was painted"
    );
}
