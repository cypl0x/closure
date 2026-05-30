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
fn chord_trie_is_empty_match() {
    let t = closure_input::ChordTrie::build(&[]);
    assert!(t.is_empty());
}

#[test]
fn chord_trie_is_empty_false_when_bound() {
    let t = closure_input::ChordTrie::build(&[("a b", "foo")]);
    assert!(!t.is_empty());
}

#[test]
fn chord_trie_command_count_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "foo"), ("a c", "bar")]);
    assert_eq!(t.command_count(), 2);
}

#[test]
fn chord_trie_is_at_root_after_unbound() {
    let mut t = closure_input::ChordTrie::build(&[("a b", "x")]);
    assert!(t.is_at_root());
    let _ = t.step("a");
    assert!(!t.is_at_root());
    let _ = t.step("z");
    assert!(t.is_at_root());
}

#[test]
fn chord_trie_all_commands_sorted_unique() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "second"),
        ("a c", "first"),
        ("d e", "first"),
    ]);
    assert_eq!(t.all_commands(), vec!["first", "second"]);
}

#[test]
fn chord_trie_all_chords_sorted() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("a c", "y")]);
    let chords = t.all_chords();
    assert!(chords.contains(&"a b".to_owned()));
    assert!(chords.contains(&"a c".to_owned()));
}

#[test]
fn chord_trie_max_depth_match() {
    let t = closure_input::ChordTrie::build(&[("a b c", "x"), ("d e", "y")]);
    assert_eq!(t.max_depth(), 3);
}

#[test]
fn chord_trie_max_depth_zero_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.max_depth(), 0);
}

#[test]
fn chord_trie_contains_command_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "foo")]);
    assert!(t.contains_command("foo"));
    assert!(!t.contains_command("bar"));
}

#[test]
fn chord_trie_chord_count_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "x"),
        ("a c", "y"),
        ("d e", "y"),
    ]);
    assert_eq!(t.chord_count(), 3);
}

#[test]
fn chord_trie_node_count_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x")]);
    // root + 'a' + 'b' = 3
    assert_eq!(t.node_count(), 3);
}

#[test]
fn chord_trie_bindings_returns_sorted_pairs() {
    let t = closure_input::ChordTrie::build(&[("a b", "y"), ("a c", "x")]);
    let pairs = t.bindings();
    assert_eq!(pairs, vec![("a b".to_owned(), "y".to_owned()), ("a c".to_owned(), "x".to_owned())]);
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

#[test]
fn chord_trie_min_depth_match() {
    let t = closure_input::ChordTrie::build(&[("a b c", "x"), ("d e", "y")]);
    assert_eq!(t.min_depth(), 2);
}

#[test]
fn chord_trie_min_depth_zero_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.min_depth(), 0);
}

#[test]
fn chord_trie_chord_depth_counts_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "x"),
        ("c d", "y"),
        ("e f g", "z"),
    ]);
    let m = t.chord_depth_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn chord_trie_mean_depth_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d e f", "y")]);
    // depths 2,4 -> mean 3
    assert_eq!(t.mean_depth(), 3);
}

#[test]
fn chord_trie_mean_depth_zero_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mean_depth(), 0);
}

#[test]
fn chord_trie_mode_depth_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "x"),
        ("c d", "y"),
        ("e f g", "z"),
    ]);
    // depths 2,2,3 -> mode 2
    assert_eq!(t.mode_depth(), Some(2));
}

#[test]
fn chord_trie_mode_depth_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mode_depth(), None);
}

#[test]
fn chord_trie_total_depth_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y"), ("a b c", "z")]);
    // depths 1, 2, 3 -> total 6
    assert_eq!(t.total_depth(), 6);
}

#[test]
fn chord_trie_total_depth_zero_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.total_depth(), 0);
}

#[test]
fn chord_trie_median_depth_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y"), ("a b c", "z")]);
    // 1,2,3 -> median 2
    assert_eq!(t.median_depth(), Some(2));
}

