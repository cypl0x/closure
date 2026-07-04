//! Hermetic tests for the gpui shell's pure helpers: theme resolution
//! from the vault config and colour mapping for the GPU renderer. The
//! window itself is display-bound (feature `gpui`, manual smoke).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_gpui::{color_u32, mix_u32, resolve_theme};

#[test]
fn theme_colors_map_to_packed_rgb() {
    let th = closure_shell_core::Theme::dark();
    let bg = color_u32(th.color(closure_shell_core::ColorRole::Bg));
    assert_eq!(bg, 0x001e_1e2e, "dark bg #1e1e2e packs to 0x1e1e2e");
}

#[test]
fn mix_blends_channelwise() {
    assert_eq!(mix_u32(0x00_0000, 0xff_ffff, 128), 0x80_8080);
    assert_eq!(mix_u32(0x10_2030, 0x10_2030, 77), 0x10_2030, "identity");
}

#[test]
fn resolve_theme_reads_the_vault_config() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\ntheme = light\n#+END_SRC\n",
    )
    .expect("write");
    assert_eq!(resolve_theme(dir.path()).name, "light");
    let empty = tempfile::tempdir().expect("tmp2");
    assert_eq!(resolve_theme(empty.path()).name, "dark", "absent config -> dark");
}

#[test]
fn resolve_input_mode_reads_the_vault_config() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\ninput_mode = vim\n#+END_SRC\n",
    )
    .expect("write");
    assert_eq!(
        closure_shell_gpui::resolve_input_mode(dir.path()),
        closure_config::InputMode::Vim
    );
    let empty = tempfile::tempdir().expect("tmp2");
    assert_eq!(
        closure_shell_gpui::resolve_input_mode(empty.path()),
        closure_config::InputMode::Doom,
        "absent config -> Doom"
    );
}
