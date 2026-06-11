//! Unit tests for the pure TUI application state model.
//!
//! The `App` is an Elm-style state machine: strokes go in, state
//! changes come out. No terminal involved, so every transition is
//! unit-testable.

use std::path::PathBuf;

use closure_tui::App;

fn paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("a.org"),
        PathBuf::from("b.org"),
        PathBuf::from("c.org"),
    ]
}

#[test]
fn app_new_selects_first_path() {
    let app = App::new(paths());
    assert_eq!(app.selected_index(), Some(0));
    assert_eq!(app.selected_path(), Some(PathBuf::from("a.org").as_path()));
}

#[test]
fn app_new_empty_has_no_selection() {
    let app = App::new(Vec::new());
    assert_eq!(app.selected_index(), None);
    assert_eq!(app.selected_path(), None);
}

#[test]
fn app_stroke_j_moves_selection_down() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    assert_eq!(app.selected_index(), Some(1));
}

#[test]
fn app_stroke_j_clamps_at_last() {
    let mut app = App::new(paths());
    for _ in 0..10 {
        app.handle_stroke("j");
    }
    assert_eq!(app.selected_index(), Some(2));
}

#[test]
fn app_stroke_k_moves_selection_up_and_clamps() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    app.handle_stroke("k");
    assert_eq!(app.selected_index(), Some(0));
    app.handle_stroke("k");
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn app_strokes_on_empty_vault_do_not_panic() {
    let mut app = App::new(Vec::new());
    app.handle_stroke("j");
    app.handle_stroke("k");
    assert_eq!(app.selected_index(), None);
}

#[test]
fn app_stroke_q_requests_quit() {
    let mut app = App::new(paths());
    assert!(!app.should_quit());
    app.handle_stroke("q");
    assert!(app.should_quit());
}

#[test]
fn app_g_g_jumps_to_first_file() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    app.handle_stroke("j");
    assert_eq!(app.selected_index(), Some(2));
    app.handle_stroke("g");
    app.handle_stroke("g");
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn app_capital_g_jumps_to_last_file() {
    let mut app = App::new(paths());
    app.handle_stroke("G");
    assert_eq!(app.selected_index(), Some(2));
}

#[test]
fn app_pending_prefix_opens_whichkey_popup() {
    let mut app = App::new(paths());
    assert!(app.popup_lines().is_none());
    app.handle_stroke("g");
    let lines = app.popup_lines();
    assert!(
        lines.is_some_and(|ls| ls.iter().any(|l| l.contains("first-file"))),
        "popup must name the reachable command, got {lines:?}"
    );
}

#[test]
fn app_resolving_chord_closes_popup() {
    let mut app = App::new(paths());
    app.handle_stroke("g");
    assert!(app.popup_lines().is_some());
    app.handle_stroke("g");
    assert!(app.popup_lines().is_none());
}

#[test]
fn app_unbound_stroke_resets_pending_and_popup() {
    let mut app = App::new(paths());
    app.handle_stroke("g");
    assert_eq!(app.pending_chord(), "g");
    app.handle_stroke("z");
    assert_eq!(app.pending_chord(), "");
    assert!(app.popup_lines().is_none());
}

#[test]
fn app_pending_chord_tracks_strokes() {
    let mut app = App::new(paths());
    assert_eq!(app.pending_chord(), "");
    app.handle_stroke("g");
    assert_eq!(app.pending_chord(), "g");
    app.handle_stroke("g");
    assert_eq!(app.pending_chord(), "");
}

#[test]
fn app_custom_bindings_override_defaults() {
    let mut app = App::with_bindings(paths(), &[("x", "quit")]);
    app.handle_stroke("j");
    assert_eq!(app.selected_index(), Some(0), "j unbound under custom map");
    app.handle_stroke("x");
    assert!(app.should_quit());
}

#[test]
fn app_unknown_command_name_is_ignored() {
    let mut app = App::with_bindings(paths(), &[("y", "no-such-command")]);
    app.handle_stroke("y");
    assert!(!app.should_quit());
    assert_eq!(app.selected_index(), Some(0));
}

// --- search mode -----------------------------------------------------

use closure_tui::AppMode;

#[test]
fn slash_enters_search_mode() {
    let mut app = App::new(paths());
    assert_eq!(app.mode(), AppMode::Browse);
    app.handle_stroke("/");
    assert_eq!(app.mode(), AppMode::Search);
    assert_eq!(app.query(), "");
}

#[test]
fn typing_appends_to_query() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("f");
    app.handle_stroke("o");
    assert_eq!(app.query(), "fo");
}

#[test]
fn spc_stroke_appends_space_to_query() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("a");
    app.handle_stroke("SPC");
    app.handle_stroke("b");
    assert_eq!(app.query(), "a b");
}

#[test]
fn del_pops_last_query_char() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("f");
    app.handle_stroke("o");
    app.handle_stroke("DEL");
    assert_eq!(app.query(), "f");
    app.handle_stroke("DEL");
    app.handle_stroke("DEL");
    assert_eq!(app.query(), "");
}

#[test]
fn esc_cancels_search_and_keeps_selection() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    app.handle_stroke("/");
    app.handle_stroke("x");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.query(), "");
    assert_eq!(app.selected_index(), Some(1));
}

#[test]
fn search_strokes_do_not_fire_browse_commands() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("q");
    assert!(!app.should_quit());
    assert_eq!(app.query(), "q");
}

#[test]
fn results_are_fuzzy_filtered() {
    let mut app = App::new(vec![
        std::path::PathBuf::from("alpha.org"),
        std::path::PathBuf::from("beta.org"),
        std::path::PathBuf::from("notes.org"),
    ]);
    app.handle_stroke("/");
    app.handle_stroke("n");
    app.handle_stroke("s");
    let results: Vec<String> = app
        .results()
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(results, vec!["notes.org".to_owned()]);
}

#[test]
fn empty_query_results_list_every_path() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    assert_eq!(app.results().len(), 3);
}

#[test]
fn ret_accepts_best_match_and_returns_to_browse() {
    let mut app = App::new(vec![
        std::path::PathBuf::from("alpha.org"),
        std::path::PathBuf::from("beta.org"),
        std::path::PathBuf::from("notes.org"),
    ]);
    app.handle_stroke("/");
    app.handle_stroke("n");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(
        app.selected_path(),
        Some(std::path::Path::new("notes.org"))
    );
    assert_eq!(app.query(), "");
}

#[test]
fn ret_with_no_match_cancels_without_moving() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("z");
    app.handle_stroke("z");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_index(), Some(0));
}
