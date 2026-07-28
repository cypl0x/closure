//! Leaving the body editor without losing what is in it.
//!
//! Two ways out of a buffer both threw it away. `Esc` on a quiet
//! Normal surface cleared the buffer and returned to the outline —
//! the paragraph you had just typed was gone, with no prompt and no
//! undo. And `:w`, which in every vi ever written means "write and
//! carry on", wrote the buffer *and closed the editor*, because the
//! ex line returned to Browse before running the command.
//!
//! The rule here: nothing silently discards a modified buffer. `Esc`
//! on an unchanged one still leaves (peeking at a body must stay
//! cheap); on a modified one it refuses and says what saves and what
//! discards. `:w` writes and stays. `:wq` writes and leaves. `:q!`
//! is how you say you meant it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "* Note\n:PROPERTIES:\n:ID: 01HQEXIT00000000000000001\n:END:\nOriginal body.\n";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The editor open on the note, in NORMAL.
fn editing() -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (d, sh, app)
}

/// Type `text` at the end of the buffer and return to NORMAL.
fn append(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    app.on_key(sh, "A", false, false, Some('A'));
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(sh, "escape", false, false, None);
    assert_eq!(app.body_mode(), EditorMode::Normal);
}

#[test]
fn escape_still_leaves_an_untouched_buffer() {
    // Opening a body to read it and pressing Esc must stay free.
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn escape_refuses_to_throw_away_a_modified_buffer() {
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "the buffer is still open"
    );
    assert!(
        app.body_buffer().contains("and more"),
        "and still holds the text: {:?}",
        app.body_buffer()
    );
    let status = app.status();
    assert!(
        status.contains("C-Enter") || status.contains(":w"),
        "and says how to save: {status}"
    );
}

#[test]
fn escape_in_insert_only_leaves_insert() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.body_mode(), EditorMode::Normal);
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
}

#[test]
fn write_saves_and_stays_in_the_buffer() {
    // `:w` is the one command in vi that means "carry on".
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "w");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "still editing after a write"
    );
    assert!(
        !app.body_dirty(),
        "and the buffer counts as saved: {}",
        app.status()
    );
    assert!(
        app.body_buffer().contains("and more"),
        "with the text intact"
    );
}

#[test]
fn write_quit_saves_and_leaves() {
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "wq");
    assert_ne!(
        app.surface(),
        ModalSurface::EditBody,
        "`wq` is the one that leaves"
    );
}

#[test]
fn a_written_body_reaches_the_file() {
    let (dir, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "w");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(
        on_disk.contains("and more"),
        "the write actually wrote: {on_disk}"
    );
}

#[test]
fn quit_bang_discards_on_purpose() {
    // The escape hatch has to exist, and has to be explicit.
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "q!");
    assert_ne!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn escape_twice_does_not_sneak_the_edit_out() {
    // A second Esc is the reflex when the first one "did nothing";
    // it must not be the thing that loses the paragraph.
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(app.body_buffer().contains("and more"));
}

// === overlays opened from the full-window editor ===

fn file_view() -> (TempDir, Shell, ModalApp) {
    let (d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_view(closure_shell_core::ViewMode::Editor, &sh);
    assert_eq!(app.surface(), ModalSurface::EditFile, "the file is open");
    (d, sh, app)
}

#[test]
fn closing_the_palette_returns_to_the_buffer_it_was_opened_from() {
    // Reported as "opening the palette or capture jumps back to the
    // clickable GUI": every overlay returned to the outline, which in
    // the editor view is a different shape of the whole app.
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "palette");
    assert_eq!(app.surface(), ModalSurface::Palette);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile, "back to the file");
}

#[test]
fn cancelling_a_capture_returns_to_the_buffer() {
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "capture-start");
    assert_eq!(app.surface(), ModalSurface::Capture);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile);
}

#[test]
fn completing_a_capture_returns_to_the_buffer() {
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "capture-start");
    for c in "From the buffer".chars() {
        app.on_key(&mut sh, "x", false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile);
}

