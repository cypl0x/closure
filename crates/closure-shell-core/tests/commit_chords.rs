//! "remove the C-ENTER chord for save & close. Replace it with C-c C-c."
//!   - C-c C-c like in org-edit-special
//!   - C-c C-k like in org-edit-special
//!
//! And the item behind it: `C-Enter` is what org binds a dozen table
//! and structure commands under, so a buffer that swallowed it for
//! "save and close" was standing on the chords the table work needs.
//!
//! org's own pair is `C-c C-c` to accept and `C-c C-k` to abandon —
//! the same two keys that leave an `org-edit-special` buffer, which is
//! exactly what a body editor is. They are commands now rather than a
//! branch inside the editor's key handler, so which-key lists them and
//! the buttons name them.
//!
//! They are deliberately *not* in the outline keymap: `C-c C-c` there
//! is org's "do the thing at point", which for a source block is
//! running it. One chord, two meanings by surface — org's own rule, and
//! only the surface can tell them apart.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn editing(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQCOMMIT0000000000001\n:END:\noriginal body\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(mode);
    assert!(app.select_by_id(&shell, "01HQCOMMIT0000000000001"));
    app.run(&mut shell, "edit-body");
    if matches!(mode, InputMode::Doom | InputMode::Vim | InputMode::Helix) {
        app.on_key(&mut shell, "i", false, false, Some('i'));
    }
    (dir, shell, app)
}

/// Type `s` into the open buffer.
fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

/// `C-c C-c`, one stroke at a time.
fn accept(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "c", true, false, None);
    app.on_key(shell, "c", true, false, None);
}

/// `C-c C-k`.
fn abandon(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "c", true, false, None);
    app.on_key(shell, "k", true, false, None);
}

#[test]
fn c_c_c_c_saves_and_closes() {
    let (dir, mut shell, mut app) = editing(InputMode::Doom);
    type_in(&mut app, &mut shell, "changed");
    accept(&mut app, &mut shell);

    assert_eq!(app.surface(), ModalSurface::Browse, "the buffer closed");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("changed"), "and it saved: {on_disk}");
}

#[test]
fn c_c_c_k_abandons_the_edit() {
    let (dir, mut shell, mut app) = editing(InputMode::Doom);
    type_in(&mut app, &mut shell, "thrown away");
    abandon(&mut app, &mut shell);

    assert_eq!(app.surface(), ModalSurface::Browse, "the buffer closed");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!on_disk.contains("thrown away"), "nothing saved: {on_disk}");
    assert!(on_disk.contains("original body"), "{on_disk}");
}

#[test]
fn ctrl_enter_no_longer_closes_the_buffer() {
    // The whole point: `C-Enter` is org's own prefix for a dozen table
    // and structure commands, and a buffer that eats it is standing on
    // the chords the table work needs.
    //
    // What it does instead has changed once. Asked for on 2026-08-04
    // ("research the Doom Emacs keybindings for quick header creation
    // in the editor"), it opened the new-headline prompt over the
    // buffer. Asked for again on 2026-08-09 — "Org headline
    // keybindings for headline in editor shouldn't trigger the
    // 'capture' like input text. Instead should do the required action
    // inline in the editor", with a screenshot of Doom putting `* ` on
    // the next line and `-- INSERT --` along the bottom — it does that.
    //
    // Either way the buffer is still there, which is what this test
    // guards. It is more true now than it was: there is no prompt to
    // abandon, and what you typed never left the screen.
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    type_in(&mut app, &mut shell, "still here");
    app.on_key(&mut shell, "enter", true, false, None);
    assert!(
        app.surface().is_editor(),
        "C-Enter left the buffer: {:?}",
        app.surface()
    );
    assert_ne!(
        app.surface(),
        ModalSurface::AddSibling,
        "C-Enter opened a field over the buffer instead of editing it"
    );
    assert!(
        app.body_buffer().contains("still here"),
        "the buffer lost what was typed into it: {:?}",
        app.body_buffer()
    );
    assert!(
        app.body_buffer().lines().any(|l| l.starts_with('*')),
        "no headline was inserted into the buffer: {:?}",
        app.body_buffer()
    );
}

#[test]
fn both_chords_work_in_every_mode() {
    // Not in the outline keymap, deliberately: `C-c C-c` there is org's
    // "do the thing at point", which for a source block is running it.
    // One chord, two meanings by surface — org's own rule, and only the
    // surface can tell them apart. So the editor resolves them, and
    // what has to hold is that pressing them works.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let (_d, mut shell, mut app) = editing(mode);
        type_in(&mut app, &mut shell, "typed");
        accept(&mut app, &mut shell);
        assert_eq!(app.surface(), ModalSurface::Browse, "{mode:?} accept");

        let (_d2, mut sh2, mut app2) = editing(mode);
        type_in(&mut app2, &mut sh2, "typed");
        abandon(&mut app2, &mut sh2);
        assert_eq!(app2.surface(), ModalSurface::Browse, "{mode:?} abandon");
    }
}

#[test]
fn a_mode_with_no_normal_can_use_them_too() {
    // The two modes that never leave INSERT are exactly the ones that
    // could not reach `:q!`, so this is where the pair matters most.
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (dir, mut shell, mut app) = editing(mode);
        type_in(&mut app, &mut shell, "kept");
        accept(&mut app, &mut shell);
        assert_eq!(app.surface(), ModalSurface::Browse, "{mode:?}");
        let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
        assert!(on_disk.contains("kept"), "{mode:?}: {on_disk}");
    }
}

#[test]
fn the_buttons_name_the_new_chords() {
    for mode in [InputMode::Doom, InputMode::Notion] {
        let (_d, _sh, app) = editing(mode);
        let actions = app.buffer_actions();
        let accept = actions
            .iter()
            .find(|(_, cmd, _)| *cmd == "commit-edit")
            .expect("an accept action");
        let discard = actions
            .iter()
            .find(|(_, cmd, _)| *cmd == "discard-edit")
            .expect("a discard action");
        assert_eq!(accept.2.as_deref(), Some("C-c C-c"), "{mode:?}");
        assert_eq!(discard.2.as_deref(), Some("C-c C-k"), "{mode:?}");
    }
}

#[test]
fn a_half_typed_prefix_does_not_eat_the_next_key() {
    // `C-c` alone is a prefix; a `C-c` followed by something unbound
    // must leave the buffer as it was rather than swallowing a stroke.
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    let before = app.body_buffer().to_owned();
    app.on_key(&mut shell, "c", true, false, None);
    app.on_key(&mut shell, "z", true, false, None);
    assert!(app.surface().is_editor(), "still editing");
    assert_eq!(app.body_buffer(), before, "and the text is untouched");
}
