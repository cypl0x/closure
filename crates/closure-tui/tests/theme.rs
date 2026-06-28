//! G2: the TUI maps the typed `Theme` palette to ratatui `Color::Rgb`
//! values — the same declarative tokens the web shell renders as CSS,
//! here as terminal colours. Hermetic (no terminal).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{ColorRole, Theme};
use closure_tui::theme_color;
use ratatui::style::Color;

#[test]
fn palette_roles_map_to_rgb_colours() {
    let hc = Theme::high_contrast();
    assert_eq!(theme_color(&hc, ColorRole::Fg), Color::Rgb(255, 255, 255));
    assert_eq!(theme_color(&hc, ColorRole::Bg), Color::Rgb(0, 0, 0));
}

#[test]
fn dark_accent_maps_to_its_rgb() {
    // #89b4fa -> (137, 180, 250)
    assert_eq!(
        theme_color(&Theme::dark(), ColorRole::Accent),
        Color::Rgb(137, 180, 250)
    );
}
