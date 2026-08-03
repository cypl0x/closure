//! Writing one setting back into `config.org` without disturbing the
//! rest of it.
//!
//! "built setup UI for the Assistant — it should get persisted in the
//! config.org file. It just UI for all of the config options for the
//! file." A settings screen is only as good as its save, and the save
//! is the part that can lose work: `config.org` is a file the user
//! writes by hand, with their comments and their ordering in it, and a
//! UI that rewrote the whole block from the parsed struct would
//! silently delete every comment and every key it does not know about.
//!
//! So the write is a splice, not a render. Everything not being set is
//! returned byte for byte.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::set_key;

const BLOCK: &str = "\
Some prose above the block.

#+BEGIN_SRC closure-config
# How you type.
input_mode = doom
theme = doom-vibrant
# llm_endpoint = http://localhost:11434
#+END_SRC

Prose below.
";

#[test]
fn an_existing_key_is_replaced_in_place() {
    let out = set_key(BLOCK, "theme", "dark");
    assert!(out.contains("theme = dark"), "{out}");
    assert!(!out.contains("theme = doom-vibrant"), "{out}");
}

#[test]
fn everything_else_survives_byte_for_byte() {
    // The whole reason this is a splice: a user's comments and their
    // ordering are theirs.
    let out = set_key(BLOCK, "theme", "dark");
    assert!(out.contains("Some prose above the block."), "{out}");
    assert!(out.contains("# How you type."), "{out}");
    assert!(out.contains("input_mode = doom"), "{out}");
    assert!(out.contains("Prose below."), "{out}");
    assert_eq!(
        out.lines().count(),
        BLOCK.lines().count(),
        "the line count changed:\n{out}"
    );
}

#[test]
fn a_commented_key_is_uncommented_rather_than_duplicated() {
    // The shipped template writes the optional keys as comments. A
    // setting screen that appended a second live line would leave the
    // file saying it twice, and the reader takes one of them.
    let out = set_key(BLOCK, "llm_endpoint", "http://127.0.0.1:8080");
    assert!(
        out.contains("llm_endpoint = http://127.0.0.1:8080"),
        "{out}"
    );
    assert_eq!(
        out.matches("llm_endpoint").count(),
        1,
        "the key is now in the file twice:\n{out}"
    );
}

#[test]
fn a_new_key_lands_inside_the_block() {
    // Not after `#+END_SRC`, where the reader would never look at it.
    let out = set_key(BLOCK, "llm_model", "claude-opus-4");
    let block_end = out.find("#+END_SRC").expect("the block still ends");
    let key_at = out.find("llm_model").expect("the key was written");
    assert!(
        key_at < block_end,
        "the key landed outside the block:\n{out}"
    );
}

#[test]
fn a_file_with_no_block_gets_one() {
    // Opening the settings screen on a vault whose config.org is only
    // prose must not throw the prose away.
    let out = set_key("just some notes\n", "llm_provider", "ollama");
    assert!(out.contains("just some notes"), "{out}");
    assert!(out.contains("#+BEGIN_SRC closure-config"), "{out}");
    assert!(out.contains("llm_provider = ollama"), "{out}");
    assert!(out.contains("#+END_SRC"), "{out}");
}

#[test]
fn an_empty_file_gets_a_whole_block() {
    let out = set_key("", "llm_provider", "openai");
    let out = set_key(&out, "llm_key_env", "OPENAI_API_KEY");
    assert!(out.contains("#+BEGIN_SRC closure-config"), "{out}");
    let parsed = closure_config::Config::from_org_source(&out).expect("it parses back");
    assert_eq!(parsed.llm_provider.as_deref(), Some("openai"));
}

#[test]
fn naming_a_provider_without_a_key_variable_is_rejected() {
    // Not a `set_key` rule but the one the settings screen has to
    // respect: the loader requires `llm_key_env` once a provider is
    // named, so a UI that writes the provider on its own has written a
    // config.org that will not load at all. Saving one field at a time
    // is therefore not safe for these two.
    let half = set_key("", "llm_provider", "openai");
    assert!(
        closure_config::Config::from_org_source(&half).is_err(),
        "a provider with no key variable loaded anyway:\n{half}"
    );
}

#[test]
fn what_is_written_is_what_is_read_back() {
    // The round trip is the only thing the settings screen actually
    // promises.
    let out = set_key(BLOCK, "llm_provider", "ollama");
    let out = set_key(&out, "llm_model", "llama3");
    let out = set_key(&out, "llm_endpoint", "http://localhost:11434");
    let cfg = closure_config::Config::from_org_source(&out).expect("parses");
    assert_eq!(cfg.llm_provider.as_deref(), Some("ollama"));
    assert_eq!(cfg.llm_model.as_deref(), Some("llama3"));
    assert_eq!(cfg.llm_endpoint.as_deref(), Some("http://localhost:11434"));
    // and the untouched key is still what it was
    assert_eq!(cfg.theme, "doom-vibrant");
}

#[test]
fn setting_a_key_twice_leaves_one_line() {
    let out = set_key(BLOCK, "llm_model", "a");
    let out = set_key(&out, "llm_model", "b");
    assert_eq!(out.matches("llm_model").count(), 1, "{out}");
    assert!(out.contains("llm_model = b"), "{out}");
}

#[test]
fn an_empty_value_clears_the_setting_rather_than_writing_a_blank() {
    // "Unset" has to be reachable from a UI, and `key = ` parses as a
    // key with an empty value rather than as an absent key.
    let out = set_key(BLOCK, "theme", "");
    let cfg = closure_config::Config::from_org_source(&out).expect("parses");
    assert_eq!(cfg.theme, closure_config::Config::default().theme, "{out}");
}

#[test]
fn a_lowercase_block_is_the_same_block() {
    // Org keywords are case-insensitive and people write them
    // lowercase. Matching only `#+BEGIN_SRC` meant the block was not
    // found, a *second* one was appended, and the reader takes the
    // first — so the setting silently did nothing. Caught on :1 by
    // looking at config.org after the save, not by any test above.
    let lower = "\
#+begin_src closure-config
llm_provider = openai
llm_key_env = K
#+end_src
";
    let out = set_key(lower, "llm_endpoint", "http://127.0.0.1:8080");
    assert_eq!(
        out.to_lowercase().matches("#+begin_src").count(),
        1,
        "a second block was appended:\n{out}"
    );
    let cfg = closure_config::Config::from_org_source(&out).expect("parses");
    assert_eq!(cfg.llm_endpoint.as_deref(), Some("http://127.0.0.1:8080"));
}

#[test]
fn a_lowercase_key_line_is_replaced_not_duplicated() {
    let lower = "#+begin_src closure-config\nllm_model = old\n#+end_src\n";
    let out = set_key(lower, "llm_model", "new");
    assert_eq!(out.matches("llm_model").count(), 1, "{out}");
    assert!(out.contains("llm_model = new"), "{out}");
}
