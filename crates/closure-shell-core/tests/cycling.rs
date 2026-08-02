//! Q3-V5 — cycling a TODO, a priority, and a checkbox.
//!
//! The two cycles the shells had were hardcoded: `TODO`→`DONE`→none and
//! `A`→`B`→`C`→none, whatever the vault's `config.org` said its keywords
//! and priorities were. A vault with `todo_keywords = TODO, NEXT, WAIT,
//! DONE` had four keywords and two of them unreachable.
//!
//! And a checkbox — the other thing an org file is full of — could only
//! be toggled by typing the `x` between the brackets by hand, with the
//! `[/]` cookie above it left saying whatever it used to say.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* TODO Ship it
:PROPERTIES:
:ID: 01HQCYCLE0000000000000001
:END:
body
* Plain
:PROPERTIES:
:ID: 01HQCYCLE0000000000000002
:END:
body
";

const CONFIG: &str = "\
#+BEGIN_SRC closure-config
todo_keywords = TODO, NEXT, WAIT, DONE
priority_levels = A, B, C, D
#+END_SRC
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    fs::write(dir.path().join("config.org"), CONFIG).expect("config");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_today("2026-07-28");
    (dir, Shell::new(vault), app)
}

fn todo_of(app: &ModalApp, shell: &Shell) -> Option<String> {
    app.detail(shell).and_then(|d| d.todo)
}

fn on_first(app: &mut ModalApp, shell: &Shell) {
    assert!(app.select_by_id(shell, "01HQCYCLE0000000000000001"));
}

#[test]
fn v5_the_todo_cycle_is_the_vaults_keyword_list() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);

    assert_eq!(todo_of(&app, &shell).as_deref(), Some("TODO"));
    app.run(&mut shell, "toggle-todo");
    assert_eq!(todo_of(&app, &shell).as_deref(), Some("NEXT"));
    app.run(&mut shell, "toggle-todo");
    assert_eq!(todo_of(&app, &shell).as_deref(), Some("WAIT"));
    app.run(&mut shell, "toggle-todo");
    assert_eq!(todo_of(&app, &shell).as_deref(), Some("DONE"));
    app.run(&mut shell, "toggle-todo");
    assert_eq!(todo_of(&app, &shell), None, "past the last keyword: none");
    app.run(&mut shell, "toggle-todo");
    assert_eq!(todo_of(&app, &shell).as_deref(), Some("TODO"), "and round");
}

#[test]
fn v5_the_cycle_runs_backwards_too() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000002"));
    assert_eq!(todo_of(&app, &shell), None);

    app.run(&mut shell, "todo-back");
    assert_eq!(
        todo_of(&app, &shell).as_deref(),
        Some("DONE"),
        "backwards from none is the last keyword"
    );
    app.run(&mut shell, "todo-back");
    assert_eq!(todo_of(&app, &shell).as_deref(), Some("WAIT"));
}

#[test]
fn v5_a_keyword_the_vault_does_not_know_starts_the_cycle_over() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* CANCELLED Old\n:PROPERTIES:\n:ID: 01HQCYCLE0000000000000003\n:END:\nbody\n",
    )
    .expect("write");
    fs::write(dir.path().join("config.org"), CONFIG).expect("config");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000003"));

    app.run(&mut shell, "toggle-todo");
    assert_eq!(
        todo_of(&app, &shell).as_deref(),
        Some("TODO"),
        "an unknown keyword is not a position in the cycle"
    );
}

#[test]
fn v5_priorities_come_from_the_config_too() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);

    for expected in ['A', 'B', 'C', 'D'] {
        app.run(&mut shell, "cycle-priority");
        assert_eq!(
            app.detail(&shell).and_then(|d| d.priority),
            Some(expected),
            "cycling to {expected}"
        );
    }
    app.run(&mut shell, "cycle-priority");
    assert_eq!(app.detail(&shell).and_then(|d| d.priority), None);
}

#[test]
fn v5_priority_up_and_down_move_by_severity() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);

    app.run(&mut shell, "cycle-priority"); // A
    app.run(&mut shell, "priority-down");
    assert_eq!(app.detail(&shell).and_then(|d| d.priority), Some('B'));
    app.run(&mut shell, "priority-up");
    assert_eq!(app.detail(&shell).and_then(|d| d.priority), Some('A'));
    app.run(&mut shell, "priority-up");
    assert_eq!(
        app.detail(&shell).and_then(|d| d.priority),
        Some('A'),
        "the top does not wrap into nothing"
    );
}

#[test]
fn v5_a_done_keyword_stamps_closed_when_the_vault_asks_for_it() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship it\n:PROPERTIES:\n:ID: 01HQCYCLE0000000000000004\n:END:\nbody\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\ntodo_keywords = TODO, DONE\nlog_done = true\n#+END_SRC\n",
    )
    .expect("config");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_today("2026-07-28");
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000004"));

    app.run(&mut shell, "toggle-todo");
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* DONE Ship it"), "{src}");
    assert!(src.contains("CLOSED: [2026-07-28 Tue]"), "{src}");

    // …and leaving DONE takes the stamp back off.
    app.run(&mut shell, "toggle-todo");
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!src.contains("CLOSED:"), "{src}");
}

#[test]
fn v5_without_log_done_nothing_is_stamped() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    for _ in 0..3 {
        app.run(&mut shell, "toggle-todo");
    }
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* DONE Ship it"), "{src}");
    assert!(!src.contains("CLOSED:"), "off by default: {src}");
}

// === checkboxes ===

const LIST: &str = "\
* Packing [/]
:PROPERTIES:
:ID: 01HQCYCLE0000000000000005
:END:
- [ ] socks
- [ ] toothbrush
- [ ] passport
";

fn list_fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), LIST).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_today("2026-07-28");
    (dir, Shell::new(vault), app)
}

#[test]
fn v5_a_checkbox_toggles_under_the_cursor() {
    let (_d, mut shell, mut app) = list_fixture();
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000005"));
    app.run(&mut shell, "edit-body");
    // The cursor opens on the first line: `- [ ] socks`.
    app.run(&mut shell, "toggle-checkbox");
    assert!(
        app.body_buffer().starts_with("- [X] socks"),
        "buffer: {}",
        app.body_buffer()
    );
    app.run(&mut shell, "toggle-checkbox");
    assert!(app.body_buffer().starts_with("- [ ] socks"));
}

#[test]
fn v5_the_cookie_counts_what_is_ticked() {
    let (dir, mut shell, mut app) = list_fixture();
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000005"));
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "toggle-checkbox");
    app.run(&mut shell, "save-buffer");

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* Packing [1/3]"), "{src}");
}

#[test]
fn v5_a_line_that_is_not_a_checkbox_is_left_alone() {
    let (_d, mut shell, mut app) = list_fixture();
    assert!(app.select_by_id(&shell, "01HQCYCLE0000000000000005"));
    app.run(&mut shell, "edit-body");
    let before = app.body_buffer().to_owned();
    // Move to the end of the buffer, past the list.
    app.on_key(&mut shell, "G", false, false, Some('G'));
    app.on_key(&mut shell, "o", false, false, Some('o'));
    app.on_key(&mut shell, "escape", false, false, None);
    app.run(&mut shell, "toggle-checkbox");
    assert!(
        app.body_buffer()
            .starts_with(before.lines().next().unwrap()),
        "the list is untouched"
    );
    assert!(
        app.status().contains("checkbox"),
        "status: {}",
        app.status()
    );
}
