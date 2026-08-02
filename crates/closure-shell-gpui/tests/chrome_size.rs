//! "especially the left most (Outline, Agenda, ...), bottom, top
//! (header) font is too tiny"
//!
//! Three named places, and all three were smaller than the prose they
//! sit around: the rail's labels at 12, the header's buttons at 11,
//! the footer's hints at whatever the row inherited. They are not
//! annotations — the rail is how you move between panes and the footer
//! is how you learn the keys — so they are read as often as the body
//! and were set two and three pixels below it.
//!
//! They ask the theme for a step by name now. The point of the test is
//! that they ask the *same* step: three sizes that merely happen to
//! agree today would drift the first time one of them was touched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    missing_docs
)]

use closure_shell_core::{Theme, TypeStep};
use closure_shell_gpui::{chrome_px, scaled_text_px};

#[test]
fn the_chrome_is_within_a_pixel_of_the_prose() {
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let body = f32::from(theme.typography.step_px(TypeStep::Body));
        let chrome = chrome_px(&theme, 1.0);
        assert!(
            chrome + 1.0 >= body,
            "{}: chrome {chrome} against body {body}",
            theme.name
        );
    }
}

#[test]
fn it_is_the_ui_step_and_not_a_number_that_matches_it() {
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert_eq!(
            chrome_px(&theme, 1.0),
            f32::from(theme.typography.step_px(TypeStep::Ui)),
            "{}",
            theme.name
        );
    }
}

#[test]
fn zoom_moves_it_the_way_it_moves_everything_else() {
    // The chrome scaling independently of the text would be a window
    // that zooms in two directions at once.
    let theme = Theme::dark();
    let one = chrome_px(&theme, 1.0);
    let two = chrome_px(&theme, 2.0);
    assert!(two > one, "{one} → {two}");
    assert_eq!(two, scaled_text_px(one, 2.0));
}

#[test]
fn a_bigger_theme_carries_the_chrome_with_it() {
    assert!(
        chrome_px(&Theme::high_contrast(), 1.0) > chrome_px(&Theme::dark(), 1.0),
        "high-contrast declares a larger base for a reason"
    );
}
