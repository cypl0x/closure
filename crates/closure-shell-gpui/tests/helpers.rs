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

// === Where this shell opens its pairing socket ===
//
// The reference shell is the one that actually dials and accepts, so
// it is the one that has to read the two address keys and hand them to
// the kernel-side `SyncApp` before anything binds.

#[test]
fn resolve_sync_addrs_reads_the_vault_config() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\n\
         sync_bind = 0.0.0.0:9999\n\
         sync_advertise = 100.101.102.103\n\
         #+END_SRC\n",
    )
    .expect("write");
    let (bind, advertise) = closure_shell_gpui::resolve_sync_addrs(dir.path());
    assert_eq!(bind.to_string(), "0.0.0.0:9999");
    assert_eq!(
        advertise.expect("set").to_string(),
        "100.101.102.103",
        "the operator's choice of reachable address"
    );
}

#[test]
fn resolve_sync_addrs_defaults_to_the_pairing_port_on_every_interface() {
    let empty = tempfile::tempdir().expect("tmp");
    let (bind, advertise) = closure_shell_gpui::resolve_sync_addrs(empty.path());
    assert_eq!(
        bind.to_string(),
        "0.0.0.0:7420",
        "absent config, a peer on the network can still reach us"
    );
    assert!(
        advertise.is_none(),
        "and which address it dials is detected, not guessed by config"
    );
}

// === Org headline syntax in the buffer ===
//
// The editor view opens a whole org file, so most of what is on screen
// is headlines — and every one of them rendered as prose: the stars,
// the TODO keyword, the priority cookie and the tags all in the
// paragraph colour. The classifier knew about blocks, drawers, tables
// and inline markup, and nothing about the one construct org is made
// of.

fn kinds(line: &str) -> Vec<(BodySpan, String)> {
    highlight_body(line).remove(0)
}

#[test]
fn a_headline_line_is_classified_by_its_level() {
    let spans = kinds("* Top");
    assert_eq!(
        spans,
        vec![(BodySpan::Headline(1), "* Top".to_owned())],
        "stars and title are one run at the level's colour"
    );
    assert_eq!(
        kinds("*** Deep"),
        vec![(BodySpan::Headline(3), "*** Deep".to_owned())]
    );
}

#[test]
fn a_todo_keyword_is_marked_apart_from_the_title() {
    assert_eq!(
        kinds("* TODO Ship it"),
        vec![
            (BodySpan::Headline(1), "* ".to_owned()),
            (BodySpan::Todo, "TODO".to_owned()),
            (BodySpan::Headline(1), " Ship it".to_owned()),
        ]
    );
}

#[test]
fn a_done_keyword_is_not_a_todo_keyword() {
    // Same construct, opposite meaning: a list where finished and
    // unfinished look alike is the one thing a TODO list must not do.
    assert_eq!(
        kinds("** DONE Ship it"),
        vec![
            (BodySpan::Headline(2), "** ".to_owned()),
            (BodySpan::Done, "DONE".to_owned()),
            (BodySpan::Headline(2), " Ship it".to_owned()),
        ]
    );
}

#[test]
fn a_priority_cookie_and_tags_get_their_own_spans() {
    assert_eq!(
        kinds("* TODO [#A] Ship it :work:urgent:"),
        vec![
            (BodySpan::Headline(1), "* ".to_owned()),
            (BodySpan::Todo, "TODO".to_owned()),
            (BodySpan::Headline(1), " ".to_owned()),
            (BodySpan::Priority, "[#A]".to_owned()),
            (BodySpan::Headline(1), " Ship it ".to_owned()),
            (BodySpan::Tags, ":work:urgent:".to_owned()),
        ]
    );
}

#[test]
fn bold_text_at_the_start_of_a_line_is_not_a_headline() {
    // `*bold*` and `* headline` differ by one space, and getting this
    // wrong would repaint half the prose in the outline colours.
    let spans = kinds("*bold* opening");
    assert!(
        !matches!(spans.first(), Some((BodySpan::Headline(_), _))),
        "{spans:?}"
    );
}

