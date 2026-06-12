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
    assert_eq!(app.selected_path(), Some(std::path::Path::new("notes.org")));
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

// --- search result cursor ---------------------------------------------

#[test]
fn search_cursor_starts_at_zero() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    assert_eq!(app.result_cursor(), 0);
}

#[test]
fn down_moves_cursor_and_clamps_at_last_result() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("o");
    assert_eq!(app.results().len(), 3, "all paths contain 'o'");
    app.handle_stroke("<down>");
    assert_eq!(app.result_cursor(), 1);
    app.handle_stroke("<down>");
    app.handle_stroke("<down>");
    app.handle_stroke("<down>");
    assert_eq!(app.result_cursor(), 2);
}

#[test]
fn up_moves_cursor_and_clamps_at_zero() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("<down>");
    app.handle_stroke("<up>");
    assert_eq!(app.result_cursor(), 0);
    app.handle_stroke("<up>");
    assert_eq!(app.result_cursor(), 0);
}

#[test]
fn editing_query_resets_cursor() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("<down>");
    assert_eq!(app.result_cursor(), 1);
    app.handle_stroke("o");
    assert_eq!(app.result_cursor(), 0);
    app.handle_stroke("<down>");
    app.handle_stroke("DEL");
    assert_eq!(app.result_cursor(), 0);
}

#[test]
fn ret_picks_cursor_item_not_best_match() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("o");
    app.handle_stroke("<down>");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(
        app.selected_path(),
        Some(std::path::Path::new("b.org")),
        "cursor was on the second result"
    );
}

#[test]
fn esc_resets_cursor_too() {
    let mut app = App::new(paths());
    app.handle_stroke("/");
    app.handle_stroke("<down>");
    app.handle_stroke("ESC");
    app.handle_stroke("/");
    assert_eq!(app.result_cursor(), 0);
}

// --- headline fuzzy search --------------------------------------------

fn rec(path: &str, id: &str, title: &str) -> closure_tui::HeadlineRecord {
    closure_tui::HeadlineRecord {
        path: std::path::PathBuf::from(path),
        id: id.to_owned(),
        title: title.to_owned(),
    }
}

fn app_with_headlines() -> App {
    let mut app = App::new(paths());
    app.set_headlines(vec![
        rec("a.org", "id-a1", "Inbox"),
        rec("a.org", "id-a2", "Inbox archive"),
        rec("b.org", "id-b1", "Project plan"),
        rec("c.org", "id-c1", "Personal notes"),
    ]);
    app
}

#[test]
fn s_enters_headline_search_mode() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    assert_eq!(app.mode(), AppMode::SearchHeadlines);
    assert_eq!(app.query(), "");
}

#[test]
fn headline_results_fuzzy_filter_titles() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    app.handle_stroke("p");
    app.handle_stroke("l");
    app.handle_stroke("a");
    app.handle_stroke("n");
    let titles: Vec<&str> = app.headline_results().iter().map(|(_, t)| *t).collect();
    assert_eq!(titles, vec!["Project plan"]);
}

#[test]
fn headline_search_empty_query_lists_all() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    assert_eq!(app.headline_results().len(), 4);
}

#[test]
fn headline_ret_jumps_to_containing_file() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    app.handle_stroke("n");
    app.handle_stroke("o");
    app.handle_stroke("t");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_path(), Some(std::path::Path::new("c.org")));
}

#[test]
fn headline_search_cursor_picks_highlighted() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    app.handle_stroke("<down>");
    app.handle_stroke("<down>");
    app.handle_stroke("RET");
    assert_eq!(app.selected_path(), Some(std::path::Path::new("b.org")));
}

#[test]
fn headline_search_esc_cancels() {
    let mut app = app_with_headlines();
    app.handle_stroke("s");
    app.handle_stroke("x");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.query(), "");
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn headline_search_without_headline_data_is_safe() {
    let mut app = App::new(paths());
    app.handle_stroke("s");
    assert!(app.headline_results().is_empty());
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_index(), Some(0));
}

// --- file body view ----------------------------------------------------

