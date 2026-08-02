//! "…Event the Emacs readline ones?"
//!
//! The second half of the question. `C-a`, `C-e`, `C-w`, `M-d` and the
//! rest are resolved inside the buffer's own key handler, not through
//! the keymap, so `bind` could not reach them: the keymap-level rebind
//! landed and the twenty chords a writer actually spends the day
//! pressing stayed where they were.
//!
//! Two things fix that. The motions become commands with names, so
//! there is something to bind *to* — and a `bind` line for a modified
//! chord is consulted before any surface's own handler, so it wins in
//! a buffer, a prompt and the outline alike. One rule: what you wrote
//! down beats what the app would have done.
//!
//! Only modified chords. A `bind j = …` must not fire while you are
//! typing a `j`, and the surface that already knows the difference
//! between a letter and a chord is the one that should keep deciding.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::Config;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture(binds: &str) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQREADL00000000000001\n:END:\nhello world\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("config.org"),
        format!(
            "#+TITLE: t\n\n#+BEGIN_SRC closure-config\ninput_mode = doom\n{binds}\n#+END_SRC\n"
        ),
    )
    .expect("write");
    let cfg = Config::from_path(&dir.path().join("config.org")).expect("config");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(cfg.input_mode);
    app.set_key_overrides(cfg.key_bindings);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQREADL00000000000001"));
    (dir, shell, app)
}

/// A body buffer in INSERT with the caret at the start of the text.
fn inserting(binds: &str) -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture(binds);
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.body_click(0, 0);
    (dir, shell, app)
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn the_motions_have_names_a_bind_line_can_use() {
    // `M-x` lists them, which is the same thing as saying they exist.
    for cmd in [
        "line-start",
        "line-end",
        "kill-line",
        "kill-word-back",
        "word-left",
        "word-right",
    ] {
        let (_d, mut shell, mut app) = inserting("");
        app.run(&mut shell, cmd);
        assert!(
            !app.status().contains("unknown command"),
            "{cmd}: {}",
            app.status()
        );
    }
}

#[test]
fn line_end_puts_the_caret_at_the_end_of_the_line() {
    let (_d, mut shell, mut app) = inserting("");
    app.run(&mut shell, "line-end");
    type_in(&mut app, &mut shell, "!");
    assert!(
        app.body_buffer().starts_with("hello world!"),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn a_rebound_readline_chord_wins_inside_a_buffer() {
    // The whole item: `C-b` is readline's "back one character" and the
    // config says it should go to the start of the line instead.
    let (_d, mut shell, mut app) = inserting("bind C-b = line-end");
    app.on_key(&mut shell, "b", true, false, None);
    type_in(&mut app, &mut shell, "!");
    assert!(
        app.body_buffer().starts_with("hello world!"),
        "C-b ran line-end: {:?}",
        app.body_buffer()
    );
}

#[test]
fn the_chord_keeps_its_builtin_meaning_when_nothing_says_otherwise() {
    let (_d, mut shell, mut app) = inserting("");
    app.on_key(&mut shell, "e", true, false, None);
    type_in(&mut app, &mut shell, "!");
    assert!(
        app.body_buffer().starts_with("hello world!"),
        "C-e is still end-of-line: {:?}",
        app.body_buffer()
    );
}

#[test]
fn an_unmodified_binding_does_not_fire_while_you_type() {
    // `bind j = next-file` is an outline binding. If an override
    // pre-empted every surface, typing `j` into a note would move the
    // selection instead of writing a letter.
    let (_d, mut shell, mut app) = inserting("bind j = next-file");
    type_in(&mut app, &mut shell, "j");
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
    assert!(
        app.body_buffer().starts_with('j'),
        "{:?}",
        app.body_buffer()
    );
}

#[test]
fn a_rebound_chord_works_in_a_prompt_too() {
    // The prompts carry the same readline set, and a rebind that held
    // in one field and not the next would be worse than no rebind.
    let (_d, mut shell, mut app) = fixture("bind C-b = line-end");
    app.run(&mut shell, "search");
    type_in(&mut app, &mut shell, "abc");
    app.on_key(&mut shell, "a", true, false, None); // C-a: to the start
    app.on_key(&mut shell, "b", true, false, None); // rebound: to the end
    type_in(&mut app, &mut shell, "Z");
    assert_eq!(app.query(), "abcZ", "the caret went to the end");
}

#[test]
fn taking_a_readline_chord_away_leaves_it_inert() {
    let (_d, mut shell, mut app) = inserting("bind C-e =");
    app.on_key(&mut shell, "e", true, false, None);
    type_in(&mut app, &mut shell, "!");
    assert!(
        app.body_buffer().starts_with("!hello"),
        "C-e did nothing: {:?}",
        app.body_buffer()
    );
}

#[test]
fn the_motions_say_something_useful_outside_a_buffer() {
    // A command bound to a key you can press anywhere has to answer
    // when pressed where it means nothing.
    let (_d, mut shell, mut app) = fixture("");
    app.run(&mut shell, "kill-line");
    assert!(!app.status().is_empty(), "it said nothing at all");
}
