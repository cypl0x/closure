//! "Org headline keybindings for headline in editor shouldn't trigger
//! the 'capture' like input text. Instead should do the required
//! action inline in the editor."
//!
//! The screenshot is Doom, showing what it should be: the caret is in
//! an org buffer, `C-RET` puts `* ` on the next line, and you carry on
//! typing there. No prompt, no modal field — the buffer is the input.
//!
//! closure opened its `AddSibling` surface instead: a one-line field
//! floating over the buffer, which you type a title into and press
//! Enter, and which is the same shape as capture. In an *outline*
//! that is right — there is nowhere else to type. In a buffer there
//! is: you are already in a text editor, and the headline is a line of
//! text.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "\
* First
:PROPERTIES:
:ID: 01INLINE0000000000000001
:END:
body of first
** A child
:PROPERTIES:
:ID: 01INLINE0000000000000002
:END:
";

fn editing() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    assert!(app.surface().is_editor(), "the buffer is open");
    (dir, shell, app)
}

/// Put the caret on the line that starts with `needle`.
///
/// The buffer is the note's *body* — its own headline line is not in
/// it, which is why the level of "the headline you are on" is
/// sometimes a line in the buffer and sometimes the note itself.
fn caret_on(app: &mut ModalApp, shell: &mut Shell, needle: &str) {
    let line = app
        .body_buffer()
        .lines()
        .position(|l| l.starts_with(needle))
        .unwrap_or_else(|| panic!("no line starts with {needle:?}:\n{}", app.body_buffer()));
    // `j` in NORMAL, which is how you get down a line in this editor.
    for _ in 0..line {
        app.on_key(shell, "j", false, false, Some('j'));
    }
}

#[test]
fn a_new_heading_lands_in_the_buffer_not_in_a_prompt() {
    let (_d, mut shell, mut app) = editing();
    caret_on(&mut app, &mut shell, "body of first");
    app.run(&mut shell, "add-heading");
    assert!(
        app.surface().is_editor(),
        "it opened a field instead of editing the buffer: {:?}",
        app.surface()
    );
    assert_ne!(app.surface(), ModalSurface::AddSibling);
    let stars = app
        .body_buffer()
        .lines()
        .filter(|l| l.starts_with('*'))
        .count();
    assert_eq!(
        stars,
        2,
        "no headline was inserted into the buffer:\n{}",
        app.body_buffer()
    );
}

#[test]
fn the_caret_is_left_where_the_title_goes() {
    // Doom leaves you after the stars and the space, typing the title.
    let (_d, mut shell, mut app) = editing();
    caret_on(&mut app, &mut shell, "body of first");
    app.run(&mut shell, "add-heading");
    for c in "Typed straight in".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.body_buffer().contains("* Typed straight in"),
        "the caret was not left in the new headline:\n{}",
        app.body_buffer()
    );
}

#[test]
fn a_sibling_takes_the_level_of_the_headline_you_are_on() {
    let (_d, mut shell, mut app) = editing();
    caret_on(&mut app, &mut shell, "** A child");
    app.run(&mut shell, "add-heading");
    for c in "Second child".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.body_buffer().contains("** Second child"),
        "a sibling of a level-2 headline is level 2:\n{}",
        app.body_buffer()
    );
}

#[test]
fn a_child_heading_goes_one_level_deeper() {
    let (_d, mut shell, mut app) = editing();
    caret_on(&mut app, &mut shell, "body of first");
    app.run(&mut shell, "add-child-heading");
    for c in "Its child".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.body_buffer().contains("** Its child"),
        "a child of a level-1 headline is level 2:\n{}",
        app.body_buffer()
    );
}

#[test]
fn a_todo_heading_carries_the_keyword() {
    let (_d, mut shell, mut app) = editing();
    caret_on(&mut app, &mut shell, "body of first");
    app.run(&mut shell, "add-todo-heading");
    for c in "Needs doing".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.body_buffer().contains("* TODO Needs doing"),
        "the TODO keyword is missing:\n{}",
        app.body_buffer()
    );
}

#[test]
fn the_outline_still_gets_its_prompt() {
    // Outside a buffer there is nowhere to type but a field, and that
    // is what the field is for. This must not change.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.select(0, &shell);
    app.run(&mut shell, "add-heading");
    assert_eq!(
        app.surface(),
        ModalSurface::AddSibling,
        "the outline lost its new-heading prompt"
    );
}
