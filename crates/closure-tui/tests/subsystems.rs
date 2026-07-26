//! The subsystem panes the GUI had and the terminal did not: the
//! sniffer, sync peers, the LLM transcript, and merge conflicts.
//!
//! They follow the shell's existing driver contract — the driver pushes
//! rows in, the pane parks a request on the way out — so the terminal
//! never grows its own network or model client.

use std::path::PathBuf;

use closure_tui::{App, AppMode, ConflictRow};

fn paths() -> Vec<PathBuf> {
    vec![PathBuf::from("a.org")]
}

fn app() -> App {
    App::new(paths())
}

// === Sniffer. ===

#[test]
fn g_n_opens_the_sniffer_with_the_driver_s_flows() {
    let mut app = app();
    app.set_sniffer(vec![
        ("api.example.com".to_owned(), "allow".to_owned()),
        ("tracker.example".to_owned(), "block".to_owned()),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("n");
    assert_eq!(app.mode(), AppMode::Sniffer);
    assert_eq!(app.sniffer_rows().len(), 2);
    assert!(app.sniffer_rows()[0].contains("api.example.com"));
    assert!(app.sniffer_rows()[0].contains("allow"));
}

#[test]
fn block_and_allow_flow_park_a_rule_for_the_cursor_flow() {
    let mut app = app();
    app.set_sniffer(vec![
        ("api.example.com".to_owned(), "allow".to_owned()),
        ("tracker.example".to_owned(), "allow".to_owned()),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("n");
    app.handle_stroke("j"); // cursor on the tracker
    app.handle_stroke("g");
    app.handle_stroke("b"); // block-flow
    assert_eq!(
        app.take_flow_request(),
        Some(("tracker.example".to_owned(), false))
    );
    app.handle_stroke("g");
    app.handle_stroke("w"); // allow-flow
    assert_eq!(
        app.take_flow_request(),
        Some(("tracker.example".to_owned(), true))
    );
}

#[test]
fn flow_rules_need_a_flow_under_the_cursor() {
    let mut app = app();
    app.handle_stroke("g");
    app.handle_stroke("b");
    assert_eq!(app.take_flow_request(), None);
    assert!(
        app.status().contains("no flow"),
        "status was {:?}",
        app.status()
    );
}

// === Sync. ===

#[test]
fn g_s_opens_the_peer_list() {
    let mut app = app();
    app.set_peers(vec![
        ("127.0.0.1:4001".to_owned(), "connected".to_owned()),
        ("127.0.0.1:4002".to_owned(), "idle".to_owned()),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("s");
    assert_eq!(app.mode(), AppMode::Sync);
    assert_eq!(app.peer_rows().len(), 2);
    assert!(app.peer_rows()[0].contains("connected"));
}

#[test]
fn the_sync_pane_closes_on_escape() {
    let mut app = app();
    app.handle_stroke("g");
    app.handle_stroke("s");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
}

// === LLM. ===

#[test]
fn g_i_opens_the_transcript() {
    let mut app = app();
    app.set_chat(vec![
        ("user".to_owned(), "what is org-mode?".to_owned()),
        ("assistant".to_owned(), "a plain-text system".to_owned()),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("i");
    assert_eq!(app.mode(), AppMode::Llm);
    assert_eq!(app.chat_rows().len(), 2);
    assert!(app.chat_rows()[0].contains("what is org-mode?"));
}

#[test]
fn g_r_toggles_whether_the_answer_is_rendered() {
    let mut app = app();
    assert!(app.llm_render(), "rendering is the default");
    app.handle_stroke("g");
    app.handle_stroke("r");
    assert!(!app.llm_render());
    app.handle_stroke("g");
    app.handle_stroke("r");
    assert!(app.llm_render());
}

#[test]
fn typing_in_the_llm_pane_asks_a_question() {
    let mut app = app();
    app.handle_stroke("g");
    app.handle_stroke("i");
    app.handle_stroke("i"); // start composing
    app.handle_stroke("h");
    app.handle_stroke("i");
    app.handle_stroke("RET");
    assert_eq!(app.take_ask_request(), Some("hi".to_owned()));
}

// === Conflicts. ===

fn conflict() -> ConflictRow {
    ConflictRow {
        block: "id-1".to_owned(),
        field: "title".to_owned(),
        ours: "Ours".to_owned(),
        theirs: "Theirs".to_owned(),
    }
}

#[test]
fn g_m_lists_the_conflicts_with_both_sides() {
    let mut app = app();
    app.set_conflicts(vec![conflict()]);
    app.handle_stroke("g");
    app.handle_stroke("m");
    assert_eq!(app.mode(), AppMode::Conflicts);
    let rows = app.conflict_rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("Ours"), "row was {:?}", rows[0]);
    assert!(rows[0].contains("Theirs"), "row was {:?}", rows[0]);
}

#[test]
fn resolving_a_title_conflict_renames_through_the_usual_channel() {
    let mut app = app();
    app.set_conflicts(vec![conflict()]);
    app.handle_stroke("g");
    app.handle_stroke("m");
    app.handle_stroke("g");
    app.handle_stroke("t"); // resolve-theirs
    assert_eq!(
        app.take_rename_request(),
        Some(("id-1".to_owned(), "Theirs".to_owned()))
    );
    assert!(app.conflict_rows().is_empty(), "the conflict is resolved");
}

#[test]
fn resolving_a_body_conflict_writes_the_body() {
    let mut app = app();
    app.set_conflicts(vec![ConflictRow {
        block: "id-2".to_owned(),
        field: "body".to_owned(),
        ours: "our body".to_owned(),
        theirs: "their body".to_owned(),
    }]);
    app.handle_stroke("g");
    app.handle_stroke("m");
    app.handle_stroke("g");
    app.handle_stroke("o"); // resolve-ours
    assert_eq!(
        app.take_body_request(),
        Some(("id-2".to_owned(), "our body".to_owned()))
    );
}

// === Empty states. ===

#[test]
fn an_unfed_pane_says_why_it_is_empty() {
    // The driver pushes these rows in; the terminal binary does not run
    // the sniffer, the sync transport or a model client yet, so the
    // panes must explain themselves rather than show a blank box.
    for (chord, want) in [
        (("g", "n"), "sniffer"),
        (("g", "s"), "peer"),
        (("g", "i"), "model"),
        (("g", "m"), "conflict"),
    ] {
        let mut app = app();
        app.handle_stroke(chord.0);
        app.handle_stroke(chord.1);
        let rows = app.pane_rows();
        assert_eq!(rows.len(), 1, "mode {:?} rows {rows:?}", app.mode());
        assert!(
            rows[0].contains(want),
            "mode {:?} said {:?}, wanted {want:?}",
            app.mode(),
            rows[0]
        );
    }
}

#[test]
fn a_fed_pane_shows_its_rows_not_the_empty_state() {
    let mut app = app();
    app.set_peers(vec![("127.0.0.1:4001".to_owned(), "connected".to_owned())]);
    app.handle_stroke("g");
    app.handle_stroke("s");
    assert_eq!(app.pane_rows().len(), 1);
    assert!(app.pane_rows()[0].contains("127.0.0.1:4001"));
}

#[test]
fn resolving_without_a_conflict_is_a_no_op() {
    let mut app = app();
    app.handle_stroke("g");
    app.handle_stroke("o");
    assert_eq!(app.take_rename_request(), None);
    assert!(
        app.status().contains("no conflict"),
        "status was {:?}",
        app.status()
    );
}
