//! "yellow marker? What is the purpose? Is it a bug?" — with a
//! screenshot of one outline row carrying an amber bar down its left
//! edge where every other row has the blue selection marker or nothing.
//!
//! The purpose: it is the drag-and-drop insertion indicator, which
//! marks the row a dragged headline would land under. It is supposed
//! to be a line along the *bottom* of that row.
//!
//! And yes, a bug. gpui's `border_color` sets one colour for every
//! side, and the row already carries a 2px left border as its
//! selection marker — transparent on unselected rows so that adding it
//! to the selected one does not shove that row's content sideways. So
//! asking for an amber bottom border repainted the left marker amber
//! too, on a row that was not selected, and the insertion line it was
//! meant to draw was the thing you did not notice.
//!
//! Two markers, two elements. They cannot recolour each other now.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::Theme;
use closure_shell_gpui::{BodySpan, drop_line_color, selection_marker_color, span_color_of};

/// The warning colour of a theme, which is what the marker was.
fn warning(theme: &Theme) -> u32 {
    span_color_of(theme, BodySpan::Priority)
}

#[test]
fn the_selection_marker_is_never_the_drop_colour() {
    // The bug, exactly: an unselected row showed the amber bar.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        for selected in [true, false] {
            assert_ne!(
                selection_marker_color(&theme, selected),
                warning(&theme),
                "{} selected={selected}",
                theme.name
            );
        }
    }
}

#[test]
fn an_unselected_row_has_no_visible_marker() {
    // Transparent rather than absent: added only to the selected row,
    // its 2px pushed that row's content right, so moving down the list
    // nudged each title sideways as it arrived.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert_eq!(selection_marker_color(&theme, false), 0, "{}", theme.name);
        assert_ne!(selection_marker_color(&theme, true), 0, "{}", theme.name);
    }
}

#[test]
fn the_drop_line_is_the_warning_colour() {
    // It is a deliberate interruption — something is about to move.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert_eq!(drop_line_color(&theme), warning(&theme), "{}", theme.name);
    }
}

#[test]
fn the_two_markers_are_told_apart_by_colour() {
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert_ne!(
            drop_line_color(&theme),
            selection_marker_color(&theme, true),
            "{}",
            theme.name
        );
    }
}
