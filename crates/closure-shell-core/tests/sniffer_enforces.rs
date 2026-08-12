//! A blocked host is a host closure will not talk to.
//!
//! The sniffer had `Action::Block`, rules that matched, a real pcap
//! backend and a pane that listed verdicts — and nothing anywhere acted
//! on one. It was an observer with opinions: it could tell you a
//! connection should not have happened, after it had.
//!
//! What closure can honestly enforce is its own outbound traffic. It
//! is not a kernel module and not a proxy, so policing the machine is
//! a different product; refusing to send a prompt to a host the user
//! blocked is this one, and it is the traffic a PKM actually
//! originates.
//!
//! Checked against the provider's own endpoint rather than against
//! config, for the reason `Provider::endpoint` exists: the config named
//! an endpoint once and the code dialled somewhere else, so only the
//! thing doing the dialling can be believed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::outbound_verdict;
use closure_sniffer::{Action, Rule};

fn blocklist(patterns: &[&str]) -> Vec<Rule> {
    patterns
        .iter()
        .map(|p| Rule {
            id: (*p).to_owned(),
            pattern: (*p).to_owned(),
            action: Action::Block,
        })
        .collect()
}

#[test]
fn a_blocked_host_is_refused() {
    let rules = blocklist(&["api.openai.com*"]);
    let v = outbound_verdict(Some("https://api.openai.com/v1/chat/completions"), &rules);
    assert_eq!(v, Some(Action::Block));
}

#[test]
fn a_host_nobody_blocked_goes_through() {
    let rules = blocklist(&["api.openai.com*"]);
    assert_eq!(
        outbound_verdict(Some("http://localhost:11434/api/generate"), &rules),
        None
    );
}

#[test]
fn a_provider_that_never_leaves_the_process_is_not_checked() {
    // An echo provider has no endpoint. Blocking one would be a rule
    // about nothing, and erroring on `None` would make an offline
    // model impossible to use with any blocklist at all.
    let rules = blocklist(&["*"]);
    assert_eq!(outbound_verdict(None, &rules), None);
}

#[test]
fn the_rule_matches_the_host_not_the_whole_url() {
    // A blocklist is a list of hosts. If the path counted, blocking
    // `api.openai.com` would miss every request with a query string.
    let rules = blocklist(&["api.openai.com*"]);
    assert_eq!(
        outbound_verdict(Some("https://api.openai.com/v1/x?y=1"), &rules),
        Some(Action::Block)
    );
}

#[test]
fn an_allow_rule_wins_over_a_broader_block() {
    // `match_first` is first-match, so a specific allow written above a
    // wildcard block is how somebody permits one host — and if that did
    // not work the only usable blocklist would be an empty one.
    let rules = vec![
        Rule {
            id: "allow-anthropic".to_owned(),
            pattern: "api.anthropic.com*".to_owned(),
            action: Action::Allow,
        },
        Rule {
            id: "block-the-rest".to_owned(),
            pattern: "*".to_owned(),
            action: Action::Block,
        },
    ];
    assert_eq!(
        outbound_verdict(Some("https://api.anthropic.com/v1/messages"), &rules),
        None
    );
    assert_eq!(
        outbound_verdict(Some("https://elsewhere.example/v1"), &rules),
        Some(Action::Block)
    );
}

#[test]
fn an_empty_blocklist_blocks_nothing() {
    // The default. A sniffer that refused everything until configured
    // would be a broken program rather than a careful one.
    assert_eq!(
        outbound_verdict(Some("https://api.openai.com/v1"), &[]),
        None
    );
}

#[test]
fn a_url_that_is_not_one_is_not_silently_allowed() {
    // Failing open on something unparseable is how a blocklist gets
    // bypassed by a typo. Unparseable is checked whole.
    let rules = blocklist(&["*nonsense*"]);
    assert_eq!(
        outbound_verdict(Some("nonsense"), &rules),
        Some(Action::Block)
    );
}