#[test]
fn chord_trie_median_depth_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.median_depth(), None);
}

#[test]
fn dispatcher_max_min_chord_strokes_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single binding "C-c C-x r" = 3 strokes
    assert_eq!(disp.max_chord_strokes(), Some(3));
    assert_eq!(disp.min_chord_strokes(), Some(3));
}

#[test]
fn dispatcher_chord_strokes_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert_eq!(disp.max_chord_strokes(), None);
    assert_eq!(disp.min_chord_strokes(), None);
    assert_eq!(disp.mean_chord_strokes(), 0);
}

#[test]
fn dispatcher_mean_chord_strokes_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mean_chord_strokes(), 3);
}

#[test]
fn dispatcher_chord_stroke_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.chord_stroke_counts();
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn dispatcher_mode_chord_strokes_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_chord_strokes(), Some(3));
}

#[test]
fn dispatcher_mode_chord_strokes_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert_eq!(disp.mode_chord_strokes(), None);
}

#[test]
fn dispatcher_total_chord_strokes_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single binding "C-c C-x r" = 3 strokes
    assert_eq!(disp.total_chord_strokes(), 3);
}

#[test]
fn dispatcher_total_chord_strokes_zero_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.total_chord_strokes(), 0);
}

#[test]
fn dispatcher_median_chord_strokes_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_chord_strokes(), Some(3));
}

#[test]
fn dispatcher_median_chord_strokes_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_chord_strokes(), None);
}

#[test]
fn dispatcher_max_min_chord_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // "C-c C-x r" = 9 bytes
    assert_eq!(disp.max_chord_byte_len(), Some(9));
    assert_eq!(disp.min_chord_byte_len(), Some(9));
}

#[test]
fn dispatcher_total_chord_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.total_chord_byte_len(), 9);
}

#[test]
fn dispatcher_mean_chord_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mean_chord_byte_len(), 9);
}

#[test]
fn dispatcher_chord_byte_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.max_chord_byte_len(), None);
    assert_eq!(disp.min_chord_byte_len(), None);
    assert_eq!(disp.mean_chord_byte_len(), 0);
    assert_eq!(disp.total_chord_byte_len(), 0);
}

#[test]
fn dispatcher_median_chord_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single binding "C-c C-x r" = 9
    assert_eq!(disp.median_chord_byte_len(), Some(9));
}

#[test]
fn dispatcher_chord_byte_len_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.chord_byte_len_counts();
    assert_eq!(m.get(&9), Some(&1));
}

#[test]
fn dispatcher_mode_chord_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_chord_byte_len(), Some(9));
}

#[test]
fn dispatcher_median_chord_byte_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_chord_byte_len(), None);
    assert_eq!(disp.mode_chord_byte_len(), None);
}

#[test]
fn dispatcher_max_min_chord_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // "C-c C-x r" = 9 chars (ASCII)
    assert_eq!(disp.max_chord_char_len(), Some(9));
    assert_eq!(disp.min_chord_char_len(), Some(9));
}

#[test]
fn dispatcher_total_chord_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.total_chord_char_len(), 9);
}

#[test]
fn dispatcher_mean_chord_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mean_chord_char_len(), 9);
}

#[test]
fn dispatcher_chord_char_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.max_chord_char_len(), None);
    assert_eq!(disp.min_chord_char_len(), None);
    assert_eq!(disp.total_chord_char_len(), 0);
    assert_eq!(disp.mean_chord_char_len(), 0);
}

#[test]
fn dispatcher_median_chord_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_chord_char_len(), Some(9));
}

#[test]
fn dispatcher_chord_char_len_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.chord_char_len_counts();
    assert_eq!(m.get(&9), Some(&1));
}

#[test]
fn dispatcher_mode_chord_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_chord_char_len(), Some(9));
}

