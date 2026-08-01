//! A headline's body editor shows — and edits — its whole subtree.
//!
//! Reported 2026-08-01: "sub tree should be visible and editable in the
//! body and editor. The body and the tree view should sync the
//! content", and later: "the current behavior is kinda annoying,
//! because items go out of sight real quick".
//!
//! The body editor showed a headline's own prose and nothing else, so
//! its children lived only in the tree on the left. You could not read
//! a subtree as a document, and a headline you typed into a body
//! vanished from the buffer the instant it was saved — it had become a
//! child, and children were not shown.
//!
//! Sync is on save, both ways: the buffer is written to the vault when
//! you save it, and the buffer is filled from the vault when you open
//! it. No half-applied third state in between.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const SRC: &str = "* Parent\n\
                   :PROPERTIES:\n\
                   :ID: 01HQSIB0000000000000001\n\
                   :END:\n\
                   parent prose\n\
                   ** One\n\
                   :PROPERTIES:\n\
                   :ID: 01HQSIB0000000000000002\n\
                   :END:\n\
                   ** Two\n\
                   :PROPERTIES:\n\
                   :ID: 01HQSIB0000000000000003\n\
                   :END:\n\
                   * Next\n\
                   :PROPERTIES:\n\
                   :ID: 01HQSIB0000000000000004\n\
                   :END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn titles(app: &ModalApp, sh: &Shell) -> Vec<String> {
    app.rows(sh).into_iter().map(|r| r.title).collect()
}

/// Open Parent's body editor.
fn open_parent(app: &mut ModalApp, sh: &mut Shell) {
    app.select(0, sh);
    app.run(sh, "edit-body");
    assert!(app.surface().is_editor(), "the body is open");
}

#[test]
fn the_body_shows_the_children() {
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    let buf = app.body_buffer();
    assert!(buf.contains("parent prose"), "its own prose: {buf}");
    assert!(buf.contains("** One"), "and its children: {buf}");
    assert!(buf.contains("** Two"), "{buf}");
}

#[test]
fn it_stops_at_the_next_top_level_headline() {
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    assert!(
        !app.body_buffer().contains("Next"),
        "a sibling is not a child: {}",
        app.body_buffer()
    );
}

#[test]
fn opening_and_saving_without_typing_changes_nothing() {
    // The round trip is the whole feature. If a save that follows no
    // edit rewrites the file, nothing else here can be trusted.
    let (dir, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    app.run(&mut sh, "save-buffer");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert_eq!(on_disk, SRC);
}

#[test]
fn renaming_a_child_in_the_buffer_renames_it_in_the_tree() {
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    let edited = app.body_buffer().replace("** One", "** One renamed");
    app.body_replace_range(0..app.body_buffer().len(), &edited);
    app.run(&mut sh, "save-buffer");
    assert!(
        titles(&app, &sh).contains(&"One renamed".to_owned()),
        "{:?}",
        titles(&app, &sh)
    );
}

#[test]
fn a_child_deleted_from_the_buffer_leaves_the_tree() {
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    let edited = app.body_buffer().replace("** Two\n", "");
    app.body_replace_range(0..app.body_buffer().len(), &edited);
    app.run(&mut sh, "save-buffer");
    let left = titles(&app, &sh);
    assert!(!left.contains(&"Two".to_owned()), "{left:?}");
    assert!(left.contains(&"One".to_owned()), "One stayed: {left:?}");
}

#[test]
fn a_headline_typed_into_the_buffer_stays_in_the_buffer() {
    // The complaint in one sentence: it used to become a child and
    // vanish from the editor, because children were not shown.
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    let edited = format!("{}** Three\n", app.body_buffer());
    app.body_replace_range(0..app.body_buffer().len(), &edited);
    app.run(&mut sh, "save-buffer");
    assert!(
        titles(&app, &sh).contains(&"Three".to_owned()),
        "it is real"
    );
    assert!(
        app.body_buffer().contains("** Three"),
        "and still on screen: {}",
        app.body_buffer()
    );
}

#[test]
fn the_children_keep_their_ids_across_a_save() {
    // Identity is what links, sync and undo address a block by, so a
    // round trip that mints new ids is a round trip that breaks them.
    let (dir, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    app.run(&mut sh, "save-buffer");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    for id in ["01HQSIB0000000000000002", "01HQSIB0000000000000003"] {
        assert!(on_disk.contains(id), "{id} survived: {on_disk}");
    }
}

#[test]
fn a_change_made_in_the_tree_shows_up_when_the_body_is_reopened() {
    // The other direction of "synced with the tree view and vice
    // versa": the buffer is filled from the vault every time it opens.
    let (_d, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    app.run(&mut sh, "save-buffer");
    app.on_key(&mut sh, "escape", false, false, None);
    let at = app
        .rows(&sh)
        .iter()
        .position(|r| r.title == "Two")
        .expect("Two");
    app.select(at, &sh);
    app.run(&mut sh, "toggle-todo");
    open_parent(&mut app, &mut sh);
    assert!(
        app.body_buffer().contains("** TODO Two"),
        "the tree's edit is in the buffer: {}",
        app.body_buffer()
    );
}

#[test]
fn a_childless_headline_still_opens_with_just_its_prose() {
    let (_d, mut sh, mut app) = fixture();
    let at = app
        .rows(&sh)
        .iter()
        .position(|r| r.title == "Next")
        .expect("Next");
    app.select(at, &sh);
    app.run(&mut sh, "edit-body");
    assert_eq!(app.body_buffer(), "", "nothing under it, nothing shown");
}

#[test]
fn the_save_chord_reaches_the_buffer_it_is_meant_to_save() {
    // `closure-input` advertises `C-s save-buffer` in every mode, and
    // inside a buffer it did nothing: the editor took every modified
    // chord for itself, so the one place saving means something was the
    // one place the chord was dead. Sync-on-save needs a save.
    let (dir, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    let edited = format!("{}** Three\n", app.body_buffer());
    app.body_replace_range(0..app.body_buffer().len(), &edited);
    app.on_key(&mut sh, "s", true, false, None); // C-s
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("** Three"), "written: {on_disk}");
    assert!(app.surface().is_editor(), "and the buffer is still open");
}

#[test]
fn the_save_chord_works_in_insert_too() {
    // Nobody drops to NORMAL first to press Ctrl-S.
    let (dir, mut sh, mut app) = fixture();
    open_parent(&mut app, &mut sh);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "typed".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "s", true, false, None);
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("typed"), "{on_disk}");
}
