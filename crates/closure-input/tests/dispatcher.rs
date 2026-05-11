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

#[test]
fn chord_trie_resolves_two_step_chord() {
    let mut t = closure_input::ChordTrie::build(&[("C-c C-x r", "rename-headline")]);
    assert!(matches!(t.step("C-c"), closure_input::TrieStep::Pending(_)));
    assert!(matches!(t.step("C-x"), closure_input::TrieStep::Pending(_)));
    let last = t.step("r");
    assert_eq!(
        last,
        closure_input::TrieStep::Resolved("rename-headline".into())
    );
}

#[test]
fn chord_trie_unbound_resets_cursor() {
    let mut t = closure_input::ChordTrie::build(&[("a b", "foo"), ("a c", "bar")]);
    let _ = t.step("a");
    assert_eq!(t.step("z"), closure_input::TrieStep::Unbound);
    // Trie is back at root → next "a" is again Pending.
    assert!(matches!(t.step("a"), closure_input::TrieStep::Pending(_)));
}

#[test]
fn parse_chord_routes_by_syntax() {
    let a = closure_input::parse_chord("C-c C-x r").unwrap();
    let b = closure_input::parse_chord("<C-c><C-x>r").unwrap();
    assert_eq!(a, KeyChord::from_strokes(&["C-c", "C-x", "r"]));
    assert_eq!(b, KeyChord::from_strokes(&["<C-c>", "<C-x>", "r"]));
}

#[test]
fn emacs_chord_parsed() {
    let chord = closure_input::parse_emacs_chord("C-c C-x r").unwrap();
    assert_eq!(chord, KeyChord::from_strokes(&["C-c", "C-x", "r"]));
}

#[test]
fn emacs_chord_empty_rejected() {
    let err = closure_input::parse_emacs_chord("   ").unwrap_err();
    let _ = err;
}

#[test]
fn dispatcher_binding_count_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.binding_count(), 1);
}

#[test]
fn dispatcher_is_bound_returns_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let chord: KeyChord = "C-c C-x r".parse().expect("parse");
    assert!(disp.is_bound(&chord));
    let other: KeyChord = "C-x b".parse().expect("parse");
    assert!(!disp.is_bound(&other));
}

#[test]
fn dispatcher_is_empty_true_when_registry_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert!(disp.is_empty());
}

#[test]
fn dispatcher_command_names_sorted_unique() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.command_names(), vec!["rename-headline"]);
}

#[test]
fn dispatcher_chords_for_command_returns_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let chords = disp.chords_for_command("rename-headline");
    assert_eq!(chords, vec!["C-c C-x r"]);
}

#[test]
fn dispatcher_chords_for_command_empty_for_unknown() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert!(disp.chords_for_command("none").is_empty());
}

#[test]
fn dispatcher_command_count_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.command_count(), 1);
}

#[test]
fn chord_trie_pending_lists_alternatives() {
    let mut t = closure_input::ChordTrie::build(&[("a b", "x"), ("a c", "y")]);
    let step = t.step("a");
    let closure_input::TrieStep::Pending(opts) = step else {
        panic!("expected Pending, got {step:?}");
    };
    assert_eq!(opts, vec!["b".to_owned(), "c".to_owned()]);
}
