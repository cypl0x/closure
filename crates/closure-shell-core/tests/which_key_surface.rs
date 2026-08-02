//! "which-key in editor doesn't work with many keys like g … Manually
//! triggering toggle-which-key just shows the initial which-key state
//! instead of the scoped to the g key chords", and "vim :q! and other
//! bindings are shown in the editor even if I am in Notion or Emacs
//! mode. Only and everywhere only show the keybindings that are
//! relevant to the corresponding mode. Hide irrelevant ones."
//!
//! One cause. `which_key_groups` was built from `mode_keymap` alone —
//! the *outline's* chords — so the panel showed `j:next-file` and
//! `c:capture` while you were inside a buffer where neither runs, and
//! the pending chord it filtered by was the outline's too, which is
//! empty in a buffer however many prefix keys the editor is holding.
//!
//! So which-key answers for the surface you are on: the editor's own
//! vocabulary in a buffer, the outline's in the outline, and in a mode
//! with no NORMAL neither `:q!` nor `dd` — because neither is reachable
//! from where the cursor is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn editing(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQWK0000000000000001\n:END:\nbody text\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(mode);
    assert!(app.select_by_id(&shell, "01HQWK0000000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

/// Every chord which-key would list, flattened.
fn chords(app: &ModalApp) -> Vec<String> {
    app.which_key_groups()
        .into_iter()
        .flat_map(|(_, rows)| rows.into_iter().map(|(chord, _)| chord))
        .collect()
}

#[test]
fn the_outline_lists_the_outline() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (_sh, app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    let listed = chords(&app);
    assert!(listed.iter().any(|c| c == "j"), "{listed:?}");
    assert!(listed.iter().any(|c| c == "c"), "{listed:?}");
    let _ = dir;
}

#[test]
fn a_buffer_does_not_list_the_outlines_chords() {
    // `j` is next-file in the outline and the letter j in a buffer.
    let (_d, _sh, app) = editing(InputMode::Doom);
    let listed = chords(&app);
    assert!(
        !listed.iter().any(|c| c == "j" || c == "c"),
        "outline chords in a buffer: {listed:?}"
    );
}

#[test]
fn a_buffer_lists_the_editors_own_vocabulary() {
    let (_d, _sh, app) = editing(InputMode::Doom);
    let listed = chords(&app);
    assert!(
        !listed.is_empty(),
        "which-key has nothing to say in a buffer"
    );
    assert!(
        listed.iter().any(|c| c.starts_with('g')),
        "no g-prefixed chords to scope to: {listed:?}"
    );
}

#[test]
fn a_mode_with_no_normal_is_not_shown_vim() {
    // "Only and everywhere only show the keybindings that are relevant
    // to the corresponding mode."
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, _sh, app) = editing(mode);
        let listed = chords(&app);
        assert!(
            !listed.iter().any(|c| c == "dd" || c == "yy" || c == ":q!"),
            "{mode:?} was offered vim: {listed:?}"
        );
        assert!(!listed.is_empty(), "{mode:?} was offered nothing at all");
    }
}

#[test]
fn a_pending_prefix_in_a_buffer_is_the_editors_own() {
    // The panel filters by this. It read the outline's pending, which
    // is empty in a buffer however many prefix keys the editor holds —
    // so `g` showed the whole map instead of the `g` map.
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    assert_eq!(app.which_key_pending(), "", "nothing pending yet");
    app.on_key(&mut shell, "g", false, false, Some('g'));
    assert_eq!(app.which_key_pending(), "g", "the editor is holding `g`");
}

#[test]
fn the_outline_still_reports_its_own_pending() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut shell, "g", false, false, Some('g'));
    assert_eq!(app.which_key_pending(), "g");
}

#[test]
fn the_buffers_affordances_name_chords_this_mode_can_run() {
    // The discard button said `:q!` in every mode, including the two
    // where `:` types a colon and the ex line cannot be opened from a
    // buffer at all.
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, _sh, app) = editing(mode);
        for (label, _, chord) in app.buffer_actions() {
            assert!(
                chord != Some(":q!"),
                "{mode:?} is told to press :q! for {label}"
            );
        }
    }
}

#[test]
fn a_modal_mode_keeps_its_ex_discard() {
    let (_d, _sh, app) = editing(InputMode::Doom);
    let discard = app
        .buffer_actions()
        .into_iter()
        .find(|(label, ..)| label.contains("discard"))
        .expect("a discard action");
    assert_eq!(discard.2, Some(":q!"), "vim's own spelling stays");
}

#[test]
fn every_action_says_what_it_runs() {
    let (_d, _sh, app) = editing(InputMode::Doom);
    let actions = app.buffer_actions();
    assert_eq!(actions.len(), 3, "save, save-and-close, discard");
    for (label, command, _) in actions {
        assert!(!label.is_empty());
        assert!(!command.is_empty(), "{label} runs nothing");
    }
}

#[test]
fn the_footer_strip_follows_the_surface_too() {
    // The bottom line offered `j:next-file` and `q:quit` while the
    // cursor was in a buffer — one of them a letter you were about to
    // type, neither of them a command that runs there.
    let (_d, _sh, app) = editing(InputMode::Doom);
    let hints = app.key_hints();
    assert!(!hints.contains("next-file"), "{hints}");
    assert!(!hints.contains("q:quit"), "{hints}");
    assert!(
        hints.contains("fold"),
        "the editor's own vocabulary: {hints}"
    );
}

#[test]
fn the_outlines_footer_is_unchanged() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (_sh, app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    assert!(app.key_hints().contains("next-file"));
    let _ = dir;
}
