//! "C-c C-l add org link interactively" — with screenshots of org's own
//! flow: a menu of link types (`file:`, `id:`, `http:`, …), TAB to
//! complete the type, RET to complete the destination, and a
//! description prompt, ending in `[[dest][description]]` at the cursor.
//!
//! Writing that by hand means typing four brackets in the right order
//! around a path you have to remember, which is exactly the kind of
//! thing the shell should know.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "\
* One
:PROPERTIES:
:ID: 01LINKAAAAAAAAAAAAAAAAAAA
:END:
first body
* Target headline
:PROPERTIES:
:ID: 01LINKBBBBBBBBBBBBBBBBBBB
:END:
second body
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn the_link_types_org_offers_are_offered() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    assert_eq!(app.surface(), ModalSurface::InsertLink);
    let types = app.link_types();
    for want in ["file:", "id:", "http:", "https:"] {
        assert!(types.iter().any(|t| t == want), "{want} missing: {types:?}");
    }
}

#[test]
fn a_link_lands_at_the_cursor_with_its_description() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    // Pick `https:` by filtering to it.
    type_in(&mut app, &mut shell, "https");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::InsertLink);
    type_in(&mut app, &mut shell, "//github.com/gitoxidelabs/gitoxide");
    app.on_key(&mut shell, "enter", false, false, None);
    type_in(&mut app, &mut shell, "gitoxide");
    app.on_key(&mut shell, "enter", false, false, None);

    assert!(
        app.body_buffer()
            .contains("[[https://github.com/gitoxidelabs/gitoxide][gitoxide]]"),
        "the link is not in the body: {:?}",
        app.body_buffer()
    );
}

#[test]
fn an_empty_description_leaves_a_bare_link() {
    // org writes `[[dest]]` when there is nothing to call it, and a
    // link ending in `][]]` is the sort of thing you find months later.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    type_in(&mut app, &mut shell, "https");
    app.on_key(&mut shell, "enter", false, false, None);
    type_in(&mut app, &mut shell, "//example.org");
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer().contains("[[https://example.org]]"),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn escape_leaves_the_body_untouched() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    let before = app.body_buffer().to_owned();
    app.run(&mut shell, "insert-link");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(
        app.body_buffer(),
        before,
        "an abandoned link left something"
    );
    assert!(
        app.surface().is_editor(),
        "it did not go back to the buffer"
    );
}

#[test]
fn the_id_type_completes_over_the_vaults_headlines() {
    // The reason `id:` is worth having at all: you do not remember a
    // ULID, you remember what the note is called.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    type_in(&mut app, &mut shell, "id");
    app.on_key(&mut shell, "enter", false, false, None);
    let suggestions = app.link_completions(&shell);
    assert!(
        suggestions
            .iter()
            .any(|s| s.label.contains("Target headline")),
        "headlines are not offered: {suggestions:?}"
    );
}

#[test]
fn the_chord_itself_works_inside_the_buffer() {
    // The command was reachable, and `C-c C-l` was not: inside a body
    // `C-c` was a two-chord dead end that knew only its own endings
    // (`C-c C-c` to save, `C-c C-k` to discard) and swallowed every
    // other `C-c` chord the keymap advertises — in the one place an
    // org user types them.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "c", true, false, Some('c'));
    app.on_key(&mut shell, "l", true, false, Some('l'));
    assert_eq!(
        app.surface(),
        ModalSurface::InsertLink,
        "C-c C-l did nothing in the buffer (status: {})",
        app.status()
    );
}

#[test]
fn the_two_chords_the_buffer_owns_still_win() {
    // `C-c C-c` and `C-c C-k` are the editor's own and are not in the
    // keymap; routing the rest through it must not cost them.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "c", true, false, Some('c'));
    app.on_key(&mut shell, "k", true, false, Some('k'));
    assert!(
        !app.surface().is_editor(),
        "C-c C-k did not discard the buffer"
    );
}

#[test]
fn the_chord_works_in_the_file_buffer_too() {
    // Doom's view *is* the file, so the full-window editor is where an
    // org user spends the session — a link chord that only works in the
    // detached body editor is a link chord they never reach.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile, "no file buffer");
    app.on_key(&mut shell, "c", true, false, Some('c'));
    app.on_key(&mut shell, "l", true, false, Some('l'));
    assert_eq!(
        app.surface(),
        ModalSurface::InsertLink,
        "C-c C-l did nothing in the file buffer (status: {})",
        app.status()
    );
}

#[test]
fn enter_takes_the_headline_the_list_is_pointing_at() {
    // The whole reason `id:` completes is that you do not know the
    // ULID. The list highlighted a row and Enter read the empty field
    // instead, so the one link type that needs the picker was the one
    // type you could not finish without knowing the answer already.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    type_in(&mut app, &mut shell, "id");
    app.on_key(&mut shell, "enter", false, false, None);
    type_in(&mut app, &mut shell, "Target");
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer()
            .contains("[[id:01LINKBBBBBBBBBBBBBBBBBBB]]"),
        "the highlighted headline was not the one linked: {:?}",
        app.body_buffer()
    );
}

#[test]
fn the_arrows_walk_the_completions() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    type_in(&mut app, &mut shell, "id");
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "down", false, false, None);
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        app.body_buffer()
            .contains("[[id:01LINKBBBBBBBBBBBBBBBBBBB]]"),
        "down did not move the completion cursor: {:?}",
        app.body_buffer()
    );
}

#[test]
fn a_typed_destination_still_wins_where_nothing_completes() {
    // `https:` has no candidates, so the field is the answer — the
    // completion rule must not swallow the ordinary case.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "insert-link");
    type_in(&mut app, &mut shell, "https");
    app.on_key(&mut shell, "enter", false, false, None);
    type_in(&mut app, &mut shell, "//example.org");
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.body_buffer().contains("[[https://example.org]]"));
}
