//! Every config key names a constraint, and every constraint a key.
//!
//! `Config::load` already rejects unknown keys with a suggestion and a
//! line number, and enforces real cross-key rules — a `llm_endpoint`
//! with no `llm_provider`, a BYOK provider with no `llm_key_env`. Those
//! are good errors and they arrive at load, which is I9.
//!
//! What was missing is the property CUE actually gives you: constraints
//! as *data*. They were hand-written `if`s in one long function, so
//! nothing related a key to its rules, a key added tomorrow was
//! unconstrained by default, and no test said so. This is the same
//! ratchet shape as `one_struct` and the conformance matrix — the two
//! guards that have already caught real omissions here.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, KEY_RULES, ValueKind};

#[test]
fn every_known_key_has_a_rule() {
    for key in Config::known_keys() {
        assert!(
            KEY_RULES.iter().any(|r| r.key == key),
            "`{key}` is a config key with no rule. A new key is \
             unconstrained until something here says what it accepts — \
             mark it `ValueKind::Free` if that is genuinely the answer."
        );
    }
}

#[test]
fn every_rule_names_a_key_that_exists() {
    // The other direction: a rule for a key nobody can set is a rule
    // nobody runs, and reads as coverage that is not there.
    let known = Config::known_keys();
    for rule in KEY_RULES {
        assert!(
            known.contains(&rule.key.to_owned()),
            "`{}` has a rule but is not a config key",
            rule.key
        );
    }
}

#[test]
fn nothing_is_ruled_twice() {
    let mut seen: Vec<&str> = KEY_RULES.iter().map(|r| r.key).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(before, seen.len(), "a key has two rules");
}

#[test]
fn a_rule_that_requires_another_key_names_a_real_one() {
    for rule in KEY_RULES {
        for needed in rule.requires {
            assert!(
                KEY_RULES.iter().any(|r| r.key == *needed),
                "`{}` requires `{needed}`, which is not a key",
                rule.key
            );
        }
    }
}

#[test]
fn free_is_used_sparingly_enough_to_mean_something() {
    // `Free` is the honest escape hatch and also the way to make this
    // whole gate vacuous. If most keys are free the rules are theatre.
    let free = KEY_RULES
        .iter()
        .filter(|r| matches!(r.kind, ValueKind::Free))
        .count();
    assert!(
        free * 2 < KEY_RULES.len(),
        "{free} of {} keys are unconstrained — the table is not saying much",
        KEY_RULES.len()
    );
}

#[test]
fn a_bool_key_rejects_a_non_bool_at_load() {
    // The rules have to be the thing that runs, not a second list that
    // agrees with the code by coincidence.
    let src = "#+BEGIN_SRC closure-config\nwrap = maybe\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("wrap"), "{msg}");
}

#[test]
fn a_url_key_rejects_something_that_is_not_one() {
    let src = "#+BEGIN_SRC closure-config\nllm_provider = openai-compatible\n\
               llm_key_env = K\nllm_endpoint = not-a-url\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    assert!(format!("{err}").contains("llm_endpoint"), "{err}");
}

#[test]
fn a_number_key_rejects_a_word_instead_of_ignoring_it() {
    // The proof that the table is what runs. `outline_width` used to
    // parse with `.ok()` and silently keep the default, so a hand-typed
    // `outline_width = wide` did nothing and said nothing — the exact
    // failure I9 exists to prevent, in the one key whose arm had no
    // error path at all.
    let src = "#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n";
    let err = Config::from_org_source(src)
        .expect_err("a width that is not a number was accepted in silence");
    let msg = format!("{err}");
    assert!(msg.contains("outline_width"), "{msg}");
    assert!(msg.contains('2'), "the error does not name the line: {msg}");
}

#[test]
fn a_key_the_parser_accepts_is_a_key_the_user_can_discover() {
    // `log_done`, `outline_width` and `recent_files` worked and were
    // absent from `known_keys()`, so `nearest_key` could not suggest
    // them and the shipped default config did not mention them. A
    // setting nobody can find is not a setting.
    let known = Config::known_keys();
    for k in ["log_done", "outline_width", "recent_files"] {
        assert!(known.contains(&k.to_owned()), "`{k}` is undiscoverable");
    }
}
