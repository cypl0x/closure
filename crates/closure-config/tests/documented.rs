//! The config file the app writes for you.
//!
//! "Put the default configuration into the vault so I can see and
//! modify it" is a documentation problem with a correctness trap in it:
//! a hand-written sample drifts from the schema the moment a key is
//! added, and then the file in every vault is wrong. So the sample is
//! *generated* from [`Config::default`], and the round-trip is a test:
//! what we write, we must be able to read back as the same defaults.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::Config;

#[test]
fn the_written_defaults_parse_back_to_the_defaults() {
    let org = Config::default_org();
    let parsed = Config::from_org_source(&org).expect("what we write, we can read");
    assert_eq!(parsed, Config::default(), "round-trip: {org}");
}

#[test]
fn every_key_is_present_even_when_it_is_the_default() {
    // The point of the file is to *show* what can be set, so a key that
    // happens to be at its default must still appear — commented out
    // where it has no value, so the round-trip stays exact.
    let org = Config::default_org();
    for key in [
        "input_mode",
        "theme",
        "view",
        "todo_keywords",
        "priority_levels",
        "tag_inheritance",
        "record_commands",
        "eval_trust",
        "sync_bind",
        "sync_advertise",
        "llm_provider",
        "llm_model",
        "llm_key_env",
        "llm_endpoint",
        "search_backend",
        "agenda_files",
        "default_vault",
        "llm_tools",
        "sniffer_blocklist",
    ] {
        assert!(org.contains(key), "`{key}` is not mentioned: {org}");
    }
}

#[test]
fn it_is_a_closure_config_block_in_a_real_org_file() {
    let org = Config::default_org();
    assert!(org.contains("#+BEGIN_SRC closure-config"), "{org}");
    assert!(org.contains("#+END_SRC"), "{org}");
    // And it parses as org, not merely as text that looks like it.
    closure_org::parse(&org).expect("valid org");
}

#[test]
fn the_security_relevant_keys_say_what_they_do() {
    // `eval_trust` is default-deny and the one key that decides whether
    // a file someone sent you can run code. A sample that lists it
    // without saying that is worse than not listing it.
    let org = Config::default_org();
    let eval_line = org
        .lines()
        .position(|l| l.contains("eval_trust"))
        .expect("mentioned");
    let context: String = org
        .lines()
        .take(eval_line + 1)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        context.contains("deny") || context.contains("nothing runs"),
        "the default-deny is explained: {context}"
    );
}
