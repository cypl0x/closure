#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_sniffer::{Action, Rule, glob_matches, match_first};

#[test]
fn literal_match() {
    assert!(glob_matches("hello", "hello"));
    assert!(!glob_matches("hello", "hell"));
    assert!(!glob_matches("hello", "helloo"));
}

#[test]
fn star_matches_any_run() {
    assert!(glob_matches("*", ""));
    assert!(glob_matches("*", "anything goes"));
    assert!(glob_matches("a*c", "abc"));
    assert!(glob_matches("a*c", "abxc"));
    assert!(glob_matches("a*c", "ac"));
    assert!(!glob_matches("a*c", "ab"));
    assert!(!glob_matches("a*c", "abx"));
}

#[test]
fn star_at_end() {
    assert!(glob_matches("https://*", "https://example.com"));
    assert!(!glob_matches("https://*", "http://example.com"));
}

#[test]
fn match_first_picks_first_match() {
    let rules = vec![
        Rule {
            id: "deny-trackers".into(),
            pattern: "*tracker*".into(),
            action: Action::Block,
        },
        Rule {
            id: "allow-rest".into(),
            pattern: "*".into(),
            action: Action::Allow,
        },
    ];
    let m = match_first("https://tracker.example.com/p", &rules).unwrap();
    assert_eq!(m.id, "deny-trackers");
    let m = match_first("https://example.com", &rules).unwrap();
    assert_eq!(m.id, "allow-rest");
}

// TDD test written *first* for sniffer capture backend trait (first sub of [0/3]).
// Requires the trait + mock backend. Will fail to compile until implemented.
#[test]
fn capture_backend_trait_with_mock() {
    use closure_sniffer::CaptureBackend; // will not exist

    let rules = vec![
        closure_sniffer::Rule {
            id: "block-evil".into(),
            pattern: "*evil*".into(),
            action: closure_sniffer::Action::Block,
        },
    ];
    let mock = closure_sniffer::MockBackend::new(rules);
    let action = mock.match_action("https://evil-tracker.com");
    assert!(matches!(action, Some(closure_sniffer::Action::Block)));

    let allow = mock.match_action("https://good.com");
    assert!(matches!(allow, None)); // or default allow, depending on impl
}
