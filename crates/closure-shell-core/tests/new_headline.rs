//! Org's four new-headline chords.
//!
//! org-mode gives you `M-RET` for a heading, and adds Shift to make it
//! a TODO (`org-insert-todo-heading`). closure bound only `M-RET`, and
//! it made a headline called "untitled" without asking — so the one
//! chord that existed was also the one you had to undo afterwards.
//!
//! The Ctrl axis is closure's. In org, `C-RET` differs from `M-RET`
//! only by respecting content — a distinction that exists because org
//! edits a *buffer* with a point somewhere inside a subtree. The
//! outline has no point inside anything: a new sibling always lands
//! after the selected subtree, so both chords would do the same thing.
//! Rather than bind a synonym, Ctrl means "one level down" and the
//! four chords cover the grid the request asked for:
//!
//! |         | sibling   | child       |
//! | plain   | `M-RET`   | `C-RET`     |
//! | TODO    | `M-S-RET` | `C-S-RET`   |

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n\
                   :PROPERTIES:\n\
                   :ID: 01HQNEWH000000000000001\n\
                   :END:\n\
                   ** Alpha child\n\
                   :PROPERTIES:\n\
                   :ID: 01HQNEWH000000000000002\n\
                   :END:\n\
                   * Beta\n\
                   :PROPERTIES:\n\
                   :ID: 01HQNEWH000000000000003\n\
                   :END:\n";

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

/// Press one of the four chords with the cursor on `Alpha`, then type
/// `title` and accept.
fn new_heading(app: &mut ModalApp, sh: &mut Shell, key: &str, ctrl: bool, alt: bool, title: &str) {
    app.select(0, sh);
    app.on_key(sh, key, ctrl, alt, None);
    assert_eq!(
        app.surface(),
        ModalSurface::AddSibling,
        "the chord asks for a title instead of inventing one"
    );
    for c in title.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(sh, "enter", false, false, None);
}

/// The row with this title, once the outline has been rebuilt.
fn row(app: &ModalApp, sh: &Shell, title: &str) -> closure_shell_core::Row {
    app.rows(sh)
        .into_iter()
        .find(|r| r.title == title)
        .unwrap_or_else(|| panic!("no row titled {title}"))
}

#[test]
fn m_ret_asks_for_a_title_and_makes_a_plain_sibling() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", false, true, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 1, "a sibling of Alpha");
    assert_eq!(r.todo, None, "plain: no keyword");
}

#[test]
fn m_s_ret_makes_the_sibling_a_todo() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "shift-enter", false, true, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 1, "still a sibling");
    assert_eq!(r.todo.as_deref(), Some("TODO"), "Shift is org's TODO axis");
}

#[test]
fn c_ret_makes_a_plain_child() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", true, false, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 2, "one level under Alpha");
    assert_eq!(r.todo, None);
}

#[test]
fn c_s_ret_makes_a_todo_child() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "shift-enter", true, false, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 2);
    assert_eq!(r.todo.as_deref(), Some("TODO"));
}

#[test]
fn a_child_lands_under_the_selection_not_under_its_children() {
    // `Alpha` already has a child. The new one is Alpha's, not its
    // sibling's — "child" is measured from the cursor.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", true, false, "Gamma");
    assert_eq!(row(&app, &sh, "Gamma").level, 2);
    assert_eq!(row(&app, &sh, "Alpha child").level, 2, "unmoved");
}

#[test]
fn escape_makes_nothing() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    let before = app.rows(&sh).len();
    app.select(0, &sh);
    app.on_key(&mut sh, "enter", false, true, None);
    for c in "Gamma".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.rows(&sh).len(), before, "the outline is untouched");
}

#[test]
fn an_empty_title_makes_nothing() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    let before = app.rows(&sh).len();
    app.select(0, &sh);
    app.on_key(&mut sh, "enter", false, true, None);
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.rows(&sh).len(), before);
}

#[test]
fn every_mode_binds_all_four() {
    // The keymap is the single source of truth the palette and
    // which-key render from (I4), so a chord that only works in Doom
    // is a chord three other users cannot discover.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for (chord, want) in [
            ("M-RET", "add-heading"),
            ("M-S-RET", "add-todo-heading"),
            ("C-RET", "add-child-heading"),
            ("C-S-RET", "add-todo-child-heading"),
        ] {
            assert_eq!(
                closure_input::command_for(mode, chord),
                Some(want),
                "{mode:?} binds {chord}"
            );
        }
    }
}

#[test]
fn all_four_chords_reach_the_prompt_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for (key, ctrl, alt) in [
            ("enter", false, true),
            ("shift-enter", false, true),
            ("enter", true, false),
            ("shift-enter", true, false),
        ] {
            let (_d, mut sh, mut app) = fixture(mode);
            app.select(0, &sh);
            app.on_key(&mut sh, key, ctrl, alt, None);
            assert_eq!(
                app.surface(),
                ModalSurface::AddSibling,
                "{mode:?} {key} ctrl={ctrl} alt={alt}"
            );
        }
    }
}

// === `A` is `a` with a TODO on it ===
//
// Reported 2026-08-02: "add sibling with shift+a (aka A) to add sibling
// as TODO item". The outline's own `a` opens the sibling prompt; the
// plain-vs-TODO axis already exists on the `M-RET` family, and shift is
// how org spells it there too.

#[test]
fn shift_a_adds_the_sibling_as_a_todo() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "A", false, false, Some('A'));
    assert_eq!(app.surface(), ModalSurface::AddSibling, "same prompt");
    for c in "Gamma".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 1, "a sibling, like `a`");
    assert_eq!(r.todo.as_deref(), Some("TODO"), "but a TODO one");
}

#[test]
fn lowercase_a_still_adds_a_plain_sibling() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "a", false, false, Some('a'));
    for c in "Delta".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(row(&app, &sh, "Delta").todo, None);
}

#[test]
fn every_modal_mode_gets_the_shifted_one() {
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        assert_eq!(
            closure_input::command_for(mode, "A"),
            Some("add-todo-heading"),
            "{mode:?}"
        );
    }
}