fn app_with_sources() -> App {
    let mut app = App::new(paths());
    app.set_sources(vec![
        (
            std::path::PathBuf::from("a.org"),
            "* One\nbody line\n* Two\n".to_owned(),
        ),
        (std::path::PathBuf::from("b.org"), "* Beta\n".to_owned()),
    ]);
    app
}

#[test]
fn ret_opens_file_view_on_selected_file() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::FileView);
    assert_eq!(app.view_source(), Some("* One\nbody line\n* Two\n"));
    assert_eq!(app.scroll(), 0);
}

#[test]
fn ret_without_source_data_stays_in_browse() {
    let mut app = App::new(paths());
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.view_source(), None);
}

#[test]
fn j_scrolls_down_and_clamps_at_last_line() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    app.handle_stroke("j");
    assert_eq!(app.scroll(), 1);
    for _ in 0..10 {
        app.handle_stroke("j");
    }
    assert_eq!(app.scroll(), 2, "three lines, max offset 2");
}

#[test]
fn k_scrolls_up_and_clamps_at_zero() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    app.handle_stroke("j");
    app.handle_stroke("k");
    assert_eq!(app.scroll(), 0);
    app.handle_stroke("k");
    assert_eq!(app.scroll(), 0);
}

#[test]
fn esc_closes_view_and_keeps_selection() {
    let mut app = app_with_sources();
    app.handle_stroke("j");
    app.handle_stroke("RET");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_index(), Some(1));
    assert!(!app.should_quit());
}

#[test]
fn q_in_view_closes_view_not_app() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    app.handle_stroke("q");
    assert_eq!(app.mode(), AppMode::Browse);
    assert!(!app.should_quit());
}

#[test]
fn view_scrolling_does_not_move_file_selection() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    app.handle_stroke("j");
    app.handle_stroke("j");
    app.handle_stroke("ESC");
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn reopening_view_resets_scroll() {
    let mut app = app_with_sources();
    app.handle_stroke("RET");
    app.handle_stroke("j");
    app.handle_stroke("ESC");
    app.handle_stroke("RET");
    assert_eq!(app.scroll(), 0);
}

// --- backlinks pane ----------------------------------------------------

fn app_with_backlinks() -> App {
    let mut app = App::new(paths());
    app.set_backlinks(vec![
        (
            std::path::PathBuf::from("a.org"),
            std::path::PathBuf::from("b.org"),
            "Beta note".to_owned(),
        ),
        (
            std::path::PathBuf::from("a.org"),
            std::path::PathBuf::from("c.org"),
            "Gamma note".to_owned(),
        ),
        (
            std::path::PathBuf::from("b.org"),
            std::path::PathBuf::from("a.org"),
            "Alpha note".to_owned(),
        ),
    ]);
    app
}

#[test]
fn b_enters_backlinks_mode() {
    let mut app = app_with_backlinks();
    app.handle_stroke("b");
    assert_eq!(app.mode(), AppMode::Backlinks);
}

#[test]
fn backlink_results_list_sources_for_selected_file() {
    let mut app = app_with_backlinks();
    app.handle_stroke("b");
    let rows: Vec<(String, &str)> = app
        .backlink_results()
        .iter()
        .map(|(p, t)| (p.display().to_string(), *t))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("b.org".to_owned(), "Beta note"),
            ("c.org".to_owned(), "Gamma note"),
        ]
    );
}

#[test]
fn backlink_results_empty_without_incoming_links() {
    let mut app = app_with_backlinks();
    app.handle_stroke("j");
    app.handle_stroke("j");
    app.handle_stroke("b");
    assert!(app.backlink_results().is_empty(), "c.org has no backlinks");
}

#[test]
fn backlink_cursor_and_ret_jump_to_linking_file() {
    let mut app = app_with_backlinks();
    app.handle_stroke("b");
    app.handle_stroke("<down>");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_path(), Some(std::path::Path::new("c.org")));
}

