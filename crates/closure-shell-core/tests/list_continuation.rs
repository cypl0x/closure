//! "auto counting and insertion of numbers or dashes, depending on the
//! chord. Do research the Doom Emacs ones."
//!
//! org's rules, which Doom inherits: `RET` at the end of a list item
//! opens the next one with the same marker, `M-RET`
//! (`org-meta-return`) does it from anywhere on the line, and a
//! numbered list counts — inserting into the middle renumbers what
//! follows rather than leaving two `3.`s.
//!
//! And the rule that makes the others usable: `RET` on an *empty* item
//! ends the list instead of making another empty one. Without it every
//! list ends in a stray bullet you have to go back and delete, which
//! is the state before this change — Enter did nothing but split the
//! line, so every item after the first was typed by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{list_continuation, renumber_list};

#[test]
fn a_dash_item_continues_with_a_dash() {
    assert_eq!(list_continuation("- milk").as_deref(), Some("- "));
    assert_eq!(list_continuation("+ milk").as_deref(), Some("+ "));
}

#[test]
fn the_indentation_comes_with_it() {
    // A nested item continues at its own depth; org's own rule, and
    // the reason nested lists survive being typed.
    assert_eq!(list_continuation("  - nested").as_deref(), Some("  - "));
    assert_eq!(list_continuation("    1. deep").as_deref(), Some("    2. "));
}

#[test]
fn a_number_counts_up() {
    assert_eq!(list_continuation("1. first").as_deref(), Some("2. "));
    assert_eq!(list_continuation("9) ninth").as_deref(), Some("10) "));
}

#[test]
fn a_checkbox_item_continues_unticked() {
    // A new item is not done, whatever the one above it says.
    assert_eq!(list_continuation("- [ ] todo").as_deref(), Some("- [ ] "));
    assert_eq!(list_continuation("- [X] done").as_deref(), Some("- [ ] "));
}

#[test]
fn an_empty_item_ends_the_list() {
    // The rule that makes the rest usable: `RET` on an empty item
    // stops rather than making another empty one.
    assert_eq!(list_continuation("- "), None);
    assert_eq!(list_continuation("  1. "), None);
    assert_eq!(list_continuation("- [ ] "), None);
}

#[test]
fn prose_is_not_a_list() {
    assert_eq!(list_continuation("just a sentence"), None);
    assert_eq!(list_continuation(""), None);
    assert_eq!(list_continuation("1984 was a year"), None);
    assert_eq!(list_continuation("* A headline"), None);
}

#[test]
fn inserting_in_the_middle_renumbers_what_follows() {
    // Two `3.`s is what "auto counting" is asking to avoid.
    let text = "1. one\n2. two\n2. inserted\n3. three\n";
    assert_eq!(
        renumber_list(text, 2),
        "1. one\n2. two\n3. inserted\n4. three\n"
    );
}

#[test]
fn renumbering_leaves_the_prose_around_it_alone() {
    let text = "intro\n1. one\n5. two\nafter\n";
    assert_eq!(renumber_list(text, 1), "intro\n1. one\n2. two\nafter\n");
}

#[test]
fn a_nested_list_is_counted_on_its_own() {
    // Depth is a different list: the inner one starts at 1 and the
    // outer one does not skip because of it.
    let text = "1. one\n   1. inner\n   7. inner two\n2. two\n";
    assert_eq!(
        renumber_list(text, 1),
        "1. one\n   1. inner\n   2. inner two\n2. two\n"
    );
}

#[test]
fn a_bulleted_list_is_left_as_it_is() {
    let text = "- one\n- two\n";
    assert_eq!(renumber_list(text, 0), text);
}

#[test]
fn the_separator_the_list_uses_is_kept() {
    // `1)` and `1.` are both org, and a list does not change style
    // halfway down because something was inserted.
    let text = "1) one\n1) two\n";
    assert_eq!(renumber_list(text, 0), "1) one\n2) two\n");
}

// --- and the same rules, through the editor -----------------------

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn editing(body: &str) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* Alpha\n:PROPERTIES:\n:ID: 01HQLIST00000000000001\n:END:\n{body}"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQLIST00000000000001"));
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    (dir, shell, app)
}

/// Put the caret at the end of line `n`.
fn at_end_of(app: &mut ModalApp, n: usize, len: usize) {
    app.body_click(n, len);
}

#[test]
fn enter_at_the_end_of_an_item_opens_the_next_one() {
    let (_d, mut shell, mut app) = editing("- milk\n");
    at_end_of(&mut app, 0, 6);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer().starts_with("- milk\n- "),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn enter_counts_a_numbered_list_up() {
    let (_d, mut shell, mut app) = editing("1. first\n");
    at_end_of(&mut app, 0, 8);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer().starts_with("1. first\n2. "),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn enter_on_an_empty_item_ends_the_list() {
    let (_d, mut shell, mut app) = editing("- milk\n- \n");
    at_end_of(&mut app, 1, 2);
    app.on_key(&mut shell, "enter", false, false, None);
    let text = app.body_buffer();
    assert!(!text.contains("- \n- "), "another empty item: {text:?}");
    assert!(text.starts_with("- milk\n"), "{text:?}");
}

#[test]
fn enter_in_prose_is_still_a_newline() {
    let (_d, mut shell, mut app) = editing("a sentence\n");
    at_end_of(&mut app, 0, 10);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer().starts_with("a sentence\n"),
        "{:?}",
        app.body_buffer()
    );
    assert!(!app.body_buffer().contains("- "), "invented a bullet");
}
