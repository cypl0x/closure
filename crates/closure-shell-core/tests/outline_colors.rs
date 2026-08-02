//! Outline colours: enough of them, and none of them shouting.
//!
//! Two reports, 2026-08-02. "In the body editor the property ID (and
//! maybe all properties) will [be] shown in red. Why use such a alert
//! color for something that is not so relevant for the user. Please use
//! the Doom Vibrant color palette." And: "please use multiple colors
//! for each level".
//!
//! A `:PROPERTIES:` drawer was painted in the error colour on the
//! theory that a drawer and an open TODO are both "unfinished business
//! the eye should catch first". That is true of a TODO and false of a
//! drawer: an id is bookkeeping, and bookkeeping belongs with the other
//! de-emphasised text.
//!
//! The heading cycle had three colours, so depth 4 read exactly like
//! depth 1. Doom's own outline faces go to eight — blue, magenta,
//! violet, then those lightened — and every one of them is bold. Five
//! distinct steps is where a reader stops being able to tell them apart
//! anyway, so the cycle is five.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ColorRole, Theme};

const fn vibrant() -> Theme {
    Theme::from_name("doom-vibrant")
}

#[test]
fn the_first_three_outline_colours_are_dooms_own() {
    // Verified against doom-themes-base.el on the user's machine:
    // outline-1 blue, outline-2 magenta, outline-3 violet.
    let t = vibrant();
    assert_eq!(t.color(ColorRole::Accent).0, "#51afef", "outline-1 blue");
    assert_eq!(
        t.color(ColorRole::Heading2).0,
        "#c57bdb",
        "outline-2 magenta"
    );
    assert_eq!(
        t.color(ColorRole::Heading3).0,
        "#a991f1",
        "outline-3 violet"
    );
}

#[test]
fn there_are_five_distinct_heading_colours() {
    let t = vibrant();
    let roles = [
        ColorRole::Accent,
        ColorRole::Heading2,
        ColorRole::Heading3,
        ColorRole::Heading4,
        ColorRole::Heading5,
    ];
    let mut seen: Vec<&str> = roles.iter().map(|r| t.color(*r).0).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 5, "five levels, five colours");
}

#[test]
fn the_deeper_two_are_lighter_relatives_of_the_first_two() {
    // doom-themes derives outline-4 and outline-5 by lightening blue and
    // magenta, which keeps the hue order readable as it repeats.
    let t = vibrant();
    let lum = |c: &str| {
        let v = u32::from_str_radix(c.trim_start_matches('#'), 16).expect("hex");
        (v >> 16) + ((v >> 8) & 0xff) + (v & 0xff)
    };
    assert!(
        lum(t.color(ColorRole::Heading4).0) > lum(t.color(ColorRole::Accent).0),
        "outline-4 is a lighter blue"
    );
    assert!(
        lum(t.color(ColorRole::Heading5).0) > lum(t.color(ColorRole::Heading2).0),
        "outline-5 is a lighter magenta"
    );
}

#[test]
fn every_theme_has_all_five() {
    // A role that resolves to nothing in one theme is a role a shell
    // cannot use.
    for name in ["dark", "light", "high-contrast", "doom-vibrant"] {
        let t = Theme::from_name(name);
        for role in [ColorRole::Heading4, ColorRole::Heading5] {
            let c = t.color(role).0;
            assert!(c.starts_with('#') && c.len() == 7, "{name} {role:?}: {c}");
        }
    }
}
