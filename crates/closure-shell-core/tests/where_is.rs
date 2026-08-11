//! `describe-command`: which keys reach a command, and what it does.
//!
//! Emacs calls it `where-is`, and it is the other half of
//! `describe-key`. One asks "what does this key do", the other "how do
//! I press this thing" — and the second is the question you have when
//! you have just found a command in the palette and would rather not
//! go through the palette next time.
//!
//! `describe_command` has existed, with tests, and nothing could reach
//! it: no chord, no palette entry. A function the program has and does
//! not offer is not a feature, and I4 is the invariant that says so —
//! every command carries its keybinding, which means being a command
//! in the first place.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn app(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01WHEREIS0000000000001A\n:END:\nbody\n",
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(mode))
}

#[test]
fn it_is_a_command_like_any_other() {
    // I4: in the registry, so the palette lists it and which-key shows
    // it, without either being told about it separately.
    assert!(
        closure_shell_core::palette_command_names().contains(&"describe-command"),
        "not in the palette"
    );
}

#[test]
fn every_mode_can_reach_it() {
    // The rail test's rule: a destination nobody can get to is not a
    // destination. Emacs spells it `C-h w`, Doom `SPC h w`, and the
    // modal maps hang it off `g`.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let bound = closure_input::mode_keymap(mode)
            .iter()
            .any(|(_, cmd)| *cmd == "describe-command");
        assert!(bound, "{mode:?} cannot reach describe-command");
    }
}

#[test]
fn running_it_opens_a_prompt_for_a_name() {
    let (_d, mut shell, mut app) = app(InputMode::Doom);
    app.run(&mut shell, "describe-command");
    assert_eq!(app.surface(), ModalSurface::DescribeCommand);
}

#[test]
fn it_answers_with_the_chords_that_reach_that_command() {
    let (_d, _shell, app) = app(InputMode::Doom);
    let told = app.describe_command("toggle-wrap").expect("a real command");
    assert!(told.chords.iter().any(|c| c == "g W"), "{:?}", told.chords);
    assert!(!told.description.is_empty());
}

#[test]
fn a_filter_that_matches_nothing_says_so_rather_than_nothing() {
    // Not "frobnicate": the filter is fuzzy, and that matches
    // `first-file` as a scattered subsequence — which is the picker
    // working, not failing. This is a string no command contains.
    let (_d, mut shell, mut app) = app(InputMode::Doom);
    app.run(&mut shell, "describe-command");
    for c in "zzqqzz".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.status().to_lowercase().contains("zzqqzz"),
        "silence is not an answer: {}",
        app.status()
    );
}

#[test]
fn escape_leaves_without_answering() {
    let (_d, mut shell, mut app) = app(InputMode::Doom);
    app.run(&mut shell, "describe-command");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}
