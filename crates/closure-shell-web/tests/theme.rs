//! G2: the web shell maps the typed `Theme` tokens to CSS custom
//! properties — one declarative source, the native styling layer.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::Theme;
use closure_shell_web::theme_css_variables;

#[test]
fn css_variables_carry_every_palette_and_layout_token() {
    let css = theme_css_variables(&Theme::dark());
    for var in [
        "--fg:",
        "--bg:",
        "--accent:",
        "--muted:",
        "--selection:",
        "--error:",
        "--warning:",
        "--success:",
        "--space:",
        "--gap:",
        "--font:",
        "--mono:",
        "--font-size:",
    ] {
        assert!(css.contains(var), "missing {var} in: {css}");
    }
    // Concrete dark values flow through.
    assert!(css.contains("#1e1e2e"), "dark bg present: {css}");
    assert!(css.contains("8px"), "spacing in px: {css}");
}

#[test]
fn different_themes_yield_different_css() {
    assert_ne!(
        theme_css_variables(&Theme::light()),
        theme_css_variables(&Theme::dark()),
    );
}
