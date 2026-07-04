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
    assert_eq!(resolve_theme(dir.path()).name, "light", "explicit name wins");
    let empty = tempfile::tempdir().expect("tmp2");
    // Contract revised 2026-07-04: the reference shell defaults to the
    // user's doom-vibrant colorscheme, not generic dark.
    assert_eq!(resolve_theme(empty.path()).name, "doom-vibrant");
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

// === Body syntax highlighting (hermetic keyword tier; the tree-sitter
// grammars ride the same Highlighter trait behind the feature). ===

use closure_shell_gpui::{BodySpan, highlight_body};

#[test]
fn src_block_lines_get_keyword_highlights() {
    let body = "intro\n#+BEGIN_SRC rust\nlet x = \"s\";\n#+END_SRC\n";
    let lines = highlight_body(body);
    assert_eq!(lines.len(), 5, "one entry per line incl. trailing empty");
    assert_eq!(lines[0], vec![(BodySpan::Plain, "intro".to_owned())]);
    assert_eq!(lines[1], vec![(BodySpan::Meta, "#+BEGIN_SRC rust".to_owned())]);
    assert!(
        lines[2].contains(&(BodySpan::Keyword, "let".to_owned())),
        "rust `let` classified: {:?}",
        lines[2]
    );
    assert!(
        lines[2].contains(&(BodySpan::Literal, "\"s\"".to_owned())),
        "string literal classified: {:?}",
        lines[2]
    );
    assert_eq!(lines[3], vec![(BodySpan::Meta, "#+END_SRC".to_owned())]);
}

#[test]
fn drawer_and_meta_lines_are_classified() {
    let lines = highlight_body(":PROPERTIES:\n:ID: abc\n:END:\n#+TITLE: x");
    assert_eq!(lines[0][0].0, BodySpan::Drawer);
    assert_eq!(lines[1][0].0, BodySpan::Drawer);
    assert_eq!(lines[2][0].0, BodySpan::Drawer);
    assert_eq!(lines[3][0].0, BodySpan::Meta);
}

#[test]
fn highlight_roundtrips_the_text() {
    let body = "a\n#+BEGIN_SRC sh\necho \"hi\" # c\n#+END_SRC\ntail";
    let joined: Vec<String> = highlight_body(body)
        .iter()
        .map(|l| l.iter().map(|(_, s)| s.as_str()).collect::<String>())
        .collect();
    assert_eq!(joined.join("\n"), body, "spans cover every byte (I1 spirit)");
}