#[test]
fn the_clickable_view_still_returns_to_the_outline() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture-start");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === pasting from outside vim's own registers ===

#[test]
fn the_system_paste_lands_in_the_buffer_in_normal_mode() {
    // `C-v` is the desktop's paste and evil's visual-block; closure has
    // no visual-block, and the reported complaint is that `C-v` "does
    // not work in vim / Doom mode" — which is NORMAL, where the
    // keystroke-typing paste path (rightly) refuses, because typing a
    // pasted URL as commands would run a dozen of them.
    let (_d, _sh, mut app) = editing();
    app.body_set_cursor(0);
    app.body_paste_text("pasted ");
    assert!(
        app.body_buffer().starts_with("pasted "),
        "{:?}",
        app.body_buffer()
    );
    assert_eq!(app.body_mode(), EditorMode::Normal, "and stays in NORMAL");
}

#[test]
fn a_system_paste_is_one_undo_step() {
    let (_d, mut sh, mut app) = editing();
    let before = app.body_buffer().to_owned();
    app.body_set_cursor(0);
    app.body_paste_text("pasted text here");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), before, "one `u` takes the whole paste");
}

#[test]
fn a_system_paste_replaces_a_visual_selection() {
    let (_d, mut sh, mut app) = editing();
    app.body_set_cursor(0);
    app.on_key(&mut sh, "v", false, false, Some('v'));
    app.on_key(&mut sh, "e", false, false, Some('e')); // select "Original"
    app.body_paste_text("New");
    assert!(
        app.body_buffer().starts_with("New"),
        "the selection went and the paste took its place: {:?}",
        app.body_buffer()
    );
    assert_eq!(app.body_mode(), EditorMode::Normal, "visual ends with it");
}

// === what the window calls itself ===

#[test]
fn the_title_names_the_vault_when_browsing() {
    let (dir, sh) = shell();
    let app = ModalApp::new(InputMode::Doom);
    let name = dir.path().file_name().expect("name").to_string_lossy();
    let title = app.window_title(&sh, &name);
    assert!(title.contains(&*name), "{title}");
    assert!(title.contains("closure"), "{title}");
}

#[test]
fn the_title_names_the_buffer_being_edited() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    let title = app.window_title(&sh, "vault");
    assert!(
        title.contains("Note"),
        "a window in a buffer says which buffer: {title}"
    );
}

#[test]
fn an_unsaved_buffer_is_marked_in_the_title() {
    // The convention every editor shares, and the one place a user
    // looks when they are about to close a window.
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    let title = app.window_title(&sh, "vault");
    assert!(title.starts_with('●'), "{title}");
}

// === zoom (Doom's text-scale, on the buffer) ===

#[test]
fn zoom_starts_at_one_and_steps_both_ways() {
    let mut app = ModalApp::new(InputMode::Doom);
    assert!((app.zoom() - 1.0).abs() < f32::EPSILON, "unscaled to begin");
    app.zoom_in();
    assert!(app.zoom() > 1.0, "bigger");
    app.zoom_out();
    assert!(
        (app.zoom() - 1.0).abs() < 0.001,
        "and exactly back: {}",
        app.zoom()
    );
    app.zoom_out();
    assert!(app.zoom() < 1.0, "smaller");
}

