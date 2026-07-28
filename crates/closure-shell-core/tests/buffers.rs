//! Q1 — buffers, jumps, recents.
//!
//! The shell held exactly one buffer: opening another note put the old
//! one in a stash you could not see and could not get back to except by
//! finding the headline again. Every cross-note movement was a fresh
//! search, and there was no way back to where a search had thrown you.
//!
//! So: an open-buffer list with the vim verbs over it (`:bnext`,
//! `:bprev`, `C-^`, `:bd`), a jumplist (`C-o`/`C-i`) that every
//! non-local move pushes to, and a recent-files picker that survives
//! the session — persisted in `config.org` like the rest of the durable
//! view state, because a vault is plain files.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQBUF000000000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQBUF000000000000000002
:END:
beta body
";

const OTHER: &str = "\
* Gamma
:PROPERTIES:
:ID: 01HQBUF000000000000000003
:END:
gamma body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    fs::write(dir.path().join("other.org"), OTHER).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open the body of the headline with `id`, the way a user does: select
/// the row, then open it.
fn open(app: &mut ModalApp, shell: &mut Shell, id: &str) {
    assert!(app.select_by_id(shell, id), "select {id}");
    app.run(shell, "edit-body");
}

fn names(app: &ModalApp, shell: &Shell) -> Vec<String> {
    app.buffer_rows(shell).into_iter().map(|b| b.name).collect()
}

#[test]
fn b1_every_opened_body_is_a_listed_buffer_most_recent_first() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");
    open(&mut app, &mut shell, "01HQBUF000000000000000003");

    let rows = app.buffer_rows(&shell);
    assert_eq!(rows.len(), 3, "three buffers: {rows:?}");
    assert_eq!(rows[0].name, "Gamma", "current is first: {rows:?}");
    assert!(rows[0].current, "the open one is marked current");
    assert!(!rows[1].current && !rows[2].current);
    assert_eq!(
        names(&app, &shell),
        vec!["Gamma", "Beta", "Alpha"],
        "most-recently-visited order"
    );
}

#[test]
fn b1_reopening_a_buffer_does_not_duplicate_it() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");
    open(&mut app, &mut shell, "01HQBUF000000000000000001");

    assert_eq!(names(&app, &shell), vec!["Alpha", "Beta"]);
}

#[test]
fn b1_an_unsaved_buffer_says_so_in_the_list() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "x", false, false, Some('x'));
    open(&mut app, &mut shell, "01HQBUF000000000000000002");

    let rows = app.buffer_rows(&shell);
    let alpha = rows.iter().find(|b| b.name == "Alpha").expect("alpha");
    assert!(alpha.dirty, "the stashed edit is unsaved: {rows:?}");
    let beta = rows.iter().find(|b| b.name == "Beta").expect("beta");
    assert!(!beta.dirty, "untouched buffer is clean: {rows:?}");
}

#[test]
fn b2_alternate_toggles_between_the_last_two_buffers() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");

    app.run(&mut shell, "buffer-alternate");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    app.run(&mut shell, "buffer-alternate");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Beta");
}

#[test]
fn b2_alternate_with_one_buffer_is_a_no_op_that_says_why() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    app.run(&mut shell, "buffer-alternate");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha");
    assert!(
        app.status().contains("no other buffer"),
        "status: {}",
        app.status()
    );
}

#[test]
fn b1_buffer_next_and_prev_walk_the_open_order_and_wrap() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001"); // Alpha
    open(&mut app, &mut shell, "01HQBUF000000000000000002"); // Beta
    open(&mut app, &mut shell, "01HQBUF000000000000000003"); // Gamma

    // Open order is Alpha, Beta, Gamma; we are on Gamma.
    app.run(&mut shell, "buffer-next");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha", "wraps to the top");
    app.run(&mut shell, "buffer-next");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Beta");
    app.run(&mut shell, "buffer-prev");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha");
    app.run(&mut shell, "buffer-prev");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Gamma", "wraps back round");
}

#[test]
fn b1_closing_a_buffer_opens_the_next_most_recent() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");

    app.run(&mut shell, "buffer-close");
    assert_eq!(names(&app, &shell), vec!["Alpha"]);
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn b1_closing_the_last_buffer_returns_to_the_outline() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    app.run(&mut shell, "buffer-close");
    assert!(app.buffer_rows(&shell).is_empty());
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn b1_a_dirty_buffer_is_not_closed_without_being_told_twice() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "x", false, false, Some('x'));

    app.run(&mut shell, "buffer-close");
    assert_eq!(
        names(&app, &shell),
        vec!["Alpha"],
        "unsaved text is not thrown away by one chord"
    );
    assert!(app.status().contains("unsaved"), "status: {}", app.status());
    app.run(&mut shell, "buffer-close-force");
    assert!(app.buffer_rows(&shell).is_empty());
}

