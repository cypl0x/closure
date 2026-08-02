//! The Notion-style "/" block menu.
//!
//! The vision's default editing mode is "Notion like … blocks with a
//! plus sign and slash commands", *compatible with org-mode*. So the
//! menu inserts real org syntax — `#+BEGIN_SRC`, a checkbox item, a
//! table row — rather than a private block model that would have to be
//! serialised later. What you pick is what lands in the file (I1).
//!
//! `slash_menu` already existed and listed *commands*; this is the
//! other half, the one that actually writes org, and the editor state
//! that drives it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, block_templates};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQSLASH000000000000001\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open the body editor and start typing into it.
///
/// The "/" menu is an INSERT-mode affordance — it is Notion's, and it
/// triggers on a typed slash — so the fixture gets there the way the
/// user does: the buffer opens in NORMAL in a modal mode, and `i` is
/// what starts typing.
fn editing() -> (tempfile::TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(
        app.body_mode(),
        closure_shell_core::EditorMode::Normal,
        "a modal mode opens the buffer in NORMAL"
    );
    app.on_key(&mut shell, "i", false, false, Some('i'));
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Insert);
    (dir, shell, app)
}

fn type_str(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, "x", false, false, Some(c));
    }
}

// === the template catalogue ===

#[test]
fn the_menu_offers_the_org_blocks_that_matter() {
    let labels: Vec<&str> = block_templates("").iter().map(|t| t.label).collect();
    for expected in [
        "Heading", "To-do", "Code", "Quote", "Table", "Link", "Divider",
    ] {
        assert!(labels.contains(&expected), "missing {expected}: {labels:?}");
    }
}

#[test]
fn every_template_inserts_real_org_syntax() {
    // The compatibility promise: anything the menu writes must parse
    // as org, so a block inserted in the GUI is a block Emacs reads.
    for t in block_templates("") {
        let doc = closure_core::Document::load_str(&format!("* H\n{}\n", t.text))
            .unwrap_or_else(|e| panic!("{} produced unparseable org: {e}", t.label));
        assert_eq!(
            doc.source(),
            format!("* H\n{}\n", t.text),
            "{} must round-trip byte-exact (I1)",
            t.label
        );
    }
}

#[test]
fn the_cursor_offset_lands_inside_the_template() {
    for t in block_templates("") {
        assert!(
            t.cursor <= t.text.chars().count(),
            "{}: cursor {} past the end of {:?}",
            t.label,
            t.cursor,
            t.text
        );
    }
    // The code block puts you on its empty middle line, not after the
    // #+END_SRC — that is the whole point of picking it.
    let code = block_templates("code")
        .into_iter()
        .find(|t| t.label == "Code")
        .expect("Code");
    let before: String = code.text.chars().take(code.cursor).collect();
    assert!(before.contains("BEGIN_SRC"), "cursor is past the opener");
    assert!(!before.contains("END_SRC"), "but before the closer");
}

#[test]
fn the_menu_fuzzy_filters() {
    let hits = block_templates("cod");
    assert!(!hits.is_empty());
    assert_eq!(hits[0].label, "Code", "best match first: {hits:?}");
    assert!(
        block_templates("zzzznotathing").is_empty(),
        "no match means no menu"
    );
}

// === the editor state that drives it ===

#[test]
fn slash_at_the_start_of_a_line_opens_the_menu() {
    let (_d, mut shell, mut app) = editing();
    assert_eq!(app.slash_query(), None, "closed to begin with");
    type_str(&mut app, &mut shell, "/");
    assert_eq!(app.slash_query(), Some(""), "open, empty query");
    assert_eq!(
        app.body_buffer(),
        "/",
        "the slash is typed, Notion-style, not swallowed"
    );
}

#[test]
fn slash_mid_word_is_just_a_slash() {
    // `and/or`, a URL, a date — a slash after text is text.
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "and/or");
    assert_eq!(app.slash_query(), None, "no menu inside a word");
    assert_eq!(app.body_buffer(), "and/or");
}