#[test]
fn dispatcher_median_chord_char_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_chord_char_len(), None);
    assert_eq!(disp.mode_chord_char_len(), None);
}

#[test]
fn dispatcher_has_command_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert!(disp.has_command("rename-headline"));
    assert!(!disp.has_command("nope"));
}

#[test]
fn dispatcher_longest_shortest_chord_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single binding "C-c C-x r"
    assert_eq!(disp.longest_chord(), Some("C-c C-x r".to_owned()));
    assert_eq!(disp.shortest_chord(), Some("C-c C-x r".to_owned()));
}

#[test]
fn dispatcher_longest_shortest_chord_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert_eq!(disp.longest_chord(), None);
    assert_eq!(disp.shortest_chord(), None);
}

#[test]
fn dispatcher_command_chord_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.command_chord_counts();
    assert_eq!(m.get("rename-headline"), Some(&1));
}

#[test]
fn dispatcher_most_bound_command_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.most_bound_command(), Some("rename-headline".to_owned()));
}

#[test]
fn dispatcher_most_bound_command_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert_eq!(disp.most_bound_command(), None);
}

#[test]
fn is_vim_syntax_detects_brackets() {
    assert!(closure_input::is_vim_syntax("<C-c><C-x>r"));
    assert!(!closure_input::is_vim_syntax("C-c C-x r"));
}

#[test]
fn is_emacs_syntax_detects_no_brackets() {
    assert!(closure_input::is_emacs_syntax("C-c C-x r"));
    assert!(!closure_input::is_emacs_syntax("<C-c>"));
}

#[test]
fn is_valid_chord_true_for_emacs_syntax() {
    assert!(closure_input::is_valid_chord("C-c C-x r"));
}

#[test]
fn is_valid_chord_true_for_vim_syntax() {
    assert!(closure_input::is_valid_chord("<C-c><C-x>r"));
}

#[test]
fn is_valid_chord_false_for_garbage() {
    assert!(!closure_input::is_valid_chord("<C-c"));
    assert!(!closure_input::is_valid_chord("   "));
}

#[test]
fn dispatcher_single_multi_stroke_count_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single binding "C-c C-x r" = 3 strokes -> multi
    assert_eq!(disp.single_stroke_count(), 0);
    assert_eq!(disp.multi_stroke_count(), 1);
}

#[test]
fn dispatcher_stroke_count_zero_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Emacs);
    assert_eq!(disp.single_stroke_count(), 0);
    assert_eq!(disp.multi_stroke_count(), 0);
}

#[test]
fn chord_trie_prefix_node_count_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x")]);
    // nodes: root, 'a', 'b'(cmd) -> prefix (no command) = root + 'a' = 2
    assert_eq!(t.prefix_node_count(), 2);
}

#[test]
fn chord_trie_prefix_node_count_empty_is_one() {
    let t = closure_input::ChordTrie::build(&[]);
    // only root, no command -> 1
    assert_eq!(t.prefix_node_count(), 1);
}

#[test]
fn chord_trie_single_stroke_count_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b c", "y"), ("d", "z")]);
    // single-stroke: "a","d" = 2
    assert_eq!(t.single_stroke_count(), 2);
}

#[test]
fn chord_trie_multi_stroke_count_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b c", "y"), ("d e f", "z")]);
    // multi-stroke: "b c","d e f" = 2
    assert_eq!(t.multi_stroke_count(), 2);
}

#[test]
fn chord_trie_single_stroke_count_zero_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.single_stroke_count(), 0);
    assert_eq!(t.multi_stroke_count(), 0);
}

#[test]
fn chord_trie_command_chord_counts_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "foo"),
        ("a c", "foo"),
        ("d e", "bar"),
    ]);
    let m = t.command_chord_counts();
    assert_eq!(m.get("foo"), Some(&2));
    assert_eq!(m.get("bar"), Some(&1));
}

