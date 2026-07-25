//! No bound chord may be dead.
//!
//! `closure-input` is the single source of truth for keybindings (I4),
//! and which-key, the palette and the context menus all render from
//! it. So every chord it binds is *advertised* to the user in the GUI —
//! and until now several of them resolved to `ModalApp`'s catch-all
//! and answered "not available in the modal GUI experiment": db-view,
//! headline-list, body-search, toggle-llm-render, allow-flow,
//! block-flow, resolve-ours, resolve-theirs.
//!
//! Advertising a binding that does nothing is worse than not binding
//! it. This test makes the gap impossible to reintroduce: it walks
//! every command in every mode's keymap and fails on any that reports
//! itself unavailable.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeSet;
use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const SRC: &str = "* TODO Alpha :work:\n:PROPERTIES:\n:ID: 01HQDEAD0000000000000001\n:END:\n\
                   SCHEDULED: <2026-07-25 Sat>\n\
                   body text about alpha\n\
                   #+BEGIN_SRC sh\necho hi\n#+END_SRC\n\
                   * [#A] Beta\n:PROPERTIES:\n:ID: 01HQDEAD0000000000000002\n:END:\n\
                   links to [[id:01HQDEAD0000000000000001][Alpha]]\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Every distinct command any mode binds a chord to.
fn every_bound_command() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for (_, command) in closure_input::mode_keymap(mode) {
            out.insert(*command);
        }
    }
    out
}

#[test]
fn every_bound_command_is_implemented() {
    let commands = every_bound_command();
    assert!(commands.len() > 20, "sanity: {} commands", commands.len());
    let mut dead = Vec::new();
    for command in &commands {
        let (_d, mut shell, mut app) = fixture();
        app.select(0, &shell);
        app.run(&mut shell, command);
        let status = app.status();
        if status.contains("not available") || status.contains("unknown command") {
            dead.push(format!("{command}: {status}"));
        }
    }
    assert!(
        dead.is_empty(),
        "chords are bound to commands the GUI does not implement:\n  {}",
        dead.join("\n  ")
    );
}

#[test]
fn quit_is_the_only_command_that_ends_the_session() {
    // A safety net for the sweep above: running every command must not
    // accidentally tear the app down.
    for command in every_bound_command() {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        assert_eq!(
            app.should_quit(),
            command == "quit",
            "{command} set should_quit to {}",
            app.should_quit()
        );
    }
}

#[test]
fn every_command_leaves_the_app_navigable() {
    // Whatever surface a command lands on, the row list must still be
    // answerable and the selection in range — a surface that panics or
    // strands the cursor is not wired, it is broken.
    for command in every_bound_command() {
        let (_d, mut shell, mut app) = fixture();
        app.select(0, &shell);
        app.run(&mut shell, command);
        let rows = app.rows(&shell);
        assert!(
            app.selected() <= rows.len(),
            "{command}: selection {} out of {} rows",
            app.selected(),
            rows.len()
        );
        // …and Escape always gets back to Browse.
        app.on_key(&mut shell, "escape", false, false, None);
        app.on_key(&mut shell, "escape", false, false, None);
        assert_eq!(
            app.surface(),
            ModalSurface::Browse,
            "{command}: Escape must return to Browse"
        );
    }
}

// === the surfaces the newly-wired commands open ===

#[test]
fn headline_list_shows_every_headline_in_the_file() {
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "headline-list");
    assert_eq!(app.surface(), ModalSurface::Headlines);
    let rows = app.headline_rows(&shell);
    assert_eq!(rows.len(), 2, "both headlines: {rows:?}");
    assert!(rows.iter().any(|(t, _)| t.contains("Alpha")));
    assert!(rows.iter().any(|(t, _)| t.contains("Beta")));
}

#[test]
fn db_view_is_a_table_of_title_todo_priority() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "db-view");
    assert_eq!(app.surface(), ModalSurface::DbView);
    let (header, rows) = app.db_rows(&shell);
    assert_eq!(header, vec!["title", "todo", "priority", "tags"]);
    let alpha = rows.iter().find(|r| r[0] == "Alpha").expect("Alpha row");
    assert_eq!(alpha[1], "TODO");
    assert_eq!(alpha[3], "work");
    let beta = rows.iter().find(|r| r[0] == "Beta").expect("Beta row");
    assert_eq!(beta[2], "A", "priority cookie lands in its own column");
    assert_eq!(beta[1], "", "no TODO keyword is an empty cell, not a dash");
}

#[test]
fn body_search_matches_text_titles_do_not() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "body-search");
    assert_eq!(app.surface(), ModalSurface::BodySearch);
    // "alpha" appears in Alpha's *body*; typing it must find the entry
    // even though a title search would too — so search for a word that
    // only exists in the body.
    for c in "text about".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    let hits = app.body_search_rows(&shell);
    assert_eq!(hits.len(), 1, "one body hit: {hits:?}");
    assert!(hits[0].1.contains("Alpha"), "{hits:?}");
}

#[test]
fn body_search_with_an_empty_query_shows_nothing() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "body-search");
    assert!(
        app.body_search_rows(&shell).is_empty(),
        "an empty needle must not dump the whole vault"
    );
}

#[test]
fn toggle_llm_render_flips_the_render_grant_and_says_so() {
    let (_d, mut shell, mut app) = fixture();
    assert!(
        !app.llm_render_access(),
        "render access is off until explicitly granted (V3b)"
    );
    app.run(&mut shell, "toggle-llm-render");
    assert!(app.llm_render_access());
    assert!(
        app.status().contains("render"),
        "the toggle must report which way it went: {}",
        app.status()
    );
    app.run(&mut shell, "toggle-llm-render");
    assert!(!app.llm_render_access(), "and back off");
}

// === jumping out of a list surface ===

#[test]
fn select_by_id_moves_the_outline_cursor_to_that_headline() {
    // The list surfaces (headlines, body search, db view) answer in
    // block ids; clicking a row has to translate that into an outline
    // selection, or the mouse path dead-ends where the Enter path does
    // not.
    let (_d, shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQDEAD0000000000000002"));
    assert_eq!(app.rows(&shell)[app.selected()].title, "Beta");
    assert!(app.select_by_id(&shell, "01HQDEAD0000000000000001"));
    assert_eq!(app.rows(&shell)[app.selected()].title, "Alpha");
}

#[test]
fn select_by_id_reports_an_unknown_id_rather_than_moving() {
    let (_d, shell, mut app) = fixture();
    app.select(1, &shell);
    assert!(!app.select_by_id(&shell, "01HQDEADNOTHINGHEREATALL"));
    assert_eq!(app.selected(), 1, "an unknown id must not move the cursor");
}
