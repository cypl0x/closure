//! G2: declarative typed theme tokens. A `Theme` is palette + spacing +
//! typography as data, resolved from the free-form `config.theme` string
//! to one of three built-ins (dark / light / high-contrast). Resolution
//! and the token values are hermetic; each shell maps the tokens to its
//! native style (web CSS variables, tui ratatui colours).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{ColorRole, Theme};

#[test]
fn theme_resolves_from_the_config_string_case_insensitively() {
    assert_eq!(Theme::from_name("light").name, "light");
    assert_eq!(Theme::from_name("LIGHT").name, "light");
    assert_eq!(Theme::from_name("high-contrast").name, "high-contrast");
    assert_eq!(Theme::from_name("hc").name, "high-contrast");
    assert_eq!(Theme::from_name("dark").name, "dark");
    // Anything unrecognised (incl. the "default") falls back to dark.
    assert_eq!(Theme::from_name("default").name, "dark");
    assert_eq!(Theme::from_name("nonsense").name, "dark");
}

#[test]
fn light_and_dark_have_distinct_backgrounds() {
    assert_ne!(
        Theme::light().palette.bg.hex(),
        Theme::dark().palette.bg.hex(),
        "light and dark are visually different"
    );
}

#[test]
fn high_contrast_is_pure_black_on_white_or_inverse() {
    let hc = Theme::high_contrast();
    let fg = hc.palette.fg.rgb();
    let bg = hc.palette.bg.rgb();
    // Maximum luminance separation: one end is pure black, the other pure
    // white (either polarity).
    let pair = [fg, bg];
    assert!(pair.contains(&(0, 0, 0)) && pair.contains(&(255, 255, 255)),
        "fg={fg:?} bg={bg:?} are max-contrast");
}

#[test]
fn colors_parse_their_hex_to_rgb() {
    // The built-in hex strings round-trip to bytes (no panic, I5).
    let (r, g, b) = Theme::dark().palette.bg.rgb();
    assert_eq!(Theme::dark().palette.bg.hex().len(), 7, "#rrggbb");
    let _ = (r, g, b);
    // A malformed colour never panics — it resolves to black.
    assert_eq!(
        closure_shell_core::Color("#zzzzzz").rgb(),
        (0, 0, 0),
        "bad hex is black, not a panic"
    );
}

#[test]
fn theme_exposes_every_color_role() {
    let t = Theme::dark();
    for role in [
        ColorRole::Fg,
        ColorRole::Bg,
        ColorRole::Accent,
        ColorRole::Muted,
        ColorRole::Selection,
        ColorRole::Error,
        ColorRole::Warning,
        ColorRole::Success,
    ] {
        assert_eq!(t.color(role).hex().len(), 7, "{role:?} is a #rrggbb");
    }
}

#[test]
fn spacing_and_typography_are_positive() {
    let t = Theme::dark();
    assert!(t.spacing.unit_px > 0 && t.spacing.gap_px > 0);
    assert!(t.typography.base_px > 0);
    assert!(!t.typography.font_family.is_empty());
    assert!(!t.typography.mono_family.is_empty());
}
