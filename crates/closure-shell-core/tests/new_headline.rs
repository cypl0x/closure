//! Org's four new-headline chords.
//!
//! org-mode gives you `M-RET` for a heading, and adds Shift to make it
//! a TODO (`org-insert-todo-heading`). closure bound only `M-RET`, and
//! it made a headline called "untitled" without asking — so the one
//! chord that existed was also the one you had to undo afterwards.
//!
//! The Ctrl axis used to be closure's own: `C-RET` meant "one level
//! down", because in org it differs from `M-RET` only by respecting
//! content and the outline has no point inside a subtree to respect.
//!
//! Rewritten 2026-08-04, at the user's ask ("research the Doom Emacs
//! keybindings for quick header creation in the editor … ctrl+enter ->
//! new sibling headline below, ctrl+shift+enter -> new sibling headline
//! above"). Their Doom, `modules/lang/org/config.el`:
//!
//! ```elisp
//! "C-RET"   #'+org/insert-item-below
//! "C-S-RET" #'+org/insert-item-above
//! "C-M-RET" #'org-insert-subheading
//! ```
//!
//! So Shift is the *direction* on the Ctrl layer and the *keyword* on
//! the Meta layer, and the child moves to `C-M-RET`:
//!
//! |         | below      | above       | child       |
//! | plain   | `M-RET`    | `C-S-RET`   | `C-M-RET`   |
//! | `C-RET` | (= `M-RET`)|             |             |
//! | TODO    | `M-S-RET`  |             | `C-M-S-RET` |
//!
//! `C-RET` and `M-RET` are one command: Doom sets
//! `org-insert-heading-respect-content t`, closure's `add-heading`
//! already inserts after the whole subtree, and a synonym would only
//! give the palette two entries for one action.

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
fn c_m_ret_makes_a_plain_child() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", true, true, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 2, "one level under Alpha");
    assert_eq!(r.todo, None);
}

#[test]
fn c_m_s_ret_makes_a_todo_child() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "shift-enter", true, true, "Gamma");
    let r = row(&app, &sh, "Gamma");
    assert_eq!(r.level, 2);
    assert_eq!(r.todo.as_deref(), Some("TODO"));
}

#[test]
fn a_child_lands_under_the_selection_not_under_its_children() {
    // `Alpha` already has a child. The new one is Alpha's, not its
    // sibling's — "child" is measured from the cursor.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", true, true, "Gamma");
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
fn c_s_ret_makes_a_sibling_above() {
    // `+org/insert-item-above`, the half that did not exist: adding a
    // heading above meant adding one below and moving it.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(1, &sh); // Alpha child
    app.on_key(&mut sh, "shift-enter", true, false, None);
    assert_eq!(app.surface(), ModalSurface::AddSibling);
    for c in "Gamma".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    let src: String = sh.vault.iter().map(|(_, doc)| doc.source()).collect();
    let made = src.find("** Gamma").expect("no heading made");
    let child = src
        .find("** Alpha child")
        .expect("Alpha child went missing");
    assert!(made < child, "it landed below, not above:\n{src}");
}

#[test]
fn every_mode_binds_all_of_them() {
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
            ("C-RET", "add-heading"),
            ("C-S-RET", "add-heading-above"),
            ("C-M-RET", "add-child-heading"),
            ("C-M-S-RET", "add-todo-child-heading"),
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
fn every_chord_reaches_the_prompt_in_every_mode() {
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
            ("enter", true, true),
            ("shift-enter", true, true),
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

// === The prompt says what it is about to make ===
//
// Reported 2026-08-02: "shift+a or A: prompt should indicate that a
// TODO item will be added as a sibling."
//
// One prompt serves all four chords plus `a`, and it said "add" for
// every one of them — so having pressed `A` rather than `a`, or `C-RET`
// rather than `M-RET`, there was nothing on screen to confirm it.

#[test]
fn the_prompt_names_a_plain_sibling() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "a", false, false, Some('a'));
    assert_eq!(app.new_heading_label(), "sibling");
}

#[test]
fn the_prompt_says_todo_for_the_shifted_chord() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "A", false, false, Some('A'));
    assert_eq!(app.new_heading_label(), "sibling TODO");
}

#[test]
fn it_says_child_for_the_ctrl_meta_chords() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "enter", true, true, None); // C-M-RET
    assert_eq!(app.new_heading_label(), "child");
}

#[test]
fn and_above_for_the_shifted_ctrl_chord() {
    // The prompt is the only thing on screen that says which of the
    // six chords you pressed, so the direction has to reach it.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "shift-enter", true, false, None); // C-S-RET
    assert_eq!(app.new_heading_label(), "sibling above");
}

#[test]
fn and_child_todo_for_all_three_modifiers() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "shift-enter", true, true, None); // C-M-S-RET
    assert_eq!(app.new_heading_label(), "child TODO");
}

#[test]
fn the_status_line_says_the_same_thing() {
    // The status already said it; the label is the same answer where
    // the eye actually is, next to what you are typing.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "A", false, false, Some('A'));
    assert!(
        app.status().contains(app.new_heading_label()),
        "{} vs {}",
        app.status(),
        app.new_heading_label()
    );
}

// === Where the cursor goes after adding one ===
//
// Asked 2026-08-02: "should after adding a sibling (a) the selection be
// on the new element or the sibling where it has been added?"
//
// Both, and the answer already exists: capture settled exactly this
// question with `Enter` to go to the new item and `C-Enter` to stay put
// and keep filing. A second prompt with a different rule would mean
// remembering which prompt you are in, so this one follows.

#[test]
fn enter_puts_the_cursor_on_what_you_just_made() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", false, true, "Gamma");
    assert_eq!(app.detail(&sh).expect("detail").title, "Gamma");
}

#[test]
fn ctrl_enter_leaves_it_where_it_was() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.on_key(&mut sh, "enter", false, true, None); // M-RET opens the prompt
    for c in "Gamma".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", true, false, None); // C-Enter: file it and stay
    assert_eq!(
        app.detail(&sh).expect("detail").title,
        "Alpha",
        "still on the headline it was added under"
    );
    assert!(
        app.rows(&sh).iter().any(|r| r.title == "Gamma"),
        "and it was still added"
    );
}

#[test]
fn the_same_rule_holds_for_a_child() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    new_heading(&mut app, &mut sh, "enter", true, false, "Kid");
    assert_eq!(app.detail(&sh).expect("detail").title, "Kid");
}