#[test]
fn chord_trie_most_bound_command_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "foo"),
        ("a c", "foo"),
        ("d e", "bar"),
    ]);
    assert_eq!(t.most_bound_command(), Some("foo".to_owned()));
}

#[test]
fn chord_trie_most_bound_command_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.most_bound_command(), None);
}

#[test]
fn chord_trie_longest_chord_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d e", "y")]);
    assert_eq!(t.longest_chord(), Some("c d e".to_owned()));
}

#[test]
fn chord_trie_shortest_chord_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d e", "y")]);
    assert_eq!(t.shortest_chord(), Some("a b".to_owned()));
}

#[test]
fn chord_trie_longest_shortest_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.longest_chord(), None);
    assert_eq!(t.shortest_chord(), None);
}

#[test]
fn chord_trie_longest_chord_tie_first_sorted() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d", "y")]);
    // both 2 strokes -> sorted-first "a b"
    assert_eq!(t.longest_chord(), Some("a b".to_owned()));
}

#[test]
fn chord_trie_commands_at_depth_match() {
    let t = closure_input::ChordTrie::build(&[
        ("a b", "first"),
        ("c d", "second"),
        ("e f g", "third"),
    ]);
    assert_eq!(t.commands_at_depth(2), vec!["first", "second"]);
    assert_eq!(t.commands_at_depth(3), vec!["third"]);
    assert!(t.commands_at_depth(9).is_empty());
}

#[test]
fn chord_trie_max_min_chord_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    // chords: "a" (1), "a b c" (5)
    assert_eq!(t.max_chord_byte_len(), Some(5));
    assert_eq!(t.min_chord_byte_len(), Some(1));
}

#[test]
fn chord_trie_total_chord_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y")]);
    // "a" (1) + "a b" (3) = 4
    assert_eq!(t.total_chord_byte_len(), 4);
}

#[test]
fn chord_trie_mean_chord_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    // (1 + 5) / 2 = 3
    assert_eq!(t.mean_chord_byte_len(), 3);
}

#[test]
fn chord_trie_chord_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.max_chord_byte_len(), None);
    assert_eq!(t.min_chord_byte_len(), None);
    assert_eq!(t.total_chord_byte_len(), 0);
    assert_eq!(t.mean_chord_byte_len(), 0);
}

#[test]
fn chord_trie_median_chord_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y"), ("a b c", "z")]);
    // lens sorted: [1, 3, 5] -> median 3
    assert_eq!(t.median_chord_byte_len(), Some(3));
}

#[test]
fn chord_trie_median_chord_byte_len_even() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    // lens sorted: [1, 5] -> midpoint 3
    assert_eq!(t.median_chord_byte_len(), Some(3));
}

#[test]
fn chord_trie_median_chord_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.median_chord_byte_len(), None);
}

#[test]
fn chord_trie_mode_chord_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d", "y"), ("e f g", "z")]);
    // lens: 3, 3, 5 -> mode 3
    assert_eq!(t.mode_chord_byte_len(), Some(3));
}

#[test]
fn chord_trie_mode_chord_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mode_chord_byte_len(), None);
}

#[test]
fn chord_trie_chord_byte_len_counts_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d", "y"), ("e f g", "z")]);
    let counts = t.chord_byte_len_counts();
    assert_eq!(counts.get(&3), Some(&2));
    assert_eq!(counts.get(&5), Some(&1));
}

#[test]
fn chord_trie_max_min_chord_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    // "a" (1 char), "a b c" (5 chars)
    assert_eq!(t.max_chord_char_len(), Some(5));
    assert_eq!(t.min_chord_char_len(), Some(1));
}

#[test]
fn chord_trie_total_chord_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y")]);
    // 1 + 3 = 4
    assert_eq!(t.total_chord_char_len(), 4);
}

#[test]
fn chord_trie_mean_chord_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    // (1 + 5) / 2 = 3
    assert_eq!(t.mean_chord_char_len(), 3);
}

