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
    assert!(
        pair.contains(&(0, 0, 0)) && pair.contains(&(255, 255, 255)),
        "fg={fg:?} bg={bg:?} are max-contrast"
    );
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

#[test]
fn doom_vibrant_matches_the_doom_emacs_palette() {
    // The user's colorscheme: Doom Emacs doom-vibrant (gui values).
    let t = Theme::from_name("doom-vibrant");
    assert_eq!(t.name, "doom-vibrant");
    assert_eq!(t.color(ColorRole::Bg).hex(), "#242730");
    assert_eq!(t.color(ColorRole::Fg).hex(), "#bbc2cf");
    assert_eq!(t.color(ColorRole::Accent).hex(), "#51afef");
    assert_eq!(t.color(ColorRole::Muted).hex(), "#62686e");
    assert_eq!(t.color(ColorRole::Selection).hex(), "#3d4451");
    assert_eq!(t.color(ColorRole::Error).hex(), "#ff665c");
    assert_eq!(t.color(ColorRole::Warning).hex(), "#fcce7b");
    assert_eq!(t.color(ColorRole::Success).hex(), "#7bc275");
    assert_eq!(t.color(ColorRole::Heading2).hex(), "#c57bdb");
    assert_eq!(t.color(ColorRole::Heading3).hex(), "#a991f1");
    assert_eq!(t.color(ColorRole::Code).hex(), "#e69055");
    assert_eq!(Theme::from_name("vibrant").name, "doom-vibrant", "alias");
}

#[test]
fn every_theme_fills_the_new_roles() {
    for t in [
        Theme::dark(),
        Theme::light(),
        Theme::high_contrast(),
        Theme::doom_vibrant(),
    ] {
        for role in [ColorRole::Heading2, ColorRole::Heading3, ColorRole::Code] {
            assert_eq!(t.color(role).hex().len(), 7, "{}/{role:?}", t.name);
        }
    }
}

// === font stacks ===
//
// `mono_family` is a CSS-shaped stack because the web tier drops it
// straight into a `font-family` rule. Every native toolkit wants one
// family name plus an ordered fallback list instead, and the gpui shell
// handed the *whole string* to `font_family()` — so it asked for a font
// literally called "JetBrains Mono, ui-monospace, monospace", got
// nothing, and fell back to whatever the platform felt like. The split
// belongs here, where every shell can share it.

#[test]
fn a_font_stack_splits_into_family_then_fallbacks() {
    let stack = closure_shell_core::font_stack("Maple Mono NF, JetBrains Mono, monospace");
    assert_eq!(
        stack,
        vec!["Maple Mono NF", "JetBrains Mono", "monospace"],
        "in order, whitespace trimmed"
    );
}

#[test]
fn a_single_family_is_a_stack_of_one() {
    assert_eq!(
        closure_shell_core::font_stack("Maple Mono NF"),
        vec!["Maple Mono NF"]
    );
}

#[test]
fn an_empty_or_ragged_stack_yields_no_empty_names() {
    // A trailing comma or a double comma must not ask the toolkit for a
    // font with no name.
    assert!(closure_shell_core::font_stack("").is_empty());
    assert_eq!(
        closure_shell_core::font_stack("Maple Mono NF,, ,monospace,"),
        vec!["Maple Mono NF", "monospace"]
    );
}

#[test]
fn every_theme_leads_with_maple_mono_nerd_font() {
    // The user's font, and the one the editor is aligned for: a Nerd
    // Font, so the rail glyphs and the org markers have coverage, and
    // monospaced, so the gutter, the block cursor and the org tables
    // line up.
    for name in ["dark", "light", "high-contrast", "doom-vibrant"] {
        let t = Theme::from_name(name);
        assert_eq!(
            closure_shell_core::font_stack(t.typography.mono_family)
                .first()
                .copied(),
            Some("Maple Mono NF"),
            "{name} mono stack: {}",
            t.typography.mono_family
        );
        assert_eq!(
            closure_shell_core::font_stack(t.typography.font_family)
                .first()
                .copied(),
            Some("Maple Mono NF"),
            "{name} ui stack: {} — one font, whole app",
            t.typography.font_family
        );
    }
}

#[test]
fn every_stack_ends_in_a_generic_family() {
    // The name is what a machine without Maple Mono installed falls back
    // to; without it the shells would land on the toolkit's default,
    // which on some platforms is proportional.
    for name in ["dark", "light", "high-contrast", "doom-vibrant"] {
        let t = Theme::from_name(name);
        for stack in [t.typography.mono_family, t.typography.font_family] {
            let last = closure_shell_core::font_stack(stack).last().copied();
            assert_eq!(last, Some("monospace"), "{name}: {stack}");
        }
    }
}
