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
fn w_writes_the_body_and_stays_in_the_editor() {
    // Here `:w` genuinely has something to do: the editor buffer is not
    // in the vault until it is written. Contract revised 2026-07-28 on
    // the user's report — `:w` in every vi means "write and carry on",
    // and closing the buffer it had just saved was the complaint.
    // `:wq` is the one that leaves.
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    // The buffer opens in NORMAL, so typing starts with `i`.
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_str(&mut app, &mut shell, "typed in the editor");
    // Esc leaves INSERT so `:` is a command, not text.
    app.on_key(&mut shell, "escape", false, false, None);
    ex(&mut app, &mut shell, "w");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "the editor stayed open"
    );
    assert!(!app.body_dirty(), "and the buffer counts as saved");
    // Leaving is `:wq`'s job; the body must be in the vault either way.
    ex(&mut app, &mut shell, "wq");
    assert_eq!(app.surface(), ModalSurface::Browse);
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
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_str(&mut app, &mut shell, "at 12:30");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(app.body_buffer(), "at 12:30");
}

#[test]
fn colon_in_editor_normal_mode_opens_the_ex_line() {
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex, "vim reflex, vim result");
}

// === `:` from inside a buffer hands the buffer back ===
//
// Reported 2026-08-01: "in the body editor the `:` commands will
// instantly bring you back to the tree and preview view". `run_ex` set
// the surface to Browse before it looked at the line, and left it to
// each arm to climb back — so every line that did not think to (a bare
// `:`, a typo, a command with nothing to do with the buffer) closed the
// buffer you were typing in. The `:` line hands back what it floated
// over, the way the palette does; only the lines that mean to leave
// leave.

/// The body editor, in NORMAL, with `text` typed into it.
fn in_the_buffer(app: &mut ModalApp, shell: &mut Shell, text: &str) {
    app.select(0, shell);
    app.run(shell, "edit-body");
    app.on_key(shell, "i", false, false, Some('i'));
    type_str(app, shell, text);
    app.on_key(shell, "escape", false, false, None);
}

#[test]
fn an_empty_ex_line_hands_the_buffer_back() {
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    app.on_key(&mut shell, "x", false, false, Some(':'));
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
    assert_eq!(app.body_buffer(), "half a paragraph");
}

#[test]
fn a_nonsense_ex_command_does_not_close_the_buffer() {
    // A typo is the most likely thing to arrive on this line, and
    // losing your place in the buffer is a heavy price for one.
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    ex(&mut app, &mut shell, "zzzznotacommand");
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
    assert!(app.status().contains("zzzznotacommand"), "{}", app.status());
    assert_eq!(app.body_buffer(), "half a paragraph");
}

#[test]
fn escape_on_the_ex_line_hands_the_buffer_back() {
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    app.on_key(&mut shell, "x", false, false, Some(':'));
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn a_command_with_nothing_to_do_with_the_buffer_leaves_it_open() {
    // `:zoom-in` is the same command the palette runs, and the palette
    // hands the buffer back afterwards. Two spellings of one command
    // must not disagree about whether your buffer survives it (I4).
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    ex(&mut app, &mut shell, "zoom-in");
    assert!(app.zoom() > 1.0, "the command ran");
    assert_eq!(app.surface(), ModalSurface::EditBody, "and gave it back");
    // The command wrote the buffer on the way through, and a write is
    // followed by a read-back so the ids the vault stamps end up on
    // screen. So what is in the buffer afterwards is what the *file*
    // holds — org bodies are newline-terminated — rather than the
    // unterminated line that was typed.
    assert_eq!(app.body_buffer(), "half a paragraph\n");
}

#[test]
fn a_command_that_opens_another_surface_writes_the_buffer_first() {
    // Leaving is the command's decision, not the ex line's — but
    // unwritten text must not leave with it.
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "typed then walked away from");
    ex(&mut app, &mut shell, "agenda");
    assert_eq!(app.surface(), ModalSurface::Agenda, "the command decided");
    app.select(0, &shell);
    assert!(
        app.detail(&shell)
            .expect("detail")
            .body
            .contains("typed then walked away from"),
        "the buffer reached the vault on the way out"
    );
}

// === The `:` line floats over the buffer, it does not replace it ===
//
// Reported 2026-08-01, twice, and named top priority: "in Editor view
// pressing `:` will forcefully reset to the tree view (left) + detail
// view (right)" — "this behavior is quite annoying, because everything
// is shifting and I always get confused".
//
// The command line already returned you to the buffer when it closed
// (`run_ex`), but while it was *open* the window had nothing to paint:
// the surface was `Ex`, which is not an editor, so the shell fell back
// to the outline and the whole layout jumped. A command line is a bar
// at the bottom of the buffer you are in — vim's is, Emacs' minibuffer
// is, and closure's palette already is.

#[test]
fn the_buffer_stays_on_screen_while_the_ex_line_is_open() {
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    let under = app.surface();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex, "the line is open");
    assert_eq!(
        app.surface_beneath(),
        under,
        "and the buffer is still what the window paints"
    );
}

#[test]
fn the_ex_line_over_the_outline_still_paints_the_outline() {
    let (_d, mut shell, mut app) = fixture();
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface_beneath(), ModalSurface::Browse);
}

#[test]
fn the_full_window_editor_keeps_its_shape_too() {
    // `view = editor` is the shape the report was written from: the
    // file buffer fills the window and there is no tree to fall back
    // to without the layout jumping.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-view");
    let under = app.surface_beneath();
    assert!(under.is_editor(), "the editor view is a buffer: {under:?}");
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex);
    assert_eq!(app.surface_beneath(), under, "nothing shifted");
}

#[test]
fn escaping_the_ex_line_leaves_the_buffer_exactly_where_it_was() {
    let (_d, mut shell, mut app) = fixture();
    in_the_buffer(&mut app, &mut shell, "half a paragraph");
    let (surface, cursor) = (app.surface(), app.body_cursor());
    app.on_key(&mut shell, "x", false, false, Some(':'));
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), surface);
    assert_eq!(app.body_cursor(), cursor);
}

#[test]
fn colon_opens_the_ex_line_in_the_full_window_editor() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-view");
    app.on_key(&mut shell, "x", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Ex, "vim reflex, vim result");
}

#[test]
fn w_writes_the_file_buffer_and_stays_in_it() {
    // A `:` line that opens but whose `w` says "nothing to save" is
    // worse than one that never opened.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-view");
    assert!(app.surface().is_editor(), "the file buffer is open");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_str(&mut app, &mut shell, "* Typed into the file\n");
    app.on_key(&mut shell, "escape", false, false, None);
    ex(&mut app, &mut shell, "w");
    assert!(app.surface().is_editor(), "still in the buffer");
    assert!(
        app.status().contains("wrote"),
        "and it wrote something: {}",
        app.status()
    );
    assert!(!app.body_dirty(), "the buffer counts as saved");
}

#[test]
fn wq_writes_the_file_buffer_and_closes_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-view");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_str(&mut app, &mut shell, "* Another one\n");
    app.on_key(&mut shell, "escape", false, false, None);
    ex(&mut app, &mut shell, "wq");
    assert!(!app.surface().is_editor(), "the buffer closed");
    assert!(!app.should_quit(), "closing a buffer is not quitting");
}
