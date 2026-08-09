//! "Research the Doom Emacs org headline keybindings and adapt."
//!
//! Most of the headline layer was already here and carefully argued —
//! `M-RET`, `C-RET`, `C-M-RET`, `M-<left>`/`M-<right>` for
//! promote/demote, `M-<up>`/`M-<down>` for moving a subtree, `C-c C-t`
//! / `C-c C-s` / `C-c C-d`. What was missing is org's *shift* layer,
//! which is the half you use without thinking:
//!
//!   S-<left> / S-<right>   org-shiftleft/right   — cycle the keyword
//!   S-<up>   / S-<down>    org-shiftup/down      — cycle the priority
//!   M-S-<left> / M-S-<right>  promote/demote the subtree
//!
//! Each of these maps onto a command closure already has, which is the
//! only reason to bind them: a chord that reaches nothing is worse
//! than no chord, because it teaches you a gesture that fails.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;

fn command_for(mode: InputMode, chord: &str) -> Option<String> {
    closure_input::keymap_with(mode, &[])
        .into_iter()
        .find(|(c, _)| c == chord)
        .map(|(_, cmd)| cmd)
}

#[test]
fn shift_left_and_right_walk_the_todo_keyword() {
    for mode in [InputMode::Doom, InputMode::Emacs] {
        assert_eq!(
            command_for(mode, "S-<right>"),
            Some("toggle-todo".to_owned()),
            "{mode:?}: org-shiftright cycles the keyword forward"
        );
        assert_eq!(
            command_for(mode, "S-<left>"),
            Some("todo-back".to_owned()),
            "{mode:?}: org-shiftleft cycles it back"
        );
    }
}

#[test]
fn shift_up_and_down_walk_the_priority() {
    for mode in [InputMode::Doom, InputMode::Emacs] {
        assert_eq!(
            command_for(mode, "S-<up>"),
            Some("priority-up".to_owned()),
            "{mode:?}"
        );
        assert_eq!(
            command_for(mode, "S-<down>"),
            Some("priority-down".to_owned()),
            "{mode:?}"
        );
    }
}

#[test]
fn meta_shift_left_and_right_reach_promote_and_demote() {
    // org distinguishes promoting a headline from promoting its
    // subtree. closure's promote *is* the subtree one — it has been
    // since "promote / demote => unexpected and almost unfixable
    // creation of subtree" — so both chords reach the same command,
    // for the reason `C-RET` and `M-RET` already do.
    for mode in [InputMode::Doom, InputMode::Emacs] {
        assert_eq!(
            command_for(mode, "M-S-<left>"),
            Some("promote".to_owned()),
            "{mode:?}"
        );
        assert_eq!(
            command_for(mode, "M-S-<right>"),
            Some("demote".to_owned()),
            "{mode:?}"
        );
    }
}

#[test]
fn the_meta_arrows_reach_the_same_verbs_as_the_hjkl_layer() {
    // Doom had `M-h`/`M-l`/`M-k`/`M-j` from evil-org and not the arrow
    // spelling of them, which stock org has and which is what a hand
    // that came from Emacs reaches for. Both, now.
    for mode in [InputMode::Doom, InputMode::Emacs] {
        assert_eq!(
            command_for(mode, "M-<left>"),
            Some("promote".to_owned()),
            "{mode:?}"
        );
        assert_eq!(
            command_for(mode, "M-<right>"),
            Some("demote".to_owned()),
            "{mode:?}"
        );
        assert_eq!(
            command_for(mode, "M-<up>"),
            Some("move-subtree-up".to_owned()),
            "{mode:?}"
        );
        assert_eq!(
            command_for(mode, "M-<down>"),
            Some("move-subtree-down".to_owned()),
            "{mode:?}"
        );
    }
}

#[test]
fn the_chords_it_already_had_are_untouched() {
    assert_eq!(
        command_for(InputMode::Doom, "M-h"),
        Some("promote".to_owned())
    );
    assert_eq!(
        command_for(InputMode::Doom, "M-j"),
        Some("move-subtree-down".to_owned())
    );
    for mode in [InputMode::Doom, InputMode::Emacs] {
        assert_eq!(
            command_for(mode, "M-RET"),
            Some("add-heading".to_owned()),
            "{mode:?}"
        );
    }
}