#[test]
fn b1_the_buffer_list_is_a_picker() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");

    app.run(&mut shell, "buffer-list");
    assert_eq!(app.surface(), ModalSurface::Buffers);
    // Typing filters the list; Enter opens what is left.
    app.on_key(&mut shell, "a", false, false, Some('a'));
    app.on_key(&mut shell, "l", false, false, Some('l'));
    assert_eq!(
        app.buffer_rows(&shell)
            .iter()
            .filter(|b| b.matches_filter)
            .count(),
        1,
        "only Alpha matches 'al'"
    );
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha");
}

#[test]
fn b3_jump_back_returns_through_every_non_local_move() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");
    open(&mut app, &mut shell, "01HQBUF000000000000000003");

    app.run(&mut shell, "jump-back");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Beta");
    app.run(&mut shell, "jump-back");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Alpha");
    app.run(&mut shell, "jump-forward");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Beta");
    app.run(&mut shell, "jump-forward");
    assert_eq!(app.buffer_rows(&shell)[0].name, "Gamma");
}

#[test]
fn b3_n_backs_land_on_the_nth_prior_place_for_every_n() {
    for n in 1..=3_usize {
        let (_d, mut shell, mut app) = fixture();
        let ids = [
            "01HQBUF000000000000000001",
            "01HQBUF000000000000000002",
            "01HQBUF000000000000000003",
            "01HQBUF000000000000000001",
        ];
        for id in ids {
            open(&mut app, &mut shell, id);
        }
        for _ in 0..n {
            app.run(&mut shell, "jump-back");
        }
        let expect = match n {
            1 => "Gamma",
            2 => "Beta",
            _ => "Alpha",
        };
        assert_eq!(
            app.buffer_rows(&shell)[0].name,
            expect,
            "{n} backs from the fourth open"
        );
    }
}

#[test]
fn b3_jumping_after_going_back_drops_the_forward_history() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    open(&mut app, &mut shell, "01HQBUF000000000000000002");
    app.run(&mut shell, "jump-back"); // on Alpha, Beta ahead
    open(&mut app, &mut shell, "01HQBUF000000000000000003");
    app.run(&mut shell, "jump-forward");
    assert_eq!(
        app.buffer_rows(&shell)[0].name,
        "Gamma",
        "there is nothing ahead of a fresh jump"
    );
}

#[test]
fn b3_jumping_with_no_history_is_a_no_op() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "jump-back");
    app.run(&mut shell, "jump-forward");
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn b3_a_jump_remembers_the_cursor_it_left() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQBUF000000000000000001");
    app.body_set_cursor(5);
    open(&mut app, &mut shell, "01HQBUF000000000000000002");
    app.run(&mut shell, "jump-back");
    assert_eq!(app.body_cursor(), (0, 5), "back means back to the spot");
}

#[test]
fn b4_files_picker_lists_every_vault_file_recent_first() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "recent-files");
    assert_eq!(app.surface(), ModalSurface::Files);
    let rows = app.file_rows(&shell);
    assert!(
        rows.iter().any(|r| r.name.ends_with("notes.org"))
            && rows.iter().any(|r| r.name.ends_with("other.org")),
        "every file is reachable: {rows:?}"
    );
}

#[test]
fn b4_opening_a_file_from_the_picker_opens_it_as_a_buffer() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "recent-files");
    let i = app
        .file_rows(&shell)
        .iter()
        .position(|r| r.name.ends_with("other.org"))
        .expect("other.org listed");
    app.file_click(&shell, i);
    assert_eq!(app.surface(), ModalSurface::EditFile);
    assert!(app.body_buffer().contains("Gamma"));
    assert_eq!(app.buffer_rows(&shell)[0].name, "other.org");
}

#[test]
fn b4_recent_files_are_remembered_across_sessions() {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "recent-files");
    let i = app
        .file_rows(&shell)
        .iter()
        .position(|r| r.name.ends_with("other.org"))
        .expect("other.org listed");
    app.file_click(&shell, i);
    app.save_last_place(&shell);

    let cfg = fs::read_to_string(dir.path().join("config.org")).unwrap_or_default();
    assert!(cfg.contains("other.org"), "config.org: {cfg}");

    let mut next = ModalApp::new(InputMode::Doom);
    next.restore_last_place(&shell);
    next.run(&mut shell, "recent-files");
    assert_eq!(
        next.file_rows(&shell)[0].name,
        "other.org",
        "the file you were last in is the first one offered"
    );
}
