//! P5: the GTK window applies the shared `Theme` tokens. `theme_css`
//! maps a `Theme` to a GTK4 CSS string (window/label/selection/severity
//! colours) — hermetic; the windowed `run` loads it into a `CssProvider`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::Theme;
use closure_shell_gtk::theme_css;

#[test]
fn css_carries_the_palette_colours() {
    let css = theme_css(&Theme::dark());
    assert!(css.contains("#1e1e2e"), "dark background present: {css}");
    assert!(css.contains("#cdd6f4"), "dark foreground present: {css}");
    assert!(css.contains("window"), "styles the window: {css}");
    assert!(css.contains("background-color"), "sets a background: {css}");
}

#[test]
fn different_themes_yield_different_css() {
    assert_ne!(theme_css(&Theme::light()), theme_css(&Theme::dark()));
    // High-contrast pushes pure black/white.
    let hc = theme_css(&Theme::high_contrast());
    assert!(hc.contains("#000000") && hc.contains("#ffffff"), "max contrast: {hc}");
}
