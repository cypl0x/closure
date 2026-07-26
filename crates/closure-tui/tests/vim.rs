//! The terminal shell's body editor is the same modal editor the gpui
//! shell drives: [`closure_shell_core::BodyEditor`].
//!
//! Before this, `EditBody` in the TUI was an append-only `String` — no
//! cursor, no modes, no vim. The two shells now share one editor, so a
//! chord that works in the GUI works in the terminal (I4).

use std::path::PathBuf;

use closure_shell_core::EditorMode;
use closure_tui::{App, AppMode, HeadlineRecord};

fn paths() -> Vec<PathBuf> {
    vec![PathBuf::from("a.org")]
}

fn rec(id: &str, title: &str, body: &str) -> HeadlineRecord {
    HeadlineRecord {
        path: PathBuf::from("a.org"),
        id: id.to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
        ..HeadlineRecord::default()
    }
}

/// An app parked in the body editor over `body`, in Normal mode with
/// the cursor at the start — the state every vim chord starts from.
fn editing(body: &str) -> App {
    let mut app = App::new(paths());
    app.set_headlines(vec![rec("id-a1", "Note", body)]);
    app.handle_stroke("l"); // headline list
    app.handle_stroke("i"); // edit the body
    assert_eq!(app.mode(), AppMode::EditBody);
    app.handle_stroke("ESC"); // INSERT -> NORMAL
    app.set_body_cursor(0);
    app
}

fn feed(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.handle_stroke(&c.to_string());
    }
}

#[test]
fn the_editor_opens_in_insert_and_escape_drops_to_normal() {
    let mut app = App::new(paths());
    app.set_headlines(vec![rec("id-a1", "Note", "text")]);
    app.handle_stroke("l");
    app.handle_stroke("i");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.handle_stroke("ESC");
    assert_eq!(app.body_mode(), EditorMode::Normal);
    assert_eq!(app.mode(), AppMode::EditBody, "escape does not cancel yet");
}

#[test]
fn diw_deletes_the_word_under_the_cursor() {
    let mut app = editing("hello world");
    feed(&mut app, "diw");
    assert_eq!(app.buffer(), " world");
}

#[test]
fn caw_changes_the_word_and_returns_to_insert() {
    let mut app = editing("hello world");
    feed(&mut app, "caw");
    assert_eq!(app.buffer(), "world");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.handle_stroke("x");
    assert_eq!(app.buffer(), "xworld", "typing resumes at the cursor");
}

#[test]
fn dd_and_p_move_a_line() {
    let mut app = editing("one\ntwo\nthree");
    feed(&mut app, "dd");
    assert_eq!(app.buffer(), "two\nthree");
    feed(&mut app, "p");
    assert_eq!(app.buffer(), "two\none\nthree");
}

#[test]
fn di_quote_reaches_inside_the_quotes() {
    let mut app = editing("#+TITLE: \"quoted\" tail");
    app.set_body_cursor(12);
    feed(&mut app, "di\"");
    assert_eq!(app.buffer(), "#+TITLE: \"\" tail");
}

#[test]
fn visual_mode_selects_and_deletes() {
    let mut app = editing("hello world");
    feed(&mut app, "viw");
    assert_eq!(app.body_mode(), EditorMode::Visual);
    feed(&mut app, "d");
    assert_eq!(app.buffer(), " world");
    assert_eq!(app.body_mode(), EditorMode::Normal);
}

#[test]
fn counts_reach_the_terminal_too() {
    let mut app = editing("a b c d e");
    feed(&mut app, "2dw");
    assert_eq!(app.buffer(), "c d e");
}

#[test]
fn the_cursor_is_exposed_for_the_renderer() {
    let mut app = editing("one\ntwo");
    assert_eq!(app.body_cursor(), (0, 0));
    feed(&mut app, "j");
    assert_eq!(app.body_cursor(), (1, 0));
    feed(&mut app, "$");
    assert_eq!(app.body_cursor(), (1, 3));
}

#[test]
fn arrow_keys_navigate_like_hjkl() {
    let mut app = editing("one\ntwo");
    app.handle_stroke("<down>");
    assert_eq!(app.body_cursor(), (1, 0));
    app.handle_stroke("<right>");
    assert_eq!(app.body_cursor(), (1, 1));
    app.handle_stroke("<left>");
    assert_eq!(app.body_cursor(), (1, 0));
    app.handle_stroke("<up>");
    assert_eq!(app.body_cursor(), (0, 0));
}

#[test]
fn insert_mode_typing_still_builds_the_buffer() {
    let mut app = editing("");
    app.handle_stroke("i");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.handle_stroke("a");
    app.handle_stroke("SPC");
    app.handle_stroke("b");
    app.handle_stroke("RET");
    app.handle_stroke("c");
    assert_eq!(app.buffer(), "a b\nc");
    app.handle_stroke("DEL");
    assert_eq!(app.buffer(), "a b\n");
}

#[test]
fn escape_on_a_quiet_normal_surface_cancels_the_edit() {
    let mut app = editing("text");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.take_body_request(), None, "cancelling saves nothing");
}

#[test]
fn escape_mid_chord_only_clears_the_chord() {
    let mut app = editing("text");
    app.handle_stroke("d");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::EditBody, "the chord was cancelled");
    feed(&mut app, "x");
    assert_eq!(app.buffer(), "ext");
}

#[test]
fn ctrl_s_saves_the_edited_body_from_normal_mode() {
    let mut app = editing("hello world");
    feed(&mut app, "diw");
    app.handle_stroke("C-s");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(
        app.take_body_request(),
        Some(("id-a1".to_owned(), " world".to_owned()))
    );
}

#[test]
fn u_undoes_inside_the_editor() {
    let mut app = editing("hello world");
    feed(&mut app, "diwu");
    assert_eq!(app.buffer(), "hello world");
}

#[test]
fn the_pane_title_announces_the_mode() {
    let mut app = editing("text");
    assert!(
        app.body_status().contains("NORMAL"),
        "status was {:?}",
        app.body_status()
    );
    app.handle_stroke("i");
    assert!(
        app.body_status().contains("INSERT"),
        "status was {:?}",
        app.body_status()
    );
    app.handle_stroke("ESC");
    app.handle_stroke("v");
    assert!(
        app.body_status().contains("VISUAL"),
        "status was {:?}",
        app.body_status()
    );
}

#[test]
fn a_pending_chord_shows_in_the_status() {
    let mut app = editing("text");
    app.handle_stroke("2");
    app.handle_stroke("d");
    assert!(
        app.body_status().contains("2d"),
        "status was {:?}",
        app.body_status()
    );
}