#[test]
fn chord_trie_chord_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.max_chord_char_len(), None);
    assert_eq!(t.min_chord_char_len(), None);
    assert_eq!(t.total_chord_char_len(), 0);
    assert_eq!(t.mean_chord_char_len(), 0);
}

#[test]
fn chord_trie_median_chord_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b", "y"), ("a b c", "z")]);
    // lens sorted: [1, 3, 5] -> median 3
    assert_eq!(t.median_chord_char_len(), Some(3));
}

#[test]
fn chord_trie_median_chord_char_len_even() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("a b c", "y")]);
    assert_eq!(t.median_chord_char_len(), Some(3));
}

#[test]
fn chord_trie_median_chord_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.median_chord_char_len(), None);
}

#[test]
fn chord_trie_mode_chord_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d", "y"), ("e f g", "z")]);
    // lens: 3, 3, 5 -> mode 3
    assert_eq!(t.mode_chord_char_len(), Some(3));
}

#[test]
fn chord_trie_mode_chord_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mode_chord_char_len(), None);
}

#[test]
fn chord_trie_chord_char_len_counts_match() {
    let t = closure_input::ChordTrie::build(&[("a b", "x"), ("c d", "y"), ("e f g", "z")]);
    let counts = t.chord_char_len_counts();
    assert_eq!(counts.get(&3), Some(&2));
    assert_eq!(counts.get(&5), Some(&1));
}

#[test]
fn dispatcher_max_min_command_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single command "rename-headline" = 15 bytes
    assert_eq!(disp.max_command_byte_len(), Some(15));
    assert_eq!(disp.min_command_byte_len(), Some(15));
}

#[test]
fn dispatcher_total_command_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.total_command_byte_len(), 15);
}

#[test]
fn dispatcher_mean_command_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mean_command_byte_len(), 15);
}

#[test]
fn dispatcher_command_byte_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.max_command_byte_len(), None);
    assert_eq!(disp.min_command_byte_len(), None);
    assert_eq!(disp.total_command_byte_len(), 0);
    assert_eq!(disp.mean_command_byte_len(), 0);
}

#[test]
fn dispatcher_median_command_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // single command "rename-headline" -> 15
    assert_eq!(disp.median_command_byte_len(), Some(15));
}

#[test]
fn dispatcher_median_command_byte_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_command_byte_len(), None);
}

#[test]
fn dispatcher_mode_command_byte_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_command_byte_len(), Some(15));
}

#[test]
fn dispatcher_mode_command_byte_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_command_byte_len(), None);
}

#[test]
fn dispatcher_command_byte_len_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.command_byte_len_counts();
    assert_eq!(m.get(&15), Some(&1));
}

#[test]
fn dispatcher_max_min_command_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    // "rename-headline" = 15 chars
    assert_eq!(disp.max_command_char_len(), Some(15));
    assert_eq!(disp.min_command_char_len(), Some(15));
}

#[test]
fn dispatcher_total_command_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.total_command_char_len(), 15);
}

#[test]
fn dispatcher_mean_command_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mean_command_char_len(), 15);
}

#[test]
fn dispatcher_command_char_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.max_command_char_len(), None);
    assert_eq!(disp.min_command_char_len(), None);
    assert_eq!(disp.total_command_char_len(), 0);
    assert_eq!(disp.mean_command_char_len(), 0);
}

#[test]
fn dispatcher_median_command_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_command_char_len(), Some(15));
}

#[test]
fn dispatcher_median_command_char_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.median_command_char_len(), None);
}

#[test]
fn dispatcher_mode_command_char_len_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_command_char_len(), Some(15));
}

#[test]
fn dispatcher_mode_command_char_len_none_when_empty() {
    let reg = Registry::new();
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    assert_eq!(disp.mode_command_char_len(), None);
}

#[test]
fn dispatcher_command_char_len_counts_match() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let m = disp.command_char_len_counts();
    assert_eq!(m.get(&15), Some(&1));
}

