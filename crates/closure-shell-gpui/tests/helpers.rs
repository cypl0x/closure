//! Hermetic tests for the gpui shell's pure helpers: theme resolution
//! from the vault config and colour mapping for the GPU renderer. The
//! window itself is display-bound (feature `gpui`, manual smoke).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_gpui::{char_cells, color_u32, mix_u32, resolve_theme};

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
    assert_eq!(
        resolve_theme(dir.path()).name,
        "light",
        "explicit name wins"
    );
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
    assert_eq!(
        lines[1],
        vec![(BodySpan::Meta, "#+BEGIN_SRC rust".to_owned())]
    );
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
    assert_eq!(
        joined.join("\n"),
        body,
        "spans cover every byte (I1 spirit)"
    );
}

// === L3: status line -> toast classification. ===

use closure_shell_core::ToastLevel;
use closure_shell_gpui::status_toast;

#[test]
fn failures_toast_as_errors() {
    assert_eq!(
        status_toast("save failed: boom"),
        Some((ToastLevel::Error, "save failed: boom".to_owned()))
    );
    assert_eq!(
        status_toast("undo failed: x"),
        Some((ToastLevel::Error, "undo failed: x".to_owned()))
    );
}

#[test]
fn destructive_and_positive_outcomes_toast() {
    assert_eq!(
        status_toast("deleted: Foo"),
        Some((ToastLevel::Warning, "deleted: Foo".to_owned()))
    );
    assert_eq!(
        status_toast("body saved"),
        Some((ToastLevel::Success, "body saved".to_owned()))
    );
    assert_eq!(
        status_toast("folded: Top"),
        Some((ToastLevel::Success, "folded: Top".to_owned()))
    );
    assert_eq!(
        status_toast("redo"),
        Some((ToastLevel::Success, "redo".to_owned()))
    );
}

#[test]
fn chatter_stays_quiet() {
    assert_eq!(status_toast(""), None);
    assert_eq!(status_toast("browse - type to filter"), None);
    assert_eq!(status_toast("rename - Enter save, Esc cancel"), None);
}

// === Q3-A2: pure UTC calendar date for the agenda pane. ===

#[test]
fn epoch_start_is_the_first_of_january_1970() {
    assert_eq!(closure_shell_gpui::today_ymd(0), "1970-01-01");
    assert_eq!(closure_shell_gpui::today_ymd(86_399), "1970-01-01");
    assert_eq!(closure_shell_gpui::today_ymd(86_400), "1970-01-02");
}

#[test]
fn known_dates_round_trip() {
    // date -u -d "2026-07-05 12:00:00" +%s
    assert_eq!(closure_shell_gpui::today_ymd(1_783_252_800), "2026-07-05");
    // Leap day: date -u -d "2024-02-29 00:00:00" +%s
    assert_eq!(closure_shell_gpui::today_ymd(1_709_164_800), "2024-02-29");
    // Century boundary second: date -u -d "2000-12-31 23:59:59" +%s
    assert_eq!(closure_shell_gpui::today_ymd(978_307_199), "2000-12-31");
}

#[test]
fn char_cells_one_cell_per_char() {
    assert_eq!(
        char_cells("ab c", 0),
        vec![
            ("a".to_owned(), 0),
            ("b".to_owned(), 1),
            (" ".to_owned(), 2),
            ("c".to_owned(), 3),
        ]
    );
}

#[test]
fn char_cells_columns_count_chars_not_bytes() {
    assert_eq!(
        char_cells("\u{e4}b", 5),
        vec![("\u{e4}".to_owned(), 5), ("b".to_owned(), 6)]
    );
}

#[test]
fn char_cells_empty_text_yields_no_cells() {
    assert_eq!(char_cells("", 3), vec![]);
}

#[test]
fn char_cells_keeps_whitespace_cells() {
    assert_eq!(
        char_cells("\ta", 0),
        vec![("\t".to_owned(), 0), ("a".to_owned(), 1)]
    );
}

