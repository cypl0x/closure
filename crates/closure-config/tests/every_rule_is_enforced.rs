//! Every constraint in `KEY_RULES` actually refuses something.
//!
//! The rules became data a few commits ago and `every_key_is_constrained`
//! checks the table is *complete*. What it does not check is that each
//! rule has teeth: a `ValueKind` nothing rejects is a row in a table and
//! not a constraint, and it would satisfy the completeness gate exactly
//! as well as a real one.
//!
//! So this pairs every kind with a value it must refuse and one it must
//! accept. Both directions, because a rule that refuses everything is as
//! useless as one that refuses nothing — and easier to write by
//! accident.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, KEY_RULES, ValueKind};

fn load(key: &str, value: &str) -> Result<Config, closure_config::ConfigError> {
    Config::from_org_source(&format!(
        "* Settings\n#+BEGIN_SRC closure-config\n{key} = {value}\n#+END_SRC\n"
    ))
}

/// A value each kind must refuse, and one it must take.
const fn samples(kind: ValueKind) -> Option<(&'static str, &'static str)> {
    match kind {
        ValueKind::Bool => Some(("maybe", "true")),
        ValueKind::PositiveInt => Some(("wide", "40")),
        ValueKind::Url => Some(("not-a-url", "https://example.com")),
        ValueKind::SocketAddr => Some(("nowhere", "127.0.0.1:7000")),
        ValueKind::IpAddr => Some(("not-an-ip", "127.0.0.1")),
        // `OneOf` needs the key's own list, so it is handled below.
        ValueKind::OneOf(_) | ValueKind::Path | ValueKind::List | ValueKind::Free => None,
    }
}

#[test]
fn every_constrained_kind_refuses_something() {
    let mut checked = 0usize;
    for rule in KEY_RULES {
        let Some((bad, _good)) = samples(rule.kind) else {
            continue;
        };
        assert!(
            load(rule.key, bad).is_err(),
            "`{} = {bad}` was accepted — the {:?} rule has no teeth",
            rule.key,
            rule.kind
        );
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} rules were exercised");
}

#[test]
fn every_one_of_rule_refuses_a_value_outside_its_list() {
    let mut checked = 0usize;
    for rule in KEY_RULES {
        let ValueKind::OneOf(allowed) = rule.kind else {
            continue;
        };
        assert!(!allowed.is_empty(), "`{}` allows nothing at all", rule.key);
        assert!(
            load(rule.key, "definitely-not-in-the-list").is_err(),
            "`{}` took a value outside its own list",
            rule.key
        );
        checked += 1;
    }
    assert!(checked >= 2, "only {checked} enum rules were exercised");
}

#[test]
fn the_error_names_the_key_that_was_wrong() {
    // An error that says "invalid value" without saying which line sends
    // the reader through the whole file.
    let err = load("wrap", "maybe").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("wrap"), "{msg}");
}

#[test]
fn a_rule_that_accepts_still_accepts() {
    // The other direction. A `check_value` that returned `Err` for
    // everything would satisfy every assertion above.
    for rule in KEY_RULES {
        let Some((_bad, good)) = samples(rule.kind) else {
            continue;
        };
        // `llm_endpoint` requires `llm_provider`, so a lone assignment
        // is legitimately refused — that is the cross-key rule doing its
        // job rather than the value rule failing.
        if !rule.requires.is_empty() {
            continue;
        }
        assert!(
            load(rule.key, good).is_ok(),
            "`{} = {good}` was refused",
            rule.key
        );
    }
}

#[test]
fn a_free_key_takes_what_it_is_given() {
    // `Free` is the honest escape hatch, and it has to actually be free
    // or it is a constraint nobody wrote down.
    for rule in KEY_RULES {
        if !matches!(rule.kind, ValueKind::Free) || !rule.requires.is_empty() {
            continue;
        }
        assert!(
            load(rule.key, "anything at all").is_ok(),
            "`{}` is marked Free and refused a value",
            rule.key
        );
    }
}
