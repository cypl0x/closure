//! "weird selection shadow in command palette — If you look on the
//! bottom right of the command palette there is some…"
//!
//! It is not a shadow. Below the floating picker's own footer, the
//! screenshot shows a row of the *old* undo-history pane — `└ ○ remove
//! subtree` — still being painted underneath it.
//!
//! Seven surfaces are drawn as floating pickers, and `surface_beneath`
//! told the pane behind to draw something else for exactly two of them
//! (the palette and the `:` line). For the other five the pane painted
//! the picker's own list *and* the picker floated over it, so you saw
//! the same data twice, once clipped by a panel edge.
//!
//! The same fact is behind two more reports: the pane behind is live,
//! so its scrollbar takes the wheel ("it scrolls both the command
//! palette and the headlines outline tree view").

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

/// The commands that open a floating picker, and the surface each one
/// lands on.
const PICKERS: [(&str, ModalSurface); 6] = [
    ("palette", ModalSurface::Palette),
    ("list-buffers", ModalSurface::Buffers),
    ("recent-files", ModalSurface::Files),
    ("list-headlines", ModalSurface::Headlines),
    ("list-blocks", ModalSurface::Blocks),
    ("messages", ModalSurface::Messages),
];

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQBENEATH0000000001\n:END:\nbody\n\
         \n#+begin_src sh\necho hi\n#+end_src\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQBENEATH0000000001"));
    (dir, shell, app)
}

#[test]
fn a_picker_never_paints_itself_underneath_itself() {
    // The ghost row: the pane drew the picker's own list and the picker
    // floated over it.
    for (command, surface) in PICKERS {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        assert_eq!(app.surface(), surface, "{command}");
        assert!(
            app.picker_view(&shell).is_some(),
            "{command} is drawn as a floating picker"
        );
        assert_ne!(
            app.surface_beneath(),
            surface,
            "{command} paints its own list behind its own panel"
        );
    }
}

#[test]
fn what_is_behind_a_picker_is_where_it_was_opened_from() {
    for (command, _) in PICKERS {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        assert_eq!(
            app.surface_beneath(),
            ModalSurface::Browse,
            "{command} from the outline"
        );
    }
}

#[test]
fn a_picker_opened_over_a_buffer_shows_the_buffer_behind_it() {
    for (command, _) in PICKERS {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, "edit-body");
        app.run(&mut shell, command);
        assert_eq!(
            app.surface_beneath(),
            ModalSurface::EditBody,
            "{command} over a body buffer"
        );
    }
}

#[test]
fn the_undo_history_is_one_of_them() {
    // The picker in the screenshot.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "undo-history");
    assert_eq!(app.surface(), ModalSurface::UndoHistory);
    assert!(app.picker_view(&shell).is_some());
    assert_ne!(app.surface_beneath(), ModalSurface::UndoHistory);
}

#[test]
fn a_pane_that_is_not_a_picker_still_draws_itself() {
    // Agenda and the graph are panes, not floating pickers: they *are*
    // the pane, and telling the pane to draw something else would
    // leave them blank.
    for (command, surface) in [
        ("agenda", ModalSurface::Agenda),
        ("backlinks", ModalSurface::Backlinks),
    ] {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        assert_eq!(app.surface(), surface, "{command}");
        assert_eq!(app.surface_beneath(), surface, "{command}");
    }
}

#[test]
fn the_two_lists_of_floating_pickers_agree() {
    // `is_floating_picker` and `picker_view` are two answers to one
    // question, and two answers to one question is how five surfaces
    // came to paint themselves twice. This holds them together.
    let (_d, mut shell, mut app) = fixture();
    for (command, surface) in PICKERS.iter().chain(&[
        ("undo-history", ModalSurface::UndoHistory),
        ("agenda", ModalSurface::Agenda),
        ("backlinks", ModalSurface::Backlinks),
        ("graph", ModalSurface::Graph),
    ]) {
        app.run(&mut shell, "escape-to-browse");
        app.run(&mut shell, command);
        if app.surface() != *surface {
            continue; // the command declined to open here
        }
        assert_eq!(
            app.picker_view(&shell).is_some(),
            app.surface_beneath() != *surface,
            "{command}: floats={} but beneath={:?}",
            app.picker_view(&shell).is_some(),
            app.surface_beneath()
        );
    }
}