#[test]
fn zoom_resets_to_one() {
    let mut app = ModalApp::new(InputMode::Doom);
    app.zoom_in();
    app.zoom_in();
    app.zoom_in();
    app.zoom_reset();
    assert!((app.zoom() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn zoom_is_bounded_at_both_ends() {
    // A font of zero px is not a zoom level, and neither is a wall of
    // one glyph.
    let mut app = ModalApp::new(InputMode::Doom);
    for _ in 0..50 {
        app.zoom_out();
    }
    assert!(app.zoom() >= 0.5, "floor: {}", app.zoom());
    for _ in 0..100 {
        app.zoom_in();
    }
    assert!(app.zoom() <= 4.2, "ceiling: {}", app.zoom());
}

#[test]
fn the_zoom_chords_reach_it_from_the_editor() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "+", true, false, Some('+'));
    assert!(app.zoom() > 1.0, "C-+ zooms in");
    app.on_key(&mut sh, "-", true, false, Some('-'));
    assert!((app.zoom() - 1.0).abs() < 0.001, "C-- zooms back out");
    app.on_key(&mut sh, "=", true, false, Some('='));
    assert!(app.zoom() > 1.0, "C-= is the same key without shift");
    app.on_key(&mut sh, "0", true, false, Some('0'));
    assert!((app.zoom() - 1.0).abs() < f32::EPSILON, "C-0 resets");
}

// === changing input mode while a buffer is open ===

#[test]
fn switching_to_notion_puts_the_open_buffer_into_insert() {
    // Notion mode has no NORMAL, so a buffer left in it after a mode
    // switch is a text field that will not take text — the exact
    // situation a non-technical default must never produce.
    let (_d, mut sh, mut app) = editing();
    assert_eq!(app.body_mode(), EditorMode::Normal, "vim opens in NORMAL");
    // vim → doom → helix → notion
    for _ in 0..3 {
        app.run(&mut sh, "cycle-mode");
    }
    assert_eq!(app.input_mode(), InputMode::Notion);
    assert_eq!(
        app.body_mode(),
        EditorMode::Insert,
        "a mode without a NORMAL types"
    );
}

#[test]
fn switching_to_a_modal_mode_puts_the_buffer_into_normal() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Notion);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    // Opening it in Notion means INSERT; the keystroke typed an `i`.
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.run(&mut sh, "cycle-mode"); // notion → emacs
    app.run(&mut sh, "cycle-mode"); // emacs → vim
    assert_eq!(app.input_mode(), InputMode::Vim);
    assert_eq!(
        app.body_mode(),
        EditorMode::Normal,
        "a modal mode starts in command state"
    );
}

// === a headline typed into a body ===

#[test]
fn a_headline_typed_into_a_body_becomes_a_child() {
    // Reported as a dislike of the comma escaping: typing `* Foo` into
    // a body meant `,* Foo` on disk. What it means is "this belongs
    // under this".
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "A", false, false, Some('A'));
    app.on_key(&mut sh, "enter", false, false, None);
    for c in "* Typed child".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.run_ex_line(&mut sh, "wq");

    let rows = app.rows(&sh);
    let child = rows
        .iter()
        .find(|r| r.title.contains("Typed child"))
        .expect("it became a row of its own");
    assert_eq!(child.level, 2, "one level under the note it was typed in");
    let note = rows
        .iter()
        .find(|r| r.title.contains("Note"))
        .expect("parent");
    assert_eq!(note.level, 1);
}

#[test]
fn the_prose_before_it_stays_the_body() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "A", false, false, Some('A'));
    app.on_key(&mut sh, "enter", false, false, None);
    for c in "* Typed child".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.run_ex_line(&mut sh, "wq");

    let id = closure_core::BlockId::from_existing("01HQEXIT00000000000000001");
    let (headline, _) = sh.vault.find_by_id(&id).expect("still there");
    assert!(
        headline.body_text().contains("Original body"),
        "the prose is still the body: {:?}",
        headline.body_text()
    );
    assert!(
        !headline.body_text().contains("Typed child"),
        "and the headline is not in it"
    );
}

#[test]
fn prose_that_merely_starts_with_a_star_is_still_escaped() {
    // `*bold*` and a list bullet are not headlines, and must not be
    // lifted out of the body.
    let (dir, mut sh, mut app) = editing();
    app.on_key(&mut sh, "A", false, false, Some('A'));
    app.on_key(&mut sh, "enter", false, false, None);
    for c in "*bold* opening".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.run_ex_line(&mut sh, "wq");

    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("*bold* opening"), "{on_disk}");
    assert_eq!(
        app.rows(&sh).len(),
        1,
        "no new row was invented: {:?}",
        app.rows(&sh).iter().map(|r| &r.title).collect::<Vec<_>>()
    );
}