#[test]
fn typing_after_the_slash_narrows_the_menu() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/cod");
    assert_eq!(app.slash_query(), Some("cod"));
    let items = app.slash_items();
    assert_eq!(items[0].label, "Code", "{items:?}");
}

#[test]
fn backspacing_past_the_slash_closes_the_menu() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/co");
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.slash_query(), Some("c"));
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.slash_query(), Some(""));
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.slash_query(), None, "the slash itself is gone");
    assert_eq!(app.body_buffer(), "");
}

#[test]
fn escape_dismisses_the_menu_without_leaving_the_editor() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/co");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.slash_query(), None, "menu closed");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "…but Escape was consumed by the menu, not by the editor"
    );
    assert_eq!(app.body_buffer(), "/co", "the typed text stays");
}

#[test]
fn accepting_a_template_replaces_the_slash_query() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "prose\n");
    type_str(&mut app, &mut shell, "/code");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.slash_query(), None, "menu closes on accept");
    let body = app.body_buffer();
    assert!(
        body.starts_with("prose\n"),
        "text before the slash survives: {body:?}"
    );
    assert!(
        !body.contains("/code"),
        "the trigger text is consumed: {body:?}"
    );
    assert!(body.contains("#+BEGIN_SRC"), "{body:?}");
    assert!(body.contains("#+END_SRC"), "{body:?}");
}

#[test]
fn accepting_puts_the_cursor_inside_the_new_block() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/code");
    app.on_key(&mut shell, "enter", false, false, None);
    let (line, _) = app.body_cursor();
    let lines: Vec<&str> = app.body_buffer().lines().collect();
    assert!(
        lines[line].is_empty(),
        "cursor sits on the empty body line: line {line} of {lines:?}"
    );
}

#[test]
fn the_menu_moves_its_own_cursor_and_accepts_that_choice() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/");
    let second = app.slash_items()[1].label;
    app.on_key(&mut shell, "down", false, false, None);
    assert_eq!(app.slash_cursor(), 1);
    app.on_key(&mut shell, "enter", false, false, None);
    let expected = block_templates("")
        .into_iter()
        .find(|t| t.label == second)
        .expect("template");
    assert!(
        app.body_buffer()
            .contains(expected.text.lines().next().unwrap()),
        "accepted {second}: {:?}",
        app.body_buffer()
    );
}

#[test]
fn an_open_menu_does_not_swallow_ordinary_saves() {
    // C-Enter still commits the body; the menu must not intercept it.
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/code");
    app.run(&mut shell, "commit-edit");
    assert_eq!(app.surface(), ModalSurface::Browse, "the body was saved");
}

// === the mouse path into the menu ===

#[test]
fn clicking_an_entry_accepts_it_like_enter_does() {
    // I8: the mouse runs the same thing the keyboard does. Clicking
    // row N must be indistinguishable from arrowing to N and pressing
    // Enter.
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/");
    app.slash_click(2);
    let clicked = app.body_buffer().to_owned();

    let (_d2, mut shell2, mut app2) = editing();
    type_str(&mut app2, &mut shell2, "/");
    app2.on_key(&mut shell2, "down", false, false, None);
    app2.on_key(&mut shell2, "down", false, false, None);
    app2.on_key(&mut shell2, "enter", false, false, None);

    assert_eq!(clicked, app2.body_buffer());
    assert_eq!(app.body_cursor(), app2.body_cursor(), "cursor too");
    assert_eq!(app.slash_query(), None, "and the menu closes either way");
}

#[test]
fn clicking_past_the_end_of_the_menu_is_a_no_op() {
    let (_d, mut shell, mut app) = editing();
    type_str(&mut app, &mut shell, "/");
    app.slash_click(999);
    assert_eq!(app.body_buffer(), "/", "nothing inserted");
    assert_eq!(app.slash_query(), Some(""), "and the menu stays open");
}
