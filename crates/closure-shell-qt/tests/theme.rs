//! P5: the Qt window applies the shared `Theme` tokens. `theme_qml` maps a
//! `Theme` to QML colour properties (window/text/accent) — hermetic; the
//! interactive window injects them into its host document.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::Theme;
use closure_shell_qt::theme_qml;

#[test]
fn qml_props_carry_the_palette_colours() {
    let q = theme_qml(&Theme::dark());
    assert!(q.contains("#1e1e2e"), "dark bg present: {q}");
    assert!(q.contains("#cdd6f4"), "dark fg present: {q}");
    assert!(q.contains("property color"), "exposes QML colour props: {q}");
}

#[test]
fn different_themes_yield_different_props() {
    assert_ne!(theme_qml(&Theme::light()), theme_qml(&Theme::dark()));
    let hc = theme_qml(&Theme::high_contrast());
    assert!(hc.contains("#000000") && hc.contains("#ffffff"), "max contrast: {hc}");
}
