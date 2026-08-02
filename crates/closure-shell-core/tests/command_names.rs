//! "Does org mode calls their function/command eval-block or something
//! like this? Polish and streamline all of the command names in order
//! to receive a top notch UI/UX and discoverability. Since these
//! commands will be exposed to the user. To some LLM via MCP they
//! should have sound names in a similiar schema."
//!
//! The schema, now that there is one: **verb first**, and a bare noun
//! opens the pane of that name. Ninety-two commands had grown three
//! shapes at once — `toggle-fold` beside `checkbox-toggle`,
//! `add-sibling` beside `buffer-next`, `block-list` beside
//! `move-subtree-up` — so guessing the name of a command you had not
//! used was a coin toss, which is the whole of discoverability.
//!
//! Every former name still resolves. A rename that breaks the chord you
//! typed yesterday, the `:` line in someone's muscle memory, or a tool
//! call an LLM already learned is a rename that costs more than the
//! tidiness it buys.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, canonical_command};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQNAME000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    app.select_by_id(&shell, "01HQNAME000000000000001");
    (dir, shell, app)
}

#[test]
fn a_toggle_is_spelled_the_same_way_everywhere() {
    // `toggle-fold`, `toggle-todo`, `toggle-tree`, `toggle-view`,
    // `toggle-wrap` — and then `checkbox-toggle`, backwards.
    assert_eq!(canonical_command("checkbox-toggle"), "toggle-checkbox");
}

#[test]
fn a_list_command_says_list_first() {
    for (was, now) in [
        ("block-list", "list-blocks"),
        ("headline-list", "list-headlines"),
        ("buffer-list", "list-buffers"),
    ] {
        assert_eq!(canonical_command(was), now, "{was}");
    }
}

#[test]
fn buffer_motion_reads_as_a_verb() {
    for (was, now) in [
        ("buffer-next", "next-buffer"),
        ("buffer-prev", "prev-buffer"),
        ("buffer-close", "close-buffer"),
        ("buffer-alternate", "alternate-buffer"),
    ] {
        assert_eq!(canonical_command(was), now, "{was}");
    }
}

#[test]
fn org_lends_its_own_word_where_closure_invented_one() {
    // org calls it `org-babel-execute-src-block`; nothing in org is
    // called `eval-block`. The answer to the question, as a test.
    assert_eq!(canonical_command("eval-block"), "execute-block");
}

#[test]
fn a_name_that_was_already_right_is_left_alone() {
    for name in [
        "toggle-fold",
        "add-sibling",
        "move-subtree-up",
        "refile",
        "schedule",
        "deadline",
        "clock-in",
        "edit-special",
        "promote",
        "demote",
    ] {
        assert_eq!(canonical_command(name), name, "{name} should not move");
    }
}

#[test]
fn every_former_name_still_runs() {
    // The rename must not cost anyone the chord they typed yesterday.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    assert_eq!(app.surface(), ModalSurface::Headlines, "the old name ran");
}

#[test]
fn the_new_name_runs_too() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "list-headlines");
    assert_eq!(app.surface(), ModalSurface::Headlines);
}

#[test]
fn the_keymaps_carry_the_new_names() {
    // closure-input is the source of truth, so it is the thing that has
    // to be readable: a chord listed against `checkbox-toggle` teaches
    // the wrong schema however well the alias works.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for (chord, cmd) in closure_input::mode_keymap(mode) {
            assert_eq!(
                canonical_command(cmd),
                *cmd,
                "{mode:?} binds {chord} to the old name {cmd}"
            );
        }
    }
}

#[test]
fn an_unknown_name_is_returned_unchanged() {
    assert_eq!(canonical_command("not-a-command"), "not-a-command");
}
