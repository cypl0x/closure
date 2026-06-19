//! C1a: pure eval-trust policy. `eval_allowed` gates whether a language
//! may execute given the vault's `eval_trust` allowlist. Default-deny:
//! an empty allowlist runs nothing. Aliases canonicalise both sides.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_eval::{canonical_language, eval_allowed};

#[test]
fn empty_trust_denies_everything() {
    assert!(!eval_allowed(&[], "shell"));
    assert!(!eval_allowed(&[], "python"));
}

#[test]
fn trusted_language_is_allowed_including_aliases() {
    let trust = vec!["shell".to_owned()];
    assert!(eval_allowed(&trust, "shell"));
    assert!(eval_allowed(&trust, "sh"));
    assert!(eval_allowed(&trust, "bash"));
    assert!(!eval_allowed(&trust, "python"));
}

#[test]
fn alias_in_trust_entry_canonicalises() {
    // user wrote `py`; a `python` block must match.
    let trust = vec!["py".to_owned()];
    assert!(eval_allowed(&trust, "python"));
}

#[test]
fn trust_is_case_insensitive() {
    let trust = vec!["SHELL".to_owned()];
    assert!(eval_allowed(&trust, "shell"));
}

#[test]
fn canonical_language_maps_aliases() {
    assert_eq!(canonical_language("sh"), "shell");
    assert_eq!(canonical_language("BASH"), "shell");
    assert_eq!(canonical_language("py"), "python");
    assert_eq!(canonical_language("node"), "javascript");
    assert_eq!(canonical_language("rb"), "ruby");
    assert_eq!(canonical_language("brainfuck"), "brainfuck");
}
