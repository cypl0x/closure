//! Writing one key back into `config.org` without touching the rest.
//!
//! Pairing is the case that needs it: you paste a ticket once and the
//! peer should still be there tomorrow, rather than being re-pasted
//! every session. The vault is plain files (I1), so the place to keep
//! that is the config file people can already read — which means the
//! app has to be able to write *one line* of it without eating the
//! comments, the ordering, or the keys it does not know about.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, set_config_key};

const FILE: &str = "\
#+TITLE: closure configuration

Some prose above the block.

#+BEGIN_SRC closure-config
# How you type.
input_mode = vim

# Colours.
theme = doom-vibrant
#+END_SRC

Some prose below it.
";

#[test]
fn an_existing_key_is_replaced_in_place() {
    let out = set_config_key(FILE, "theme", "light").expect("rewritten");
    assert!(out.contains("theme = light"), "{out}");
    assert!(!out.contains("theme = doom-vibrant"), "{out}");
    assert!(out.contains("input_mode = vim"), "the others stay: {out}");
}

#[test]
fn a_new_key_is_added_inside_the_block() {
    let out = set_config_key(FILE, "sync_peers", "closure-sync:1.2.3.4:7420|ab").expect("ok");
    let cfg = Config::from_org_source(&out).expect("still parses");
    assert_eq!(cfg.sync_peers, vec!["closure-sync:1.2.3.4:7420|ab"]);
}

#[test]
fn the_prose_and_the_comments_survive() {
    let out = set_config_key(FILE, "theme", "light").expect("rewritten");
    assert!(out.contains("Some prose above the block."), "{out}");
    assert!(out.contains("Some prose below it."), "{out}");
    assert!(out.contains("# How you type."), "{out}");
    assert!(out.contains("# Colours."), "{out}");
}

#[test]
fn a_file_with_no_block_grows_one() {
    // A vault whose config.org is prose only, or missing entirely.
    let out = set_config_key("#+TITLE: mine\n", "theme", "light").expect("ok");
    let cfg = Config::from_org_source(&out).expect("parses");
    assert_eq!(cfg.theme, "light");
    assert!(out.contains("#+TITLE: mine"), "{out}");
}

#[test]
fn the_result_always_parses_back() {
    let mut file = FILE.to_owned();
    for (k, v) in [
        ("theme", "light"),
        ("input_mode", "doom"),
        ("sync_peers", "a, b"),
        ("theme", "dark"),
    ] {
        file = set_config_key(&file, k, v).expect("rewritten");
        Config::from_org_source(&file).expect("still valid after every write");
    }
    let cfg = Config::from_org_source(&file).expect("parses");
    assert_eq!(cfg.theme, "dark", "the last write wins");
    assert_eq!(cfg.sync_peers.len(), 2);
}

#[test]
fn peers_round_trip_as_a_list() {
    let out = set_config_key(FILE, "sync_peers", "one, two, three").expect("ok");
    let cfg = Config::from_org_source(&out).expect("parses");
    assert_eq!(cfg.sync_peers, vec!["one", "two", "three"]);
}
