//! "This UI element may need a min size in order to prevent jumpy UI.
//! Because when go from line 9 to 10 or from row 99 to 100 (and back)
//! it will move the UI. That doesn't look good to me."
//!
//! Exactly right: the `L9:C1` chip was `flex_none` with no width of its
//! own, so it grew by a character at every power of ten and pushed the
//! dirty dot, the macro indicator and the chord echo sideways with it.
//! A status line that twitches while you type is worse than one that
//! reserves a little space it does not always use.
//!
//! So the element reserves room for the sizes a note actually reaches,
//! and grows past it only for a file long enough to need it — where
//! one reflow is better than clipping the number.

#![cfg(feature = "gpui-test")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    missing_docs
)]

use closure_shell_gpui::cursor_label_w;

/// Roughly the advance of the 11px monospace the chip is painted in.
const ADVANCE: f32 = 7.2;

#[test]
fn crossing_from_nine_to_ten_does_not_move_anything() {
    // The report, stated directly.
    assert!(
        (cursor_label_w("L9:C1", ADVANCE) - cursor_label_w("L10:C1", ADVANCE)).abs() < f32::EPSILON,
        "the element changed width between line 9 and line 10"
    );
}

#[test]
fn crossing_from_ninety_nine_to_a_hundred_does_not_move_anything() {
    assert!(
        (cursor_label_w("L99:C99", ADVANCE) - cursor_label_w("L100:C100", ADVANCE)).abs()
            < f32::EPSILON,
        "the element changed width at a hundred"
    );
}

#[test]
fn a_file_long_enough_to_need_more_room_gets_it() {
    // The reserve is a floor, not a clamp: a note long enough to run
    // past it must show its number rather than have it cut off to
    // protect the layout. (`L12000:C1` is nine characters and still
    // fits inside the reserve — the first label that does not is one
    // with a long column too.)
    let big = cursor_label_w("L120000:C1000", ADVANCE);
    assert!(
        big > cursor_label_w("L9:C1", ADVANCE),
        "a long label was squeezed into the reserve: {big}"
    );
    assert!(
        big >= "L120000:C1000".chars().count() as f32 * ADVANCE,
        "the label does not fit in {big}px"
    );
}

#[test]
fn the_reserve_follows_the_font() {
    // Zoom moves the advance, and a width in fixed pixels would either
    // clip at large zoom or waste half the header at small.
    assert!(cursor_label_w("L1:C1", ADVANCE * 2.0) > cursor_label_w("L1:C1", ADVANCE));
}
