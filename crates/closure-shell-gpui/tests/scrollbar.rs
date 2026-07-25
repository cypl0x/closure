//! Scrollbar geometry.
//!
//! Every scrollable pane in the reference shell — outline, editor,
//! side lists, the which-key overlay — draws the same scrollbar, and
//! all of it reduces to two pure conversions: content extents to a
//! thumb rectangle, and a drag position back to a scroll offset. gpui
//! has no scrollbar widget, so this is ours, and this is where its
//! arithmetic is pinned.
//!
//! Fractions everywhere: the geometry is resolution-independent, and
//! the window multiplies by the track's pixel height when it paints.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{scroll_for_track_fraction, thumb_geometry};

/// Fractions are compared with a tolerance; these are floats, and the
/// tests care about the geometry, not the last bit.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn content_that_fits_has_no_scrollbar() {
    assert_eq!(
        thumb_geometry(100.0, 100.0, 0.0, 0.1),
        None,
        "content exactly filling the viewport needs no bar"
    );
    assert_eq!(thumb_geometry(100.0, 40.0, 0.0, 0.1), None, "shorter still");
}

#[test]
fn a_degenerate_viewport_has_no_scrollbar() {
    // Before the first layout the viewport is zero; dividing by it
    // would produce NaN and paint a garbage thumb.
    assert_eq!(thumb_geometry(0.0, 500.0, 0.0, 0.1), None);
}

#[test]
fn the_thumb_is_the_visible_fraction_of_the_content() {
    // A quarter of the content is visible -> a quarter-height thumb.
    let t = thumb_geometry(100.0, 400.0, 0.0, 0.05).expect("scrollable");
    assert!(close(t.height, 0.25), "height {}", t.height);
    assert!(close(t.top, 0.0), "at the top: {}", t.top);
}

#[test]
fn scrolling_moves_the_thumb_down_the_track() {
    // Viewport 100, content 400 -> 300 of scroll range.
    let half = thumb_geometry(100.0, 400.0, 150.0, 0.05).expect("scrollable");
    // Half scrolled: the thumb top sits half way down the free track
    // (the track minus the thumb), i.e. 0.5 * (1 - 0.25).
    assert!(close(half.top, 0.375), "top {}", half.top);
    let end = thumb_geometry(100.0, 400.0, 300.0, 0.05).expect("scrollable");
    assert!(close(end.top + end.height, 1.0), "bottom flush: {end:?}");
}

#[test]
fn the_thumb_never_escapes_the_track() {
    // Over-scroll in both directions must clamp, not overflow.
    let over = thumb_geometry(100.0, 400.0, 9_999.0, 0.05).expect("scrollable");
    assert!(close(over.top + over.height, 1.0), "clamped: {over:?}");
    let under = thumb_geometry(100.0, 400.0, -50.0, 0.05).expect("scrollable");
    assert!(close(under.top, 0.0), "clamped: {under:?}");
}

#[test]
fn a_tiny_thumb_is_floored_so_it_stays_grabbable() {
    // 40 000 rows in a 20-row viewport would give a 1-pixel thumb.
    let t = thumb_geometry(100.0, 200_000.0, 0.0, 0.06).expect("scrollable");
    assert!(close(t.height, 0.06), "floored to the minimum: {t:?}");
    // …and the floor must not push it off the end at full scroll.
    let end = thumb_geometry(100.0, 200_000.0, 199_900.0, 0.06).expect("scrollable");
    assert!(close(end.top + end.height, 1.0), "still flush: {end:?}");
    assert!(end.top >= 0.0);
}

// === the inverse: dragging the thumb ===

#[test]
fn dragging_to_the_top_scrolls_to_zero() {
    assert!(close(scroll_for_track_fraction(100.0, 400.0, 0.0), 0.0));
}

#[test]
fn dragging_to_the_bottom_scrolls_to_the_end() {
    assert!(close(scroll_for_track_fraction(100.0, 400.0, 1.0), 300.0));
}

#[test]
fn dragging_half_way_scrolls_half_the_range() {
    assert!(close(scroll_for_track_fraction(100.0, 400.0, 0.5), 150.0));
}

#[test]
fn a_drag_outside_the_track_clamps() {
    assert!(close(scroll_for_track_fraction(100.0, 400.0, -2.0), 0.0));
    assert!(close(scroll_for_track_fraction(100.0, 400.0, 7.0), 300.0));
}

#[test]
fn unscrollable_content_never_scrolls() {
    assert!(close(scroll_for_track_fraction(100.0, 50.0, 1.0), 0.0));
    assert!(close(scroll_for_track_fraction(0.0, 500.0, 1.0), 0.0));
}

#[test]
fn drag_round_trips_through_the_thumb_geometry() {
    // Grabbing the thumb, dragging to fraction f, and reading the
    // thumb back must land where the drag asked — the property that
    // makes the bar feel attached to the content.
    for f in [0.0_f32, 0.1, 0.5, 0.9, 1.0] {
        let offset = scroll_for_track_fraction(100.0, 400.0, f);
        let t = thumb_geometry(100.0, 400.0, offset, 0.05).expect("scrollable");
        let free = 1.0 - t.height;
        assert!(
            close(t.top, f * free),
            "fraction {f} -> offset {offset} -> thumb {t:?}"
        );
    }
}
