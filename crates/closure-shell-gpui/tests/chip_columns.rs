//! "crowded and broken TOD-O and DON-E … If the zoom is too high the
//! TODO and DONE indicator will be 'umgebrochen'."
//!
//! The keyword sits in a fixed-width column so that the titles of a
//! mixed list start at one x — which is right, and was written in
//! *unzoomed* pixels. The text inside it scales with the zoom and the
//! column did not, so past about 1.4 the word no longer fitted and
//! wrapped: `TOD` over `O`, `DON` over `E`. The priority cookie added
//! beside it had the same defect the moment it was written, `[#` over
//! `A]`.
//!
//! A column that holds text is as wide as that text, so it is measured
//! from the text: the face is monospace, an advance is a known
//! fraction of the size, and the width is a character count times an
//! advance at the current zoom. Then it cannot be outgrown, because it
//! is derived from the thing that grows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    missing_docs
)]

use closure_shell_gpui::{CHIP_TEXT, chip_col_px, scaled_text_px};

/// Zooms a person actually uses, and then some.
const ZOOMS: [f32; 7] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

/// Roughly how wide one monospace glyph is at `size`.
fn advance(size: f32) -> f32 {
    size * 0.62
}

#[test]
fn a_column_is_wide_enough_for_its_own_text_at_every_zoom() {
    // The bug, as an inequality. `CANCELLED` is the longest keyword the
    // kernel ships, and `[#A]` is four characters and never more.
    for zoom in ZOOMS {
        let text = scaled_text_px(CHIP_TEXT, zoom);
        for (chars, what) in [(9.0, "CANCELLED"), (4.0, "[#A]")] {
            assert!(
                chip_col_px(chars, zoom) >= advance(text) * chars,
                "{what} at zoom {zoom}: column {} vs text {}",
                chip_col_px(chars, zoom),
                advance(text) * chars
            );
        }
    }
}

#[test]
fn the_column_grows_with_the_zoom() {
    // The whole defect: it did not.
    let mut last = 0.0;
    for zoom in ZOOMS {
        let w = chip_col_px(4.0, zoom);
        assert!(w > last, "zoom {zoom} did not widen the column");
        last = w;
    }
}

#[test]
fn it_grows_in_step_with_the_text_rather_than_faster() {
    // A column that outran its text would push every title to the right
    // for nothing — the "crowded" half of the complaint.
    let one = chip_col_px(4.0, 1.0);
    let two = chip_col_px(4.0, 2.0);
    assert_eq!(two, one * 2.0, "{one} → {two}");
}

#[test]
fn a_longer_word_gets_a_wider_column() {
    assert!(chip_col_px(9.0, 1.0) > chip_col_px(4.0, 1.0));
}

#[test]
fn the_columns_stay_modest_at_ordinary_zoom() {
    // The other half of "crowded": the fixed 44px was generous for
    // `TODO`, and a derived width must not be more generous still or
    // every title moves right.
    assert!(
        chip_col_px(4.0, 1.0) <= 44.0,
        "{} px for four characters",
        chip_col_px(4.0, 1.0)
    );
}

#[test]
fn the_keyword_column_holds_the_longest_keyword_a_vault_declares() {
    // `CANCELLED` is nine characters and org's `#+TODO:` line is how a
    // vault asks for it. A column sized for `TODO` clipped it to
    // `CANCEL` — visible on screen, and the first version of this fix
    // shipped with a comment claiming six characters covered it.
    for zoom in ZOOMS {
        let text = scaled_text_px(CHIP_TEXT, zoom);
        assert!(
            chip_col_px(10.0, zoom) >= advance(text) * 9.0,
            "CANCELLED at zoom {zoom}"
        );
    }
}
