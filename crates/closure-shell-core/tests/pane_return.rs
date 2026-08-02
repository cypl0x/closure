//! "messages command will throw me back from the editor view to the
//! outline tree view — This is like the =n=th time. Do I have to
//! experience this for every new command?"
//!
//! No. That sentence is the item: the bug is a class, not an instance,
//! and it had already been reported three times — `messages`,
//! `undo-history`, and "graph … Being in the editor view you get
//! thrown into the outline view as well".
//!
//! Every one of those opens a *pane* over whatever you were doing. The
//! buffer is never closed — the text is still in it — but the way back
//! was `go_home`, which answers "the outline" for anyone whose view is
//! the clickable one. So the note you were writing vanished from the
//! screen while still being open, and `Esc` returned you to the list.
//!
//! A pane opened over a buffer goes back to that buffer. Written once,
//! and tested over the whole command registry rather than over the
//! three commands that happen to have been reported, so the fourth one
//! is right before anybody has to find it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQPANE00000000000001
:END:
the body I was writing

#+begin_src sh
echo hi
#+end_src
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQPANE00000000000001"));
    (dir, shell, app)
}

/// A body buffer, open, with text in it.
fn editing() -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (dir, shell, app)
}

#[test]
fn messages_comes_back_to_the_buffer() {
    // The report, verbatim.
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "messages");
    assert_eq!(app.surface(), ModalSurface::Messages);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "{}", app.status());
    assert!(app.body_buffer().contains("the body I was writing"));
}

#[test]
fn undo_history_comes_back_too() {
    // "calling undo-history in editor mode instantly throws you into
    // the outline mode".
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "undo-history");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn the_graph_comes_back_as_well() {
    // "graph still uses the old view component … Being in the editor
    // view you get thrown into the outline view as well".
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "graph");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn every_pane_in_the_registry_comes_back_to_the_buffer() {
    // The whole point of the item: not the three that were reported,
    // but every command that opens a pane — including the ones nobody
    // has tried yet from inside a buffer.
    let mut checked = 0;
    for command in closure_shell_core::palette_command_names() {
        let (_d, mut shell, mut app) = editing();
        app.run(&mut shell, command);
        let opened = app.surface();
        // Only the commands that actually opened a pane. The rest are
        // edits, motions and the buffer's own verbs — and `Browse` is
        // *home*, which the commands that deliberately leave a buffer
        // (`reload-shell`, the view toggles) are asking for.
        if opened.is_editor() || opened == ModalSurface::Browse {
            continue;
        }
        checked += 1;
        app.on_key(&mut shell, "escape", false, false, None);
        assert_eq!(
            app.surface(),
            ModalSurface::EditBody,
            "`{command}` opened {opened:?} and Esc did not come back"
        );
    }
    assert!(checked > 5, "only {checked} pane commands exercised");
}

#[test]
fn a_pane_opened_from_the_outline_still_goes_back_to_the_outline() {
    // The other half: nothing may start returning to a buffer that was
    // never open.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "messages");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn closing_the_buffer_from_a_pane_does_not_strand_the_return() {
    // A pane is open over a buffer, and the buffer is closed from
    // inside it. There is nothing to go back to, and going back
    // anyway would open a buffer the user had just dismissed.
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "messages");
    app.run(&mut shell, "discard-edit");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_ne!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn a_second_pane_does_not_forget_the_buffer_under_the_first() {
    // Panes stack in practice: messages, then the palette over it.
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "messages");
    app.run(&mut shell, "palette");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "{}", app.status());
}