#[test]
fn backlink_esc_closes_and_keeps_selection() {
    let mut app = app_with_backlinks();
    app.handle_stroke("b");
    app.handle_stroke("<down>");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn backlink_ret_on_empty_list_returns_to_browse() {
    let mut app = App::new(paths());
    app.handle_stroke("b");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.selected_index(), Some(0));
}

// --- capture minibuffer -------------------------------------------------

#[test]
fn c_enters_capture_mode() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    assert_eq!(app.mode(), AppMode::Capture);
    assert_eq!(app.query(), "");
}

#[test]
fn capture_strokes_edit_title_not_commands() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    app.handle_stroke("q");
    app.handle_stroke("SPC");
    app.handle_stroke("j");
    assert!(!app.should_quit());
    assert_eq!(app.selected_index(), Some(0));
    assert_eq!(app.query(), "q j");
}

#[test]
fn capture_del_edits_title() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    app.handle_stroke("a");
    app.handle_stroke("b");
    app.handle_stroke("DEL");
    assert_eq!(app.query(), "a");
}

#[test]
fn capture_esc_cancels_without_request() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    app.handle_stroke("x");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.take_capture_request(), None);
}

#[test]
fn capture_ret_emits_request_once() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    app.handle_stroke("M");
    app.handle_stroke("i");
    app.handle_stroke("l");
    app.handle_stroke("k");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.take_capture_request(), Some("Milk".to_owned()));
    assert_eq!(app.take_capture_request(), None, "request is consumed");
}

#[test]
fn capture_ret_on_empty_title_is_cancel() {
    let mut app = App::new(paths());
    app.handle_stroke("c");
    app.handle_stroke("RET");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.take_capture_request(), None);
}

#[test]
fn set_paths_keeps_selection_by_path() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    app.set_paths(vec![
        std::path::PathBuf::from("b.org"),
        std::path::PathBuf::from("new.org"),
    ]);
    assert_eq!(app.selected_path(), Some(std::path::Path::new("b.org")));
}

#[test]
fn set_paths_selects_first_when_old_selection_gone() {
    let mut app = App::new(paths());
    app.handle_stroke("j");
    app.set_paths(vec![std::path::PathBuf::from("z.org")]);
    assert_eq!(app.selected_index(), Some(0));
}

// --- per-file headline list (l) -----------------------------------------

#[test]
fn l_enters_headlines_mode() {
    let mut app = app_with_headlines();
    app.handle_stroke("l");
    assert_eq!(app.mode(), AppMode::Headlines);
}

#[test]
fn headline_list_shows_only_selected_files_headlines() {
    let mut app = app_with_headlines();
    app.handle_stroke("l");
    let rows: Vec<(&str, &str)> = app.file_headlines();
    assert_eq!(rows, vec![("Inbox", "id-a1"), ("Inbox archive", "id-a2")]);
}

#[test]
fn headline_list_follows_file_selection() {
    let mut app = app_with_headlines();
    app.handle_stroke("j");
    app.handle_stroke("l");
    assert_eq!(app.file_headlines(), vec![("Project plan", "id-b1")]);
}

#[test]
fn headline_list_cursor_moves_and_clamps() {
    let mut app = app_with_headlines();
    app.handle_stroke("l");
    assert_eq!(app.result_cursor(), 0);
    app.handle_stroke("j");
    assert_eq!(app.result_cursor(), 1);
    app.handle_stroke("j");
    assert_eq!(app.result_cursor(), 1, "two headlines in a.org");
    app.handle_stroke("k");
    assert_eq!(app.result_cursor(), 0);
}

#[test]
fn headline_list_esc_and_h_close() {
    let mut app = app_with_headlines();
    app.handle_stroke("l");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    app.handle_stroke("l");
    app.handle_stroke("h");
    assert_eq!(app.mode(), AppMode::Browse);
    assert!(!app.should_quit());
}

#[test]
fn headline_list_empty_file_is_safe() {
    let mut app = App::new(paths());
    app.handle_stroke("l");
    assert_eq!(app.mode(), AppMode::Headlines);
    assert!(app.file_headlines().is_empty());
    app.handle_stroke("j");
    assert_eq!(app.result_cursor(), 0);
}