#[test]
fn an_indented_star_is_a_list_bullet_not_a_headline() {
    let spans = kinds("  * a list item");
    assert!(
        !matches!(spans.first(), Some((BodySpan::Headline(_), _))),
        "org headlines start at column zero: {spans:?}"
    );
}

#[test]
fn a_star_inside_a_block_stays_verbatim() {
    let lines = highlight_body("#+BEGIN_EXAMPLE\n* not a headline\n#+END_EXAMPLE");
    assert_eq!(
        lines[1],
        vec![(BodySpan::Example, "* not a headline".to_owned())],
        "block content is not org syntax"
    );
}

#[test]
fn highlighting_a_headline_preserves_every_byte() {
    // The painter reconstructs the line from its spans; a classifier
    // that drops or duplicates a byte silently corrupts what is shown.
    for line in [
        "* TODO [#A] Ship it :work:urgent:",
        "**** DONE  odd   spacing  :t:",
        "* Ünïcödé — täg :ümlaut:",
        "*",
        "* ",
    ] {
        let joined: String = kinds(line).into_iter().map(|(_, t)| t).collect();
        assert_eq!(joined, line, "round-trip of {line:?}");
    }
}

// === why the window would not open ===
//
// gpui picks its backend from the environment and then `unwrap`s the
// GPU context, so a machine with no Vulkan driver gets a panic and a
// backtrace through `blade_graphics` — which says nothing about what to
// install. This is the check that runs first and says it in words.

use closure_shell_gpui::{Preflight, gpui_preflight, vulkan_icd_dirs};