#[test]
fn chord_trie_max_min_command_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    // commands: "x" (1), "yyyy" (4)
    assert_eq!(t.max_command_byte_len(), Some(4));
    assert_eq!(t.min_command_byte_len(), Some(1));
}

#[test]
fn chord_trie_total_command_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    assert_eq!(t.total_command_byte_len(), 5);
}

#[test]
fn chord_trie_mean_command_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    // (1+4)/2 = 2
    assert_eq!(t.mean_command_byte_len(), 2);
}

#[test]
fn chord_trie_command_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.max_command_byte_len(), None);
    assert_eq!(t.min_command_byte_len(), None);
    assert_eq!(t.total_command_byte_len(), 0);
    assert_eq!(t.mean_command_byte_len(), 0);
}

#[test]
fn chord_trie_median_command_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yy"), ("c", "zzz")]);
    // lens sorted: [1, 2, 3] -> median 2
    assert_eq!(t.median_command_byte_len(), Some(2));
}

#[test]
fn chord_trie_median_command_byte_len_even() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "zzzzz")]);
    // [1, 5] -> midpoint 3
    assert_eq!(t.median_command_byte_len(), Some(3));
}

#[test]
fn chord_trie_median_command_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.median_command_byte_len(), None);
}

#[test]
fn chord_trie_mode_command_byte_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "xx"), ("b", "yy"), ("c", "zzz")]);
    // lens: 2,2,3 -> mode 2
    assert_eq!(t.mode_command_byte_len(), Some(2));
}

#[test]
fn chord_trie_mode_command_byte_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mode_command_byte_len(), None);
}

#[test]
fn chord_trie_command_byte_len_counts_match() {
    let t = closure_input::ChordTrie::build(&[("a", "xx"), ("b", "yy"), ("c", "zzz")]);
    let counts = t.command_byte_len_counts();
    assert_eq!(counts.get(&2), Some(&2));
    assert_eq!(counts.get(&3), Some(&1));
}

#[test]
fn chord_trie_max_min_command_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    assert_eq!(t.max_command_char_len(), Some(4));
    assert_eq!(t.min_command_char_len(), Some(1));
}

#[test]
fn chord_trie_total_command_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    assert_eq!(t.total_command_char_len(), 5);
}

#[test]
fn chord_trie_mean_command_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yyyy")]);
    assert_eq!(t.mean_command_char_len(), 2);
}

#[test]
fn chord_trie_command_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.max_command_char_len(), None);
    assert_eq!(t.min_command_char_len(), None);
    assert_eq!(t.total_command_char_len(), 0);
    assert_eq!(t.mean_command_char_len(), 0);
}

#[test]
fn chord_trie_median_command_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "yy"), ("c", "zzz")]);
    assert_eq!(t.median_command_char_len(), Some(2));
}

#[test]
fn chord_trie_median_command_char_len_even() {
    let t = closure_input::ChordTrie::build(&[("a", "x"), ("b", "zzzzz")]);
    assert_eq!(t.median_command_char_len(), Some(3));
}

#[test]
fn chord_trie_median_command_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.median_command_char_len(), None);
}

#[test]
fn chord_trie_mode_command_char_len_match() {
    let t = closure_input::ChordTrie::build(&[("a", "xx"), ("b", "yy"), ("c", "zzz")]);
    assert_eq!(t.mode_command_char_len(), Some(2));
}

#[test]
fn chord_trie_mode_command_char_len_none_when_empty() {
    let t = closure_input::ChordTrie::build(&[]);
    assert_eq!(t.mode_command_char_len(), None);
}

#[test]
fn chord_trie_command_char_len_counts_match() {
    let t = closure_input::ChordTrie::build(&[("a", "xx"), ("b", "yy"), ("c", "zzz")]);
    let counts = t.command_char_len_counts();
    assert_eq!(counts.get(&2), Some(&2));
    assert_eq!(counts.get(&3), Some(&1));
}
