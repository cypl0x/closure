//! "refactor bottom message line … Since these messages are quite
//! useful. Please create something similiar like the *MESSAGES* buffer
//! in Emacs which is kind off a log. Introduce a (infinite?) history
//! (just show a few youngest log messages) but with the ability to
//! scroll with the corresponding keymap/chords mode. As always put a
//! button with some well thought configurable keybinding."
//!
//! The status line was one `String`, overwritten by whatever happened
//! next: "saved", "3 file(s) changed on disk", "peer added" all landed
//! in the same slot and the previous one was gone before you had
//! finished reading it. There was no history because nothing kept one.
//!
//! So every message goes through one place on its way to that slot, and
//! that place also keeps it. `messages` is the log; it opens as the
//! picker every other list uses, which is where the scrolling and the
//! filtering come from for free.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQMSG000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    app.select_by_id(&shell, "01HQMSG000000000000001");
    (dir, shell, app)
}

#[test]
fn a_message_is_kept_after_the_next_one_replaces_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-wrap");
    let first = app.status().to_owned();
    app.run(&mut shell, "toggle-wrap");
    let second = app.status().to_owned();
    assert_ne!(first, second, "two different messages");

    let log = app.messages();
    assert!(log.contains(&first), "the first is kept: {log:?}");
    assert!(log.contains(&second), "{log:?}");
}

#[test]
fn the_newest_message_is_first() {
    // "just show a few youngest log messages" — so the youngest is
    // where a glance lands.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-wrap");
    let first = app.status().to_owned();
    app.run(&mut shell, "toggle-wrap");
    let second = app.status().to_owned();
    let log = app.messages();
    let at = |t: &String| log.iter().position(|m| m == t).expect("logged");
    assert!(at(&second) < at(&first), "{log:?}");
}

#[test]
fn the_same_message_twice_running_is_one_entry() {
    // Emacs collapses a repeat into "[2 times]" for the same reason: a
    // log full of "saved" is a log you stop reading. Alternating
    // messages are four real events and stay four — only a repeat of
    // the one already at the top is dropped.
    let (_d, mut shell, mut app) = fixture();
    app.set_status("saved");
    app.set_status("saved");
    app.set_status("saved");
    let _ = &mut shell;
    assert_eq!(
        app.messages().iter().filter(|m| *m == "saved").count(),
        1,
        "{:?}",
        app.messages()
    );

    app.set_status("something else");
    app.set_status("saved");
    assert_eq!(
        app.messages().iter().filter(|m| *m == "saved").count(),
        2,
        "and it comes back when something happened between: {:?}",
        app.messages()
    );
}

#[test]
fn the_log_is_a_picker_like_every_other_list() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-wrap");
    app.run(&mut shell, "messages");
    assert_eq!(app.surface(), ModalSurface::Messages);
    let view = app.picker_view(&shell).expect("the messages picker");
    assert!(!view.rows.is_empty(), "nothing logged");
    assert!(!view.title.is_empty());
}

#[test]
fn it_filters_like_every_other_picker() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-wrap");
    app.run(&mut shell, "messages");
    let all = app.picker_view(&shell).expect("picker").rows.len();
    for c in "zzzznothing".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    assert!(app.picker_view(&shell).expect("picker").rows.len() < all.max(1));
}

#[test]
fn every_mode_can_open_it() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "messages").is_some(),
            "{mode:?} cannot reach the log"
        );
    }
}

#[test]
fn escape_leaves_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "messages");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_ne!(app.surface(), ModalSurface::Messages);
}

#[test]
fn the_log_does_not_grow_without_bound() {
    // "(infinite?)" — no: a log that never forgets is a leak with a
    // scrollbar. Deep enough to answer "what did that say", capped.
    let (_d, mut shell, mut app) = fixture();
    for i in 0..500 {
        app.set_status(format!("message {i}"));
    }
    let _ = &mut shell;
    assert!(app.messages().len() <= 200, "{}", app.messages().len());
    assert!(
        app.messages().contains(&"message 499".to_owned()),
        "the newest survived"
    );
}
