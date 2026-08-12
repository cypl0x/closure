//! A resumed list shows the number it says it starts at.
//!
//! `1. [@3] third step` is what an author writes when a numbered list
//! was interrupted by prose or a block and picks up again. Unread, the
//! second half restarts at one and the third step is labelled first —
//! and the `[@3]` sits in the body as visible noise, so the feature
//! cost the reader twice.
//!
//! Tenth construct through the preview seam (I12): the file keeps
//! `[@3]`, the reader sees `3.`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Recipe
:PROPERTIES:
:ID: 01LISTNUM00000000001
:END:
1. chop the onions
2. fry them

some prose in between

1. [@3] add the stock
2. simmer
";

fn body(src: &str, title: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    let rows = app.rows(&shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, &shell);
    app.selected_detail(&shell).expect("a detail").body.clone()
}

#[test]
fn the_resumed_list_is_numbered_from_where_it_says() {
    let b = body(VAULT, "Recipe");
    assert!(b.contains("3. add the stock"), "{b}");
}

#[test]
fn the_item_after_it_follows_on() {
    // A counter that renumbered only its own line would leave "3." then
    // "2.", which is worse than not reading it at all.
    let b = body(VAULT, "Recipe");
    assert!(b.contains("4. simmer"), "{b}");
}

#[test]
fn the_cookie_itself_is_not_shown() {
    // It is an instruction about numbering, not part of the step.
    let b = body(VAULT, "Recipe");
    assert!(!b.contains("[@3]"), "{b}");
}

#[test]
fn a_list_before_it_is_untouched() {
    let b = body(VAULT, "Recipe");
    assert!(b.contains("1. chop the onions"), "{b}");
    assert!(b.contains("2. fry them"), "{b}");
}

#[test]
fn a_list_with_no_counter_is_left_exactly_as_written() {
    let src = "* Plain\n:PROPERTIES:\n:ID: 01LISTNUM00000000002\n:END:\n1. one\n2. two\n";
    let b = body(src, "Plain");
    assert!(b.contains("1. one") && b.contains("2. two"), "{b}");
}

#[test]
fn a_checklist_keeps_its_boxes() {
    // `[ ]` sits where `[@3]` does. Renumbering must not eat it.
    let src = "* Tasks\n:PROPERTIES:\n:ID: 01LISTNUM00000000003\n:END:\n\
               1. [@3] [ ] third task\n2. [X] fourth\n";
    let b = body(src, "Tasks");
    assert!(b.contains("3. [ ] third task"), "{b}");
    assert!(b.contains("4. [X] fourth"), "{b}");
}

#[test]
fn the_file_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    let rows = app.rows(&shell);
    let i = rows.iter().position(|r| r.title == "Recipe").expect("row");
    app.select(i, &shell);
    let _ = app.selected_detail(&shell);
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(on_disk.contains("1. [@3] add the stock"), "{on_disk}");
}
