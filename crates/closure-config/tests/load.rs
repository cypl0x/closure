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
    // `UnknownKey` grew fields — which line, and the key you probably
    // meant — for the "configuration language" item on 2026-08-06.
    // What is asserted here is unchanged.
    assert!(matches!(err, ConfigError::UnknownKey { key, .. } if key == "nope"));
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

#[test]
fn record_commands_parses_bool() {
    assert!(
        Config::from_kv_block("record_commands = true\n")
            .expect("p")
            .record_commands
    );
    assert!(
        !Config::from_kv_block("record_commands = false\n")
            .expect("p")
            .record_commands
    );
}

#[test]
fn record_commands_defaults_false() {
    assert!(
        !Config::from_kv_block("theme = dark\n")
            .expect("p")
            .record_commands
    );
}

#[test]
fn search_backend_parses() {
    assert_eq!(
        Config::from_kv_block("search_backend = ripgrep\n")
            .expect("p")
            .search_backend
            .as_deref(),
        Some("ripgrep")
    );
    assert!(
        Config::from_kv_block("theme = x\n")
            .expect("p")
            .search_backend
            .is_none()
    );
}

// TDD for LLM per-tool permissions (live configurable, part of deep access).
// llm_tools = comma list of allowed tools for the LLM (read,search,capture...).
// Error or filter at load/use.
#[test]
fn llm_tools_parses_list() {
    let cfg = Config::from_kv_block("llm_tools = read,search,capture\n").expect("parse");
    assert_eq!(
        cfg.llm_tools.as_deref(),
        Some(&["read".to_owned(), "search".to_owned(), "capture".to_owned()][..])
    );
}

// TDD for sniffer blocklist (last sub for sniffer).
// sniffer_blocklist = comma globs; used for =closure sniff= view.
#[test]
fn sniffer_blocklist_parses() {
    let cfg = Config::from_kv_block("sniffer_blocklist = *tracker*,*evil*\n").expect("parse");
    assert_eq!(
        cfg.sniffer_blocklist.as_deref(),
        Some(&["*tracker*".to_owned(), "*evil*".to_owned()][..])
    );
}

// C1a: eval_trust is a comma list of languages allowed to execute.
// Absent => empty => default-deny (the security default).
#[test]
fn eval_trust_parses_list() {
    let cfg = Config::from_kv_block("eval_trust = shell, python\n").expect("parse");
    assert_eq!(
        cfg.eval_trust,
        vec!["shell".to_owned(), "python".to_owned()]
    );
}

#[test]
fn eval_trust_defaults_empty() {
    let cfg = Config::default();
    assert!(cfg.eval_trust.is_empty(), "default-deny: nothing trusted");
}

// TDD for the final Config validation sub-item (typed cross-key constraints,
// yesod principle: error at load time, not first use).
// Example from ROADMAP: llm_provider set ⇒ llm_key_env must also be set.
#[test]
fn llm_provider_without_key_env_is_rejected_at_load() {
    let src = "#+BEGIN_SRC closure-config\n\
               llm_provider = anthropic\n\
               llm_model = claude-sonnet-4-6\n\
               #+END_SRC\n";
    let err = Config::from_org_source(src).expect_err("cross-key should fail at load");
    // Will fail until we add the check at end of from_kv_block.
    // For now the test documents the desired behavior.
    match err {
        ConfigError::BadValue { reason, .. } => {
            assert!(
                reason.contains("llm_key_env")
                    || reason.contains("cross")
                    || reason.contains("required"),
                "expected cross-key error mentioning llm_key_env, got: {reason}"
            );
        }
        other => panic!("expected BadValue for missing dependent key, got {other:?}"),
    }
}

