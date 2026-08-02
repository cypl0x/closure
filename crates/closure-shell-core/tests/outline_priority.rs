//! "there are so many colors in the Doom Emacs Vibrant color palette
//! and you prefer to use this dimmed color for something like ID and
//! TODO" — and, unfiled and worse: there is no `[#A]` in the outline at
//! all.
//!
//! An urgent task and an unprioritised one were the same row. org's
//! priority cookie is the one piece of a headline that says *do this
//! first*, the outline is where you look to decide what to do first,
//! and the row model had nowhere to put it: `Row` carried the id, the
//! path, the title, the level and the keyword, and dropped the cookie
//! the parser had already found.
//!
//! So the row carries it, and it is painted the way org paints it —
//! `org-priority-faces`, A loudest, C quietest — which is also how the
//! TODO keyword beside it is already painted, so the two agree about
//! what urgent looks like.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Project
:PROPERTIES:
:ID: 01HQPRI000000000000001
:END:
** TODO [#A] An urgent task
:PROPERTIES:
:ID: 01HQPRI000000000000002
:END:
** TODO [#B] A middling one
:PROPERTIES:
:ID: 01HQPRI000000000000003
:END:
** DONE [#C] A quiet finished one
:PROPERTIES:
:ID: 01HQPRI000000000000004
:END:
** TODO No cookie at all
:PROPERTIES:
:ID: 01HQPRI000000000000005
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// The row for `id`.
fn row(app: &ModalApp, shell: &Shell, id: &str) -> closure_shell_core::Row {
    app.rows(shell)
        .into_iter()
        .find(|r| r.id == id)
        .expect("a row")
}

#[test]
fn a_row_carries_the_priority_the_parser_found() {
    let (_d, shell, app) = fixture();
    assert_eq!(
        row(&app, &shell, "01HQPRI000000000000002").priority,
        Some('A')
    );
    assert_eq!(
        row(&app, &shell, "01HQPRI000000000000003").priority,
        Some('B')
    );
    assert_eq!(
        row(&app, &shell, "01HQPRI000000000000004").priority,
        Some('C')
    );
}

#[test]
fn a_headline_without_a_cookie_has_no_priority() {
    let (_d, shell, app) = fixture();
    assert_eq!(row(&app, &shell, "01HQPRI000000000000005").priority, None);
    assert_eq!(row(&app, &shell, "01HQPRI000000000000001").priority, None);
}

#[test]
fn the_cookie_is_not_left_in_the_title() {
    // It is painted as its own piece, so a title that still contains it
    // would show it twice.
    let (_d, shell, app) = fixture();
    let r = row(&app, &shell, "01HQPRI000000000000002");
    assert_eq!(r.title, "An urgent task", "{:?}", r.title);
}

#[test]
fn a_priority_reads_as_a_cookie_wherever_it_is_shown() {
    // `[#A]`, org's own spelling — the thing you would type to make one.
    let (_d, shell, app) = fixture();
    let r = row(&app, &shell, "01HQPRI000000000000002");
    assert_eq!(
        r.priority.map(closure_shell_core::priority_cookie),
        Some("[#A]".to_owned())
    );
}

#[test]
fn urgency_is_ordered_and_a_is_the_loudest() {
    // The whole point of the cookie: A must not read like C. The rank
    // is what a painter turns into a colour, and it is here rather than
    // in a shell so every shell agrees.
    use closure_shell_core::priority_rank;
    assert!(priority_rank('A') > priority_rank('B'));
    assert!(priority_rank('B') > priority_rank('C'));
    assert_eq!(
        priority_rank('a'),
        0,
        "only org's own uppercase cookies rank"
    );
}

#[test]
fn the_row_view_shows_it_as_a_badge() {
    // `RowView` is what a shell without its own outline paints, and its
    // doc already promised "a priority letter" among the badges.
    let (_d, shell, app) = fixture();
    let _ = &app;
    let rows = match closure_shell_core::browse_view(&shell.vault) {
        closure_shell_core::Node::Pane { children, .. } => children
            .into_iter()
            .find_map(|c| match c {
                closure_shell_core::Node::Rows { rows, .. } => Some(rows),
                _ => None,
            })
            .expect("rows"),
        other => panic!("{other:?}"),
    };
    let view = rows
        .into_iter()
        .find(|v| v.id == "01HQPRI000000000000002")
        .expect("a view");
    assert!(view.badges.iter().any(|b| b == "[#A]"), "{:?}", view.badges);
}

#[test]
fn the_cookie_is_clickable_onto_a_command_that_exists() {
    // The chip cycles the priority, the way the keyword chip beside it
    // toggles the keyword. A chip wired to a name nothing answers to is
    // an affordance that does nothing when clicked.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQPRI000000000000002"));
    app.run(&mut shell, "cycle-priority");
    assert!(
        !app.status().contains("unknown command"),
        "{}",
        app.status()
    );
    assert_ne!(
        row(&app, &shell, "01HQPRI000000000000002").priority,
        Some('A'),
        "it cycled"
    );
}
