//! closure-config integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_config::{Config, ConfigError, InputMode};

#[test]
fn default_is_doom_mode_and_default_theme() {
    let c = Config::default();
    assert_eq!(c.input_mode, InputMode::Doom);
    assert_eq!(c.theme, "default");
    assert_eq!(c.todo_keywords, vec!["TODO".to_owned(), "DONE".to_owned()]);
}

#[test]
fn load_from_org_block() {
    let src = "#+BEGIN_SRC closure-config\n\
               input_mode = vim\n\
               theme = \"solarized\"\n\
               todo_keywords = TODO, DOING, DONE\n\
               #+END_SRC\n";
    let c = Config::from_org_source(src).expect("load");
    assert_eq!(c.input_mode, InputMode::Vim);
    assert_eq!(c.theme, "solarized");
    assert_eq!(c.todo_keywords, vec!["TODO", "DOING", "DONE"]);
}

#[test]
fn missing_block_is_error() {
    let src = "* Heading\nbody\n";
    let err = Config::from_org_source(src).unwrap_err();
    assert!(matches!(err, ConfigError::Missing));
}

#[test]
fn unknown_key_is_rejected_at_load_time() {
    let src = "#+BEGIN_SRC closure-config\nnope = x\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    assert!(matches!(err, ConfigError::UnknownKey(k) if k == "nope"));
}

#[test]
fn unknown_input_mode_is_rejected() {
    let src = "#+BEGIN_SRC closure-config\ninput_mode = whatever\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    assert!(matches!(err, ConfigError::BadValue { .. }));
}

#[test]
fn priority_levels_loaded() {
    let src = "#+BEGIN_SRC closure-config\npriority_levels = A, B, C, D\n#+END_SRC\n";
    let c = Config::from_org_source(src).unwrap();
    assert_eq!(c.priority_levels, vec!['A', 'B', 'C', 'D']);
}

#[test]
fn priority_levels_lowercase_rejected() {
    let src = "#+BEGIN_SRC closure-config\npriority_levels = a, b\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    assert!(matches!(err, ConfigError::BadValue { .. }));
}

#[test]
fn tag_inheritance_loaded() {
    let src = "#+BEGIN_SRC closure-config\ntag_inheritance = false\n#+END_SRC\n";
    let c = Config::from_org_source(src).unwrap();
    assert!(!c.tag_inheritance);
}

#[test]
fn agenda_files_loaded() {
    let src = "#+BEGIN_SRC closure-config\nagenda_files = a.org, b.org\n#+END_SRC\n";
    let c = Config::from_org_source(src).unwrap();
    assert_eq!(c.agenda_files.len(), 2);
}

#[test]
fn config_loads_from_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.org");
    std::fs::write(
        &path,
        "#+BEGIN_SRC closure-config\ninput_mode = helix\n#+END_SRC\n",
    )
    .expect("write");
    let c = Config::from_path(&path).expect("load");
    assert_eq!(c.input_mode, InputMode::Helix);
}

// --- LLM BYOK config ---------------------------------------------------

#[test]
fn llm_keys_parse() {
    let cfg = Config::from_kv_block(
        "llm_provider = anthropic\nllm_model = claude-sonnet-4-6\nllm_key_env = ANTHROPIC_API_KEY\n",
    )
    .expect("parse");
    assert_eq!(cfg.llm_provider.as_deref(), Some("anthropic"));
    assert_eq!(cfg.llm_model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(cfg.llm_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
}

#[test]
fn llm_keys_default_to_none() {
    let cfg = Config::from_kv_block("theme = dark\n").expect("parse");
    assert_eq!(cfg.llm_provider, None);
    assert_eq!(cfg.llm_model, None);
    assert_eq!(cfg.llm_key_env, None);
}
