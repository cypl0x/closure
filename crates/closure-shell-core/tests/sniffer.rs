//! V7a: headless sniffer surface. `SnifferApp` is a pure state machine
//! over the capture trait — a live event list, cursor, substring filter,
//! and per-flow allow/block toggles that mutate the blocklist rules —
//! the same headless-tested pattern as the launcher `App`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::SnifferApp;
use closure_sniffer::{Action, MockBackend, Rule};

fn backend() -> MockBackend {
    MockBackend::new(vec![Rule {
        id: "ads".to_owned(),
        pattern: "ads.*".to_owned(),
        action: Action::Block,
    }])
}

#[test]
fn records_events_with_their_decided_action() {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("ads.example:443 TCP", &b);
    app.record("api.example:443 TCP", &b);
    assert_eq!(app.events().len(), 2);
    assert_eq!(
        app.events()[0].action,
        Some(Action::Block),
        "ads matched the block rule"
    );
    assert_eq!(app.events()[1].action, None, "api had no rule");
}

#[test]
fn filter_narrows_the_event_list() {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("ads.example:443 TCP", &b);
    app.record("api.example:443 TCP", &b);
    app.set_filter("api");
    let shown = app.filtered();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].candidate.contains("api"));
}

#[test]
fn block_selected_adds_a_block_rule_and_redecides() {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("api.example:443 TCP", &b);
    assert_eq!(app.events()[0].action, None);
    app.select(0);
    app.block_selected();
    // The flow is now blocked, and a rule exists for it.
    assert_eq!(app.events()[0].action, Some(Action::Block));
    assert!(app.rules().iter().any(|r| r.action == Action::Block));
}

#[test]
fn allow_selected_adds_an_allow_rule() {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("ads.example:443 TCP", &b); // blocked by the backend rule
    app.select(0);
    app.allow_selected();
    assert_eq!(
        app.events()[0].action,
        Some(Action::Allow),
        "user override allows it"
    );
}

#[test]
fn detail_describes_the_selected_flow() {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("ads.example:443 TCP", &b);
    app.select(0);
    let d = app.detail().expect("a selected flow");
    assert!(d.contains("ads.example"));
    assert!(d.contains("Block"));
}

#[test]
fn empty_app_has_no_detail() {
    let app = SnifferApp::new();
    assert!(app.detail().is_none());
}
