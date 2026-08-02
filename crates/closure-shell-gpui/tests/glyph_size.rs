//! "tiny icons/glyphs — Especially the folded/unfolded indicator is too
//! tiny. Please prefer creating consistent views throughout the App.
//! Reuse them. You could create your own view just for each of these
//! Icons with a custom font size boldness and whatever … Do increase
//! the size of the TODO and DONE texts as well. Maybe even a bold or
//! semibold font?" and "very tiny search icon".
//!
//! The chrome got a scale; the glyphs did not. A fold arrow, a status
//! dot and a search magnifier are one or two characters carrying a
//! whole meaning, so they need *more* size than a word does, not the
//! leftovers — and they were being drawn at whatever their container
//! inherited, which after the chrome was sized was smaller than the
//! prose beside them.
//!
//! One step for all of them, above the body, and the keyword chips
//! bold: a two-character mark that is not heavier than a sentence
//! disappears into it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    missing_docs
)]

use closure_shell_core::{Theme, TypeStep};
use closure_shell_gpui::{chip_text_px, chrome_px, glyph_px};

const ZOOMS: [f32; 4] = [0.75, 1.0, 1.5, 3.0];

#[test]
fn a_glyph_is_larger_than_the_words_beside_it() {
    // The complaint: one or two characters carrying a whole meaning,
    // drawn smaller than a sentence.
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let body = f32::from(theme.typography.step_px(TypeStep::Body));
        assert!(
            glyph_px(&theme, 1.0) >= body,
            "{}: glyph {} vs body {body}",
            theme.name,
            glyph_px(&theme, 1.0)
        );
    }
}

#[test]
fn it_is_larger_than_the_chrome_too() {
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        assert!(
            glyph_px(&theme, 1.0) > chrome_px(&theme, 1.0),
            "{}",
            theme.name
        );
    }
}

#[test]
fn the_keyword_chip_is_no_longer_the_smallest_thing_on_the_row() {
    // "Do increase the size of the TODO and DONE texts as well."
    for theme in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
        let small = f32::from(theme.typography.step_px(TypeStep::Small));
        assert!(
            chip_text_px(&theme, 1.0) >= small,
            "{}: chip {} vs small {small}",
            theme.name,
            chip_text_px(&theme, 1.0)
        );
    }
}

#[test]
fn everything_scales_with_the_zoom_together() {
    let theme = Theme::dark();
    for z in ZOOMS {
        assert_eq!(glyph_px(&theme, z), glyph_px(&theme, 1.0) * z);
        assert_eq!(chip_text_px(&theme, z), chip_text_px(&theme, 1.0) * z);
    }
}

#[test]
fn a_bigger_theme_carries_the_glyphs_with_it() {
    assert!(glyph_px(&Theme::high_contrast(), 1.0) > glyph_px(&Theme::dark(), 1.0));
    assert!(chip_text_px(&Theme::high_contrast(), 1.0) > chip_text_px(&Theme::dark(), 1.0));
}
