//! "M-x command-palette and the generic filter popup view should
//! autocomplete the current selection with the tab key."
//!
//! What vertico and readline both do: Tab does not choose, it *types*
//! for you. You narrow to a handful, Tab fills the field with the one
//! under the cursor, and you are left looking at it able to keep
//! going — add an argument, narrow further, or press Enter.
//!
//! closure's Tab reached the field's own handler, where it did
//! nothing at all in a picker.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    let mut org = String::new();
    for i in 0..12 {
        use std::fmt::Write as _;
        let _ = write!(
            org,
            "* Headline number {i:02}\n:PROPERTIES:\n:ID: 01PICKTAB{i:015}\n:END:\n"
        );
    }
    std::fs::write(dir.path().join("notes.org"), org).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn typed(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn tab_fills_the_field_with_the_selected_command() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "palette");
    typed(&mut app, &mut shell, "add-chi");
    let picked = app
        .picker_view(&shell)
        .expect("a picker")
        .rows
        .first()
        .expect("something matched")
        .label
        .clone();
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(
        app.field_buffer(),
        picked,
        "Tab did not complete the selection"
    );
}

#[test]
fn tab_completes_whichever_row_the_cursor_is_on() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "palette");
    typed(&mut app, &mut shell, "add");
    app.on_key(&mut shell, "down", false, false, None);
    let picked = {
        let view = app.picker_view(&shell).expect("a picker");
        view.rows[view.cursor].label.clone()
    };
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(app.field_buffer(), picked);
}

#[test]
fn tab_does_not_run_it() {
    // The difference from Enter, and the whole point: you are left in
    // the picker looking at the completed text.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "palette");
    typed(&mut app, &mut shell, "add-chi");
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(
        app.surface(),
        closure_shell_core::ModalSurface::Palette,
        "Tab ran the command instead of completing it"
    );
}

#[test]
fn the_generic_picker_completes_too() {
    // "and the generic filter popup view": the headline picker is the
    // same view, so it gets the same key.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "list-headlines");
    typed(&mut app, &mut shell, "number 0");
    let picked = {
        let view = app.picker_view(&shell).expect("a picker");
        view.rows[view.cursor].label.clone()
    };
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(app.query(), picked, "Tab did not complete in the picker");
}

#[test]
fn tab_on_an_empty_list_leaves_the_text_alone() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "palette");
    typed(&mut app, &mut shell, "zzzznothing");
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(
        app.field_buffer(),
        "zzzznothing",
        "Tab threw away what was typed when nothing matched"
    );
}
