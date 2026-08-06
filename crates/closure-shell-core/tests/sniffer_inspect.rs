//! "[#C] build top notch UI for the network snitcher. Features include:
//! inspect, debug, network graph, MITM, block, allow, mock … And as
//! always: A discoverable UI with discoverable and productive key
//! chords."
//!
//! Allow and block existed. Inspect did not, and it is the one the
//! others are built on: a flow is a bare string in a list, and the rule
//! that decided it is invisible — so the question you actually have
//! looking at a blocked request ("why?") has no answer on screen, and
//! the question after it ("whose rule?") has none either.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::SnifferApp;
use closure_sniffer::{Action, MockBackend, Rule};

fn backend() -> MockBackend {
    MockBackend::new(vec![Rule {
        id: "config:telemetry".to_owned(),
        pattern: "telemetry.*".to_owned(),
        action: Action::Block,
    }])
}

fn app() -> SnifferApp {
    let mut app = SnifferApp::new();
    let b = backend();
    app.record("telemetry.example.com:443 tcp", &b);
    app.record("api.github.com:443 tcp", &b);
    app
}

#[test]
fn a_flow_is_taken_apart_into_the_parts_you_asked_about() {
    // "telemetry.example.com:443 tcp" is three facts wearing one
    // string, and a filter over that string cannot tell a port from a
    // hostname that contains digits.
    let app = app();
    let flow = app.inspect(0).expect("the first flow");
    assert_eq!(flow.host, "telemetry.example.com");
    assert_eq!(flow.port, Some(443));
    assert_eq!(flow.protocol, "tcp");
}

#[test]
fn it_says_which_rule_decided_and_what_that_rule_said() {
    // The question you have looking at a blocked request.
    let app = app();
    let flow = app.inspect(0).expect("a flow");
    assert_eq!(flow.action, Some(Action::Block));
    let rule = flow.rule.expect("something decided this");
    assert_eq!(rule.id, "config:telemetry");
    assert_eq!(rule.pattern, "telemetry.*");
}

#[test]
fn a_flow_nothing_matched_says_so_rather_than_looking_blocked() {
    let app = app();
    let flow = app.inspect(1).expect("the second flow");
    assert_eq!(flow.host, "api.github.com");
    assert!(flow.rule.is_none(), "invented a rule: {:?}", flow.rule);
    assert!(flow.action.is_none());
}

#[test]
fn a_rule_you_added_in_this_session_is_named_as_yours() {
    // Two rules can decide the same flow and one of them is the one you
    // just pressed `b` on. "Whose rule" is the second question.
    let mut app = app();
    app.select(1);
    app.block_selected();
    let flow = app.inspect(1).expect("a flow");
    let rule = flow.rule.expect("the rule just added");
    assert_eq!(flow.action, Some(Action::Block));
    assert!(
        rule.id.starts_with("user-"),
        "a rule added by hand does not read as yours: {}",
        rule.id
    );
}

#[test]
fn inspecting_past_the_end_is_none_rather_than_a_panic() {
    let app = app();
    assert!(app.inspect(99).is_none());
}

#[test]
fn a_flow_with_no_port_still_inspects() {
    // The candidate string is whatever the backend produced; a shape
    // this has not seen must not panic (I5).
    let mut app = SnifferApp::new();
    app.record("localhost", &backend());
    let flow = app.inspect(0).expect("a flow");
    assert_eq!(flow.host, "localhost");
    assert_eq!(flow.port, None);
    assert!(flow.protocol.is_empty());
}
