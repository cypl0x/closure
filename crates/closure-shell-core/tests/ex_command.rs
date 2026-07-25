//! The `:` command line.
//!
//! `:` opened the fuzzy palette, which is useful but is not what a vim
//! user's hands do: `:w` typed into a fuzzy filter matches nothing and
//! silently does nothing. So `:` now opens a real ex line that
//! understands the small set of commands muscle memory reaches for,
//! and falls through to the palette's command set for everything else
//! — a superset of what it replaced.
//!
//! One honesty requirement runs through this: closure writes through
//! the kernel on every edit (I8), so a vault is never unsaved. `:w`
//! must say that rather than pretend to have done something.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQEXCMD00000000000001\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Vim))
}

fn type_str(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, "x", false, false, Some(c));
    }
}

/// Open the ex line and run `cmd`.
fn ex(app: &mut ModalApp, shell: &mut Shell, cmd: &str) {
    app.on_key(shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex, "`:` opens the ex line");
    type_str(app, shell, cmd);
    app.on_key(shell, "enter", false, false, None);
}

#[test]
fn colon_opens_an_empty_ex_line() {
    let (_d, mut shell, mut app) = fixture();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex);
    assert_eq!(app.ex_buffer(), "");
    type_str(&mut app, &mut shell, "wq");
    assert_eq!(app.ex_buffer(), "wq");
}

#[test]
fn escape_abandons_the_ex_line() {
    let (_d, mut shell, mut app) = fixture();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    type_str(&mut app, &mut shell, "q");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!app.should_quit(), "escaping must not run the command");
    assert_eq!(app.ex_buffer(), "", "and the buffer is cleared");
}

#[test]
fn backspace_edits_the_ex_line() {
    let (_d, mut shell, mut app) = fixture();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    type_str(&mut app, &mut shell, "wq");
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.ex_buffer(), "w");
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.ex_buffer(), "");
    // Backspacing past the start closes it, like the `/` menu.
    app.on_key(&mut shell, "backspace", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn q_quits() {
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "q");
    assert!(app.should_quit());
}

#[test]
fn q_bang_quits_too() {
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "q!");
    assert!(app.should_quit());
}

#[test]
fn w_reports_that_the_vault_is_already_written() {
    // Nothing to save is the truth, and saying "written" would be a
    // lie about a write that never happened.
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "w");
    assert!(!app.should_quit(), ":w does not quit");
    assert_eq!(app.surface(), ModalSurface::Browse);
    let status = app.status();
    assert!(
        status.contains("every edit") || status.contains("already"),
        "the status must explain, not just say ok: {status}"
    );
}

#[test]
fn wq_and_x_both_write_and_quit() {
    for cmd in ["wq", "x"] {
        let (_d, mut shell, mut app) = fixture();
        ex(&mut app, &mut shell, cmd);
        assert!(app.should_quit(), ":{cmd} must quit");
    }
}

#[test]
fn w_commits_the_body_when_the_editor_is_open() {
    // Here `:w` genuinely has something to do: the editor buffer is not
    // in the vault until it is committed.
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    type_str(&mut app, &mut shell, "typed in the editor");
    // Esc leaves INSERT so `:` is a command, not text.
    app.on_key(&mut shell, "escape", false, false, None);
    ex(&mut app, &mut shell, "w");
    assert_eq!(app.surface(), ModalSurface::Browse, "the editor closed");
    assert!(
        app.detail(&shell)
            .expect("detail")
            .body
            .contains("typed in the editor"),
        "…and the body reached the vault"
    );
}

#[test]
fn an_unknown_ex_command_falls_through_to_the_command_set() {
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "agenda");
    assert_eq!(
        app.surface(),
        ModalSurface::Agenda,
        "a command name runs that command"
    );
}

#[test]
fn a_nonsense_ex_command_says_so_and_changes_nothing() {
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "zzzznotacommand");
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(
        app.status().contains("zzzznotacommand"),
        "name the thing it could not find: {}",
        app.status()
    );
    assert!(!app.should_quit());
}

#[test]
fn an_empty_ex_line_just_closes() {
    let (_d, mut shell, mut app) = fixture();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!app.should_quit());
}

#[test]
fn colon_inside_the_body_editor_is_text_not_a_command() {
    // In INSERT you are typing prose; `:PROPERTIES:` and `12:30` must
    // not open a command line.
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    type_str(&mut app, &mut shell, "at 12:30");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(app.body_buffer(), "at 12:30");
}

#[test]
fn colon_in_editor_normal_mode_opens_the_ex_line() {
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "escape", false, false, None);
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex, "vim reflex, vim result");
}
