//! "sync with system clipboard (two way)" and "having something on the
//! system clipboard and being able to use p in Vim/helix/Doom mode to
//! paste would be nice".
//!
//! `C-c` and `C-v` reached the system clipboard already. vim's `y` and
//! `p` did not: they used an internal register, so copying a line in
//! closure and pasting it into a browser needed a second gesture, and
//! copying a URL in a browser could not be pasted with the key a vim
//! user's hands already know.
//!
//! The core is dep-free and cannot touch a system clipboard, so the
//! shell drives it — but the shell needs a seam that says *when*
//! something changed, or it would either write the clipboard on every
//! keystroke or fight whatever else is on it. A generation counter is
//! that seam: the register says it moved, and the mirror answers.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQCLIP00000000000001\n:END:\nfirst line\nsecond line\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQCLIP00000000000001"));
    app.run(&mut shell, "edit-body");
    app.body_click(0, 0);
    (dir, shell, app)
}

/// `yy` — yank the line the caret is on.
fn yank_line(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "y", false, false, Some('y'));
    app.on_key(shell, "y", false, false, Some('y'));
}

#[test]
fn yanking_says_that_the_register_moved() {
    let (_d, mut shell, mut app) = editing();
    let before = app.register_generation();
    yank_line(&mut app, &mut shell);
    assert_ne!(app.register_generation(), before, "nothing to mirror");
}

#[test]
fn the_register_holds_what_was_yanked() {
    let (_d, mut shell, mut app) = editing();
    yank_line(&mut app, &mut shell);
    assert!(
        app.register_text().contains("first line"),
        "{:?}",
        app.register_text()
    );
}

#[test]
fn what_the_system_clipboard_holds_can_be_put_in_the_register() {
    // The other direction: a URL copied in a browser, pasted with `p`.
    let (_d, mut shell, mut app) = editing();
    app.set_register_from_clipboard("https://example.invalid/");
    app.on_key(&mut shell, "p", false, false, Some('p'));
    assert!(
        app.body_buffer().contains("https://example.invalid/"),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn pushing_the_same_text_again_is_not_a_change() {
    // The mirror runs on every key. Without this it would write the
    // clipboard forever and fight anything else that owns it.
    let (_d, _sh, mut app) = editing();
    app.set_register_from_clipboard("same");
    let after_first = app.register_generation();
    app.set_register_from_clipboard("same");
    assert_eq!(app.register_generation(), after_first);
}

#[test]
fn the_clipboard_does_not_overwrite_a_fresher_yank() {
    // Both directions run on every key, so the loser has to be the one
    // that did not change: pushing what the register already holds
    // must not count as the clipboard having moved.
    let (_d, mut shell, mut app) = editing();
    yank_line(&mut app, &mut shell);
    let yanked = app.register_text().to_owned();
    app.set_register_from_clipboard(&yanked);
    assert_eq!(app.register_text(), yanked);
}

#[test]
fn an_empty_clipboard_is_not_worth_taking() {
    let (_d, mut shell, mut app) = editing();
    yank_line(&mut app, &mut shell);
    let yanked = app.register_text().to_owned();
    app.set_register_from_clipboard("");
    assert_eq!(app.register_text(), yanked, "an empty clipboard won");
}