/// The refusal text, or a panic naming what came back instead.
fn refusal(p: Preflight) -> String {
    match p {
        Preflight::Refused(why) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_session_with_a_display_and_a_driver_is_fine() {
    let dir = tempfile::tempdir().expect("tmp");
    let icd = dir.path().join("icd.d");
    fs::create_dir_all(&icd).expect("mkdir");
    fs::write(icd.join("intel_icd.x86_64.json"), "{}").expect("write");
    assert_eq!(
        gpui_preflight(Some("wayland-1"), None, &[icd], None, None),
        Preflight::Ready
    );
}

#[test]
fn no_display_at_all_is_explained() {
    let why = refusal(gpui_preflight(None, None, &[], None, None));
    assert!(
        why.contains("DISPLAY") || why.contains("display"),
        "names what is missing: {why}"
    );
}

#[test]
fn a_display_without_a_vulkan_driver_is_explained() {
    // Fabian's `inari`: X11 was there, the GPU context was not.
    let dir = tempfile::tempdir().expect("tmp");
    let empty = dir.path().join("icd.d");
    fs::create_dir_all(&empty).expect("mkdir");
    let why = refusal(gpui_preflight(None, Some(":0"), &[empty], None, None));
    assert!(why.contains("Vulkan"), "{why}");
    assert!(
        why.contains("hardware.graphics.enable"),
        "and says the NixOS fix: {why}"
    );
    assert!(
        why.contains("lvp_icd") || why.contains("lavapipe"),
        "and the software fallback: {why}"
    );
    assert!(
        why.contains("nix develop"),
        "and where the fallback comes from: {why}"
    );
}

#[test]
fn an_explicit_icd_override_is_believed() {
    // `VK_ICD_FILENAMES` names the driver directly; a missing icd.d
    // directory is then irrelevant.
    assert_eq!(
        gpui_preflight(
            Some("wayland-1"),
            None,
            &[],
            Some("/nix/store/x/lvp_icd.json"),
            None
        ),
        Preflight::Ready
    );
}

#[test]
fn a_driverless_box_falls_back_to_the_bundled_rasteriser() {
    // A headless server (no GPU, no distro Vulkan package) with a
    // display: the dev shell ships lavapipe, so the window can still
    // open — slowly — instead of refusing.
    let dir = tempfile::tempdir().expect("tmp");
    let empty = dir.path().join("icd.d");
    fs::create_dir_all(&empty).expect("mkdir");
    let lvp = dir.path().join("lvp_icd.x86_64.json");
    fs::write(&lvp, "{}").expect("write");
    assert_eq!(
        gpui_preflight(
            None,
            Some(":0"),
            &[empty],
            None,
            Some(&lvp.to_string_lossy())
        ),
        Preflight::Software(lvp)
    );
}

#[test]
fn a_real_driver_beats_the_bundled_rasteriser() {
    // Software rendering is the fallback, never the choice: a machine
    // with a GPU must not be quietly dropped onto lavapipe.
    let dir = tempfile::tempdir().expect("tmp");
    let icd = dir.path().join("icd.d");
    fs::create_dir_all(&icd).expect("mkdir");
    fs::write(icd.join("radeon_icd.x86_64.json"), "{}").expect("write");
    let lvp = dir.path().join("lvp_icd.x86_64.json");
    fs::write(&lvp, "{}").expect("write");
    assert_eq!(
        gpui_preflight(None, Some(":0"), &[icd], None, Some(&lvp.to_string_lossy())),
        Preflight::Ready
    );
}

#[test]
fn a_software_icd_that_is_not_on_disk_is_not_believed() {
    // The env var can outlive the store path that set it. A manifest
    // that is not there would fail inside the loader with the same
    // opaque panic the preflight exists to prevent.
    let dir = tempfile::tempdir().expect("tmp");
    let empty = dir.path().join("icd.d");
    fs::create_dir_all(&empty).expect("mkdir");
    let why = refusal(gpui_preflight(
        None,
        Some(":0"),
        &[empty],
        None,
        Some("/nix/store/gone/lvp_icd.x86_64.json"),
    ));
    assert!(why.contains("Vulkan"), "{why}");
}

#[test]
fn the_icd_search_covers_more_than_nixos() {
    // A driver installed by a distro package lands in one of the
    // loader's own search paths; looking only where NixOS puts it
    // reports "no driver" on a machine that has one.
    let dirs = vulkan_icd_dirs();
    for want in [
        "/run/opengl-driver/share/vulkan/icd.d",
        "/usr/share/vulkan/icd.d",
        "/usr/local/share/vulkan/icd.d",
        "/etc/vulkan/icd.d",
    ] {
        assert!(
            dirs.iter().any(|d| d == std::path::Path::new(want)),
            "{want} is searched: {dirs:?}"
        );
    }
}

// === Long labels in a one-line bar ===

use closure_shell_gpui::elide;

#[test]
fn a_short_label_is_left_alone() {
    assert_eq!(elide("Project", 28), "Project");
    assert_eq!(elide("", 4), "");
}

#[test]
fn a_long_label_ends_in_an_ellipsis_of_the_right_length() {
    let out = elide("Compare current init.el with doom emacs default", 28);
    assert_eq!(out.chars().count(), 28, "fits the budget exactly: {out}");
    assert!(out.ends_with('…'), "{out}");
    assert!(out.starts_with("Compare current"), "{out}");
}

#[test]
fn eliding_counts_characters_not_bytes() {
    // A German capture line is the normal case, and cutting at a byte
    // offset inside a multi-byte character panics.
    let out = elide("Ünïcödé — täg für die Wohnung Kranenburg", 12);
    assert_eq!(out.chars().count(), 12, "{out}");
    assert!(out.ends_with('…'));
}

// === What closure kills belongs on the system clipboard ===
//
// Reported 2026-08-02: "sync with system clipboard (two way)".
//
// One direction already worked: `C-c` writes a selection out and `C-v`
// reads one in. The other did not — `d` in the outline, `dd`/`yy` in the
// editor and `C-k` in a prompt all filled closure's own kill ring and
// stopped there, so a subtree you had just cut could not be pasted into
// anything else on the desktop.
//
// The bridge is "did the ring change": whatever put something there,
// the top of it is what a paste elsewhere should produce.

#[test]
fn a_new_ring_top_is_what_gets_mirrored() {
    use closure_shell_gpui::ring_to_mirror;
    assert_eq!(
        ring_to_mirror(None, Some("cut text")),
        Some("cut text".to_owned()),
        "something was killed"
    );
}

#[test]
fn an_unchanged_ring_mirrors_nothing() {
    // The check runs after every keystroke, so it has to be quiet when
    // nothing happened — writing the clipboard on every key would fight
    // whatever else on the desktop owns it.
    use closure_shell_gpui::ring_to_mirror;
    assert_eq!(ring_to_mirror(Some("same"), Some("same")), None);
    assert_eq!(ring_to_mirror(None, None), None);
}

#[test]
fn a_ring_that_emptied_mirrors_nothing() {
    // Undo can take the last kill back off. That is not a reason to
    // blank the desktop's clipboard.
    use closure_shell_gpui::ring_to_mirror;
    assert_eq!(ring_to_mirror(Some("was there"), None), None);
}

#[test]
fn replacing_the_top_mirrors_the_new_one() {
    use closure_shell_gpui::ring_to_mirror;
    assert_eq!(
        ring_to_mirror(Some("first"), Some("second")),
        Some("second".to_owned())
    );
}

// === The header buttons name their chord ===
//
// Reported 2026-08-02: "capture palette (right) and the keybinding mode
// switcher should show the current keybinding of the mode."
//
// The three header buttons ran real commands and said nothing about how
// to run them from the keyboard — in a shell whose premise is that
// every element shows its binding, the three most-clicked things in the
// window were the exception. The chord comes from the active mode's
// keymap, so it is right in all five rather than a vim label everywhere.

#[test]
fn a_button_shows_the_chord_beside_its_name() {
    use closure_shell_gpui::header_label;
    assert_eq!(header_label("＋ capture", Some("c")), "＋ capture  c");
}

#[test]
fn a_command_this_mode_cannot_reach_shows_no_chord() {
    // Better a bare label than a chord that does nothing when pressed.
    use closure_shell_gpui::header_label;
    assert_eq!(header_label("❯ palette", None), "❯ palette");
}

#[test]
fn the_chords_are_the_active_modes_own() {
    // Doom captures with `c`, Emacs with `C-c c` — the label has to
    // follow the mode, which is the whole point of the report.
    use closure_config::InputMode;
    assert_eq!(
        closure_input::chord_for_command(InputMode::Doom, "capture"),
        Some("c")
    );
    assert_eq!(
        closure_input::chord_for_command(InputMode::Emacs, "capture"),
        Some("C-c c")
    );
}

// === The header says which vault is open ===
//
// Reported 2026-08-02: "show vault (path somewhere) in the top left."
// The window never said, and with more than one vault on a machine the
// only way to tell was to open a note and read the path under it.

#[test]
fn a_vault_under_home_is_shown_the_way_a_shell_prompt_does() {
    use closure_shell_gpui::vault_label;
    let home = std::path::Path::new("/home/someone");
    assert_eq!(
        vault_label(std::path::Path::new("/home/someone/vault"), Some(home)),
        "~/vault"
    );
}

#[test]
fn home_itself_is_just_the_tilde() {
    use closure_shell_gpui::vault_label;
    let home = std::path::Path::new("/home/someone");
    assert_eq!(vault_label(home, Some(home)), "~");
}

#[test]
fn a_vault_outside_home_keeps_its_whole_path() {
    // Shortening what is not under `$HOME` would be a lie about where
    // the file is.
    use closure_shell_gpui::vault_label;
    let home = std::path::Path::new("/home/someone");
    assert_eq!(
        vault_label(std::path::Path::new("/srv/shared/notes"), Some(home)),
        "/srv/shared/notes"
    );
}

#[test]
fn no_home_at_all_is_not_a_reason_to_hide_the_path() {
    use closure_shell_gpui::vault_label;
    assert_eq!(
        vault_label(std::path::Path::new("/home/someone/vault"), None),
        "/home/someone/vault"
    );
}
