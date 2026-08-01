//! Deleting a headline puts it on the kill ring, and `p` puts it back.
//!
//! Reported 2026-08-01: "deleted element should go to kill ring and be
//! pasted with p? At least in Doom mode." `delete` called
//! `remove_subtree`, which drops the text on the floor — the only way
//! back was undo, and undo is not a way to *move* something.
//!
//! `d` then `p` is how vim moves a line and how evil moves an org
//! subtree, and the store has had `cut`/`paste` over a kill ring the
//! whole time. The outline was simply not using them.
//!
//! `p` was `cycle-priority`, which keeps three other spellings
//! (`] p`, `[ p`, `SPC m p`); paste has exactly one that anybody's
//! hands know.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n\
                   :PROPERTIES:\n\
                   :ID: 01HQKILL000000000000001\n\
                   :END:\n\
                   alpha body\n\
                   * Beta\n\
                   :PROPERTIES:\n\
                   :ID: 01HQKILL000000000000002\n\
                   :END:\n";

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

fn titles(app: &ModalApp, sh: &Shell) -> Vec<String> {
    app.rows(sh).into_iter().map(|r| r.title).collect()
}

#[test]
fn delete_puts_the_subtree_on_the_kill_ring() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "delete");
    assert_eq!(titles(&app, &sh), ["Beta"], "it is gone from the outline");
    assert!(
        sh.ring_top().is_some_and(|t| t.contains("Alpha")),
        "and on the ring: {:?}",
        sh.ring_top()
    );
}

#[test]
fn paste_puts_it_back_after_the_selection() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "delete");
    app.select(0, &sh); // now on Beta
    app.run(&mut sh, "paste-subtree");
    assert_eq!(
        titles(&app, &sh),
        ["Beta", "Alpha"],
        "d then p is how you move a subtree"
    );
}

#[test]
fn the_body_survives_the_round_trip() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "delete");
    app.select(0, &sh);
    app.run(&mut sh, "paste-subtree");
    let at = app
        .rows(&sh)
        .iter()
        .position(|r| r.title == "Alpha")
        .expect("Alpha is back");
    app.select(at, &sh);
    assert!(
        app.detail(&sh).expect("detail").body.contains("alpha body"),
        "a cut that loses the body is not a cut"
    );
}

#[test]
fn pasting_an_empty_ring_says_so_and_changes_nothing() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "paste-subtree");
    assert_eq!(titles(&app, &sh), ["Alpha", "Beta"], "untouched");
    assert!(!app.status().is_empty(), "and it says why");
}

#[test]
fn p_is_paste_in_the_modal_modes() {
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        assert_eq!(
            closure_input::command_for(mode, "p"),
            Some("paste-subtree"),
            "{mode:?}"
        );
    }
}

#[test]
fn emacs_pastes_with_its_own_yank() {
    assert_eq!(
        closure_input::command_for(InputMode::Emacs, "C-y"),
        Some("paste-subtree")
    );
}

#[test]
fn priority_cycling_keeps_the_spellings_it_had() {
    // Taking `p` is only defensible because the command it displaced
    // has other chords a Doom user already knows.
    for chord in ["] p", "SPC m p"] {
        assert_eq!(
            closure_input::command_for(InputMode::Doom, chord),
            Some("priority-down"),
            "{chord}"
        );
    }
}

#[test]
fn p_pastes_through_the_keymap_not_just_the_command() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "d", false, false, Some('d'));
    app.select(0, &sh);
    app.on_key(&mut sh, "p", false, false, Some('p'));
    assert_eq!(titles(&app, &sh), ["Beta", "Alpha"]);
}