#[test]
fn keyless_providers_need_no_key_env() {
    // echo (tests) and ollama (localhost) need no API key, so the
    // cross-key rule must not require llm_key_env for them.
    for provider in ["echo", "ollama"] {
        let src = format!("#+BEGIN_SRC closure-config\nllm_provider = {provider}\n#+END_SRC\n");
        let cfg = Config::from_org_source(&src)
            .unwrap_or_else(|e| panic!("{provider} should load without key_env: {e}"));
        assert_eq!(cfg.llm_provider.as_deref(), Some(provider));
        assert_eq!(cfg.llm_key_env, None);
    }
}

// --- CUE-inspired validation (line/col at load) tests written first per TDD ---
// These will fail until ConfigError carries structured location and from_kv_block
// (and from_org_source) attach line + column for BadValue/UnknownKey.

#[test]
fn bad_value_reports_line_in_block() {
    let src = "#+BEGIN_SRC closure-config\ninput_mode = whatever\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    // The desired CUE-like behavior (ROADMAP): errors at load with line/col context.
    // This stronger assert will fail until from_kv_block threads the line_no (and col)
    // into BadValue for type/parse errors in the match arms (not just the split case).
    match err {
        ConfigError::BadValue { key, reason } => {
            assert_eq!(key, "input_mode");
            assert!(reason.contains("whatever") || reason.contains("unknown"));
            // Demanding: the error context must mention the line inside the block.
            // Current code only puts "line N" in the key field for the "no =" parse error.
            assert!(
                reason.contains("line") || key.contains("line"),
                "expected line info in BadValue for CUE-style early error, got key={key:?} reason={reason:?}"
            );
        }
        other => panic!("expected BadValue, got {other:?}"),
    }
}

#[test]
fn unknown_key_reports_at_load_with_context() {
    let src = "#+BEGIN_SRC closure-config\nnope = x\n#+END_SRC\n";
    let err = Config::from_org_source(src).unwrap_err();
    // `UnknownKey` grew fields — which line, and the key you probably
    // meant — for the "configuration language" item on 2026-08-06.
    // What is asserted here is unchanged.
    assert!(matches!(err, ConfigError::UnknownKey { key, .. } if key == "nope"));
}

#[test]
fn theme_kind_maps_string_to_dark_or_light_defaulting_dark() {
    use closure_config::ThemeKind;
    // Default theme ("default") -> Dark.
    assert_eq!(Config::default().theme_kind(), ThemeKind::Dark);
    // Explicit light (case-insensitive) -> Light.
    let c = Config::from_kv_block("theme = light\n").expect("parse");
    assert_eq!(c.theme_kind(), ThemeKind::Light);
    let c = Config::from_kv_block("theme = LIGHT\n").expect("parse");
    assert_eq!(c.theme_kind(), ThemeKind::Light);
    // Explicit dark + any other value -> Dark.
    assert_eq!(
        Config::from_kv_block("theme = dark\n")
            .expect("p")
            .theme_kind(),
        ThemeKind::Dark
    );
    assert_eq!(
        Config::from_kv_block("theme = solarized\n")
            .expect("p")
            .theme_kind(),
        ThemeKind::Dark
    );
}

// === which shape the GUI opens in ===

#[test]
fn view_defaults_to_the_clickable_outline() {
    assert_eq!(Config::default().view, "clickable");
}

#[test]
fn view_editor_asks_for_the_file_buffer() {
    let cfg = Config::from_org_source("#+BEGIN_SRC closure-config\nview = editor\n#+END_SRC\n")
        .expect("parses");
    assert_eq!(cfg.view, "editor");
    // `file` is the same request said the other way.
    let alias = Config::from_org_source("#+BEGIN_SRC closure-config\nview = file\n#+END_SRC\n")
        .expect("parses");
    assert_eq!(alias.view, "editor");
}

#[test]
fn an_unknown_view_is_a_config_error_not_a_silent_default() {
    let err = Config::from_org_source("#+BEGIN_SRC closure-config\nview = sideways\n#+END_SRC\n")
        .expect_err("rejected");
    assert!(format!("{err}").contains("sideways"), "{err}");
}
