//! Dispatcher tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_config::InputMode;
use closure_core::{KeyChord, Registry, RenameHeadline};
use closure_input::Dispatcher;

#[test]
fn dispatcher_resolves_registered_chord() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let chord: KeyChord = "C-c C-x r".parse().expect("parse");
    assert_eq!(disp.resolve(&chord), Some("rename-headline"));
}

#[test]
fn unbound_chord_returns_none() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    let chord: KeyChord = "C-x b".parse().expect("parse");
    assert_eq!(disp.resolve(&chord), None);
}

#[test]
fn bindings_returns_sorted_pairs() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let b = disp.bindings();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0], ("C-c C-x r", "rename-headline"));
}

#[test]
fn vim_chord_parses_leader_and_letters() {
    let chord = closure_input::parse_vim_chord("<leader>ff").unwrap();
    assert_eq!(chord.to_string(), "<leader> f f");
}

#[test]
fn vim_chord_parses_ctrl_compound() {
    let chord = closure_input::parse_vim_chord("<C-c><C-x>r").unwrap();
    assert_eq!(chord.to_string(), "<C-c> <C-x> r");
}

#[test]
fn vim_chord_unmatched_bracket_is_error() {
    let err = closure_input::parse_vim_chord("<C-c").unwrap_err();
    let _ = err;
}
