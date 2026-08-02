//! "Try to improve the font size … especially the left most (Outline,
//! Agenda, ...), bottom, top (header) font is too tiny"
//!
//! There was no scale — eighty-five literal sizes scattered through
//! the painter, `10.0` here and `11.0` there and `12.0` next to it, so
//! "the chrome is too small" had eighty-five places to be fixed and no
//! way to stay fixed. Three of those numbers were the rail, the footer
//! and the header, and all three were smaller than the prose they sit
//! around.
//!
//! One scale, in the theme, derived from the one size a theme already
//! declared. A shell asks for a step by name; the steps keep their
//! order by construction, so no future edit can make a badge bigger
//! than the body text it annotates.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{Theme, TypeStep};

#[test]
fn the_steps_are_ordered_largest_first() {
    let t = Theme::dark().typography;
    assert!(t.step_px(TypeStep::Body) > t.step_px(TypeStep::Ui));
    assert!(t.step_px(TypeStep::Ui) > t.step_px(TypeStep::Small));
    assert!(t.step_px(TypeStep::Small) > t.step_px(TypeStep::Tiny));
}

#[test]
fn body_text_is_the_size_the_theme_declares() {
    // `base_px` is the one number a theme states, and the body is it.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert_eq!(
            theme.typography.step_px(TypeStep::Body),
            theme.typography.base_px,
            "{}",
            theme.name
        );
    }
}

#[test]
fn the_chrome_is_not_tiny() {
    // The complaint, as a number: the rail, the footer and the header
    // all use the `Ui` step, and it has to stay within a pixel of the
    // prose beside it. Twelve was three below the body and read as an
    // afterthought.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let t = theme.typography;
        assert!(
            t.step_px(TypeStep::Ui) + 1 >= t.base_px,
            "{}: chrome {} vs body {}",
            theme.name,
            t.step_px(TypeStep::Ui),
            t.base_px
        );
    }
}

#[test]
fn the_smallest_step_is_still_readable() {
    // A badge nobody can read is a badge nobody looks at.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert!(
            theme.typography.step_px(TypeStep::Tiny) >= 11,
            "{}: {}",
            theme.name,
            theme.typography.step_px(TypeStep::Tiny)
        );
    }
}

#[test]
fn a_larger_base_moves_the_whole_scale() {
    // high-contrast declares a bigger base for the same reason it
    // declares louder colours, and the steps have to follow it rather
    // than sit at fixed pixel sizes underneath it.
    let small = Theme::dark().typography;
    let large = Theme::high_contrast().typography;
    assert!(large.base_px > small.base_px, "the premise");
    for step in [
        TypeStep::Body,
        TypeStep::Ui,
        TypeStep::Small,
        TypeStep::Tiny,
    ] {
        assert!(
            large.step_px(step) > small.step_px(step),
            "{step:?} did not follow the base"
        );
    }
}
