//! Two ways to work, and the switch between them.
//!
//! The GUI is a Notion-shaped, clickable outline: rows, a detail pane, a
//! rail. That is the right shape for a mouse and the wrong one for
//! someone who lives in Doom — where the file *is* the interface, the
//! whole frame is a buffer, and the outline is something you fold rather
//! than something you click.
//!
//! So the shell has both. Every window opens on the outline — that is
//! where the rail and every affordance live — and `toggle-view` (`g v`)
//! switches at any time. Switching input mode switches the view with
//! it, and `view = editor` in the vault config starts there.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell, ViewMode};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "* One\n:PROPERTIES:\n:ID: 01HQVIEW00000000000000001\n:END:\nbody one\n\
                   * Two\n:PROPERTIES:\n:ID: 01HQVIEW00000000000000002\n:END:\nbody two\n";

fn fixture(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

fn feed(app: &mut ModalApp, shell: &mut Shell, keys: &str) {
    for c in keys.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

// === which view you start in ===

#[test]
fn every_window_opens_on_the_outline() {
    // Even in Doom mode. The outline is where the rail and every
    // affordance live; a shell that opened straight into a raw file
    // buffer would hide the app from anyone who had not asked for that.
    // `view = editor` in the vault config, `g v`, or switching input
    // mode are the three ways to ask.
    for mode in [
        InputMode::Vim,
        InputMode::Doom,
        InputMode::Helix,
        InputMode::Notion,
        InputMode::Emacs,
    ] {
        let (_d, _sh, app) = fixture(mode);
        assert_eq!(app.view_mode(), ViewMode::Clickable, "{mode:?}");
    }
}

#[test]
fn the_view_a_mode_belongs_in_is_declared() {
    for mode in [InputMode::Vim, InputMode::Doom, InputMode::Helix] {
        assert_eq!(ViewMode::for_input(mode), ViewMode::Editor, "{mode:?}");
    }
    for mode in [InputMode::Notion, InputMode::Emacs] {
        assert_eq!(ViewMode::for_input(mode), ViewMode::Clickable, "{mode:?}");
    }
}

#[test]
fn a_config_can_ask_to_start_in_the_editor_view() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.set_view(ViewMode::Editor, &shell);
    assert_eq!(app.surface(), ModalSurface::EditFile);
    assert_eq!(app.body_buffer(), SRC);
    app.set_view(ViewMode::Clickable, &shell);
    assert_eq!(app.surface(), ModalSurface::Browse);
    let _ = &mut shell;
}

#[test]
fn switching_the_input_mode_leaves_the_view_where_it_is() {
    // Contract revised 2026-08-02 on the user's report: "switching the
    // mode in the top left corner will show the full view body editor.
    // Disable this behavior, because the full view editor should be
    // toggled seperately by its own command."
    //
    // The view used to follow the mode — a mode with a NORMAL was taken
    // to want the file, one without it the rows. Clicking the mode chip
    // to try Vim then threw away the pane you were reading, which is a
    // large surprise for a small chord. The initial view still comes
    // from the configured mode ([`ViewMode::for_input`]); it is only
    // *switching* that no longer drags it along.
    let (_d, mut shell, mut app) = fixture(InputMode::Notion);
    assert_eq!(app.view_mode(), ViewMode::Clickable);
    for _ in 0..5 {
        app.run(&mut shell, "cycle-mode");
        assert_eq!(
            app.view_mode(),
            ViewMode::Clickable,
            "{:?}",
            app.input_mode()
        );
        assert_eq!(app.surface(), ModalSurface::Browse);
    }
}

// === the switch itself ===

#[test]
fn toggle_view_opens_the_file_as_a_buffer() {
    let (_d, mut shell, mut app) = fixture(InputMode::Notion);
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.view_mode(), ViewMode::Editor);
    assert_eq!(app.surface(), ModalSurface::EditFile);
    assert_eq!(
        app.body_buffer(),
        SRC,
        "the whole file, exactly as it is on disk"
    );
}

#[test]
fn the_file_buffer_opens_in_normal_for_a_modal_mode() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.body_mode(), EditorMode::Normal);
    assert_eq!(app.body_cursor(), (0, 0));
}

#[test]
fn toggle_view_again_returns_to_the_outline() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.view_mode(), ViewMode::Clickable);
    assert_eq!(app.surface(), ModalSurface::Browse, "back to the rows");
}

#[test]
fn the_file_buffer_is_a_buffer_so_it_takes_the_window() {
    assert!(ModalSurface::EditFile.is_editor());
}

// === editing the file ===

#[test]
fn the_vim_grammar_works_on_the_whole_file() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    feed(&mut app, &mut shell, "jjdd"); // delete the `:PROPERTIES:` line? no: line 2
    assert!(
        app.body_buffer().lines().count() < SRC.lines().count(),
        "a line went: {:?}",
        app.body_buffer()
    );
}

#[test]
fn saving_the_buffer_writes_the_file() {
    let (dir, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    // `A` appends at the end of the first line, so the edit is visible
    // without depending on where the cursor starts.
    feed(&mut app, &mut shell, "A!");
    app.on_key(&mut shell, "enter", true, false, None); // C-Enter saves
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.starts_with("* One!"), "written: {on_disk:?}");
    assert_eq!(
        app.surface(),
        ModalSurface::EditFile,
        "saving a file keeps you in it, the way :w does"
    );
}

#[test]
fn a_saved_file_edit_reaches_the_outline() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    feed(&mut app, &mut shell, "A!");
    app.on_key(&mut shell, "enter", true, false, None);
    app.run(&mut shell, "toggle-view");
    let titles: Vec<String> = app.rows(&shell).iter().map(|r| r.title.clone()).collect();
    assert_eq!(titles, vec!["One!".to_owned(), "Two".to_owned()]);
}

#[test]
fn escape_leaves_the_file_buffer_without_writing_it() {
    let (dir, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    feed(&mut app, &mut shell, "A!");
    app.on_key(&mut shell, "escape", false, false, None); // INSERT -> NORMAL
    app.on_key(&mut shell, "escape", false, false, None); // NORMAL -> out
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(
        fs::read_to_string(dir.path().join("notes.org")).expect("read"),
        SRC,
        "an abandoned buffer changes nothing"
    );
}

#[test]
fn the_buffer_is_named_after_the_file_it_is() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "toggle-view");
    let name = app.buffer_name(&shell).expect("a name");
    assert!(name.contains("notes.org"), "the file: {name}");
}
