//! "Bundle Maple NF font with closure gpui shell" — and the reason it
//! matters more than it sounds: `*bold*` was not bold.
//!
//! The emphasis machinery was all there and correct. `span_decoration`
//! asked for `FontWeight::BOLD` and `FontStyle::Italic`, the painter
//! put them in the `HighlightStyle`, and on screen `+struck+` came out
//! struck while `*bold*` came out exactly like the prose beside it.
//! The strikethrough is drawn by the renderer; the weight and the slant
//! have to be *found*, in a real font face, and the window was running
//! on whatever the platform substituted for a font nobody had
//! installed. A substitute with one face cannot be made bold.
//!
//! So the faces ship with the shell. Maple Mono NF is OFL-1.1, which
//! permits redistribution; the bytes come from nixpkgs at build time
//! rather than into git, because two megabytes a face times five is not
//! a thing to keep in a source tree — and a build without them falls
//! back to the system stack rather than failing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::Theme;
use closure_shell_gpui::{BUNDLED_FACES, bundled_fonts, font_family_name};

#[test]
fn the_shell_asks_for_the_font_it_bundles() {
    // A window that ships a font and then asks for a different family
    // has bundled nothing.
    let family = font_family_name(&Theme::dark());
    assert!(
        BUNDLED_FACES.is_empty() || family == "Maple Mono NF",
        "asks for {family}, bundles {BUNDLED_FACES:?}"
    );
}

#[test]
fn the_bundle_covers_the_four_faces_emphasis_needs() {
    // Regular is the prose, Bold is `*bold*` and every headline, Italic
    // is `/italic/`, and BoldItalic is a headline with emphasis in it —
    // which is what the item asking about `/emphasis/ *bold*` in a
    // header is asking for.
    if BUNDLED_FACES.is_empty() {
        return; // built outside the flake; the system stack answers
    }
    for face in ["Regular", "Bold", "Italic", "BoldItalic"] {
        assert!(
            BUNDLED_FACES.contains(&face),
            "no {face} face: {BUNDLED_FACES:?}"
        );
    }
}

#[test]
fn every_bundled_face_carries_bytes_that_are_a_font() {
    // An embed that silently produced an empty slice would look exactly
    // like a working bundle until the first glyph.
    for (name, bytes) in BUNDLED_FACES.iter().zip(bundled_fonts()) {
        assert!(bytes.len() > 100_000, "{name} is {} bytes", bytes.len());
        // TrueType's magic, which is what a `.ttf` starts with.
        assert_eq!(&bytes[..4], b"\x00\x01\x00\x00", "{name} is not a ttf");
    }
}

#[test]
fn the_face_list_and_the_bytes_are_the_same_length() {
    // They are two halves of one table; a mismatch would name one face
    // and load another.
    assert_eq!(BUNDLED_FACES.len(), bundled_fonts().len());
}

#[test]
fn a_build_without_the_fonts_still_names_a_family() {
    // The fallback is the old behaviour, not a crash: `app_font` has to
    // hand gpui something either way.
    let family = font_family_name(&Theme::dark());
    assert!(!family.is_empty());
}

#[test]
fn the_flake_build_actually_bundles_them() {
    // The tests above are all conditional on the bundle existing, which
    // would let a broken build script pass every one of them. Inside
    // the dev shell `CLOSURE_FONT_DIR` is set and the faces must be
    // there; outside it, this is the honest no-op the rest rely on.
    if std::env::var_os("CLOSURE_FONT_DIR").is_some() {
        assert_eq!(BUNDLED_FACES.len(), 5, "the flake sets the font dir");
        assert!(!bundled_fonts().is_empty());
    }
}
