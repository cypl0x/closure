//! Headless App-state coverage for getters/setters not exercised by the
//! interaction tests: config-error surfacing and body-search scoring.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use closure_tui::App;

#[test]
fn config_error_roundtrips() {
    let mut app = App::new(vec![]);
    assert_eq!(app.config_error(), None);
    app.set_config_error(Some("bad llm_provider".to_owned()));
    assert_eq!(app.config_error(), Some("bad llm_provider"));
    app.set_config_error(None);
    assert_eq!(app.config_error(), None);
}

#[test]
fn body_results_empty_query_is_empty() {
    let mut app = App::new(vec![]);
    app.set_sources(vec![(PathBuf::from("a.org"), "hello world\n".to_owned())]);
    // No query typed yet -> no hits (early return).
    assert!(app.body_results().is_empty());
}

#[test]
fn body_results_ranks_better_fuzzy_match_first() {
    let mut app = App::new(vec![PathBuf::from("a.org")]);
    app.set_sources(vec![(
        PathBuf::from("a.org"),
        "p a r s e r spread\nparser exact\n".to_owned(),
    )]);
    app.handle_stroke("S");
    for c in "parser".chars() {
        app.handle_stroke(&c.to_string());
    }
    let rows = app.body_results();
    assert_eq!(rows.len(), 2, "both lines match: {rows:?}");
    // Contiguous "parser" outscores the spread-out match.
    assert!(rows[0].1.contains("parser exact"), "ranked: {rows:?}");
}
