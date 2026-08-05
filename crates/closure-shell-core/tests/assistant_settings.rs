//! "built setup UI for the Assistant — it should get persisted in the
//! config.org file. It just UI for all of the config options for the
//! file."
//!
//! The screen is a list of the assistant's settings with what they are
//! set to now, editable, saved back into `config.org` as a splice so
//! the user's comments and their other keys survive.
//!
//! Two things make this more than a form. The key itself is never a
//! setting: `llm_key_env` names an *environment variable*, so a
//! screenshot of this screen leaks nothing, and the screen has to say
//! whether that variable is actually set — "you configured a provider
//! and the key is missing" is the failure people hit and it is
//! invisible from the config alone. And the loader refuses a config
//! that names a provider without a key variable, so the screen cannot
//! save those two independently.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::Config;
use closure_shell_core::{SettingField, assistant_settings_with};

/// The environment, as a lookup the test controls. The shipped
/// `assistant_settings` reads the real one; passing it in is what lets
/// "the key variable is missing" be tested without a process-wide
/// mutation the workspace forbids anyway.
fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name: &str| {
        owned
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

fn assistant_settings(cfg: &Config) -> Vec<SettingField> {
    assistant_settings_with(cfg, &env(&[]))
}

fn cfg_of(src: &str) -> Config {
    Config::from_org_source(src).expect("parses")
}

const CONFIGURED: &str = "\
#+BEGIN_SRC closure-config
llm_provider = ollama
llm_model = llama3
llm_key_env = OLLAMA_KEY
llm_endpoint = http://localhost:11434
#+END_SRC
";

#[test]
fn every_assistant_option_is_on_the_screen() {
    // "It just UI for all of the config options for the file" — for
    // the assistant, that is these five. A setting the screen omits is
    // one the user has to go and hand-edit anyway, which defeats it.
    let fields = assistant_settings(&cfg_of(CONFIGURED));
    let keys: Vec<&str> = fields.iter().map(|f| f.key).collect();
    for expected in [
        "llm_provider",
        "llm_model",
        "llm_key_env",
        "llm_endpoint",
        "llm_tools",
    ] {
        assert!(keys.contains(&expected), "{expected} is missing: {keys:?}");
    }
}

#[test]
fn each_field_shows_what_it_is_set_to() {
    let fields = assistant_settings(&cfg_of(CONFIGURED));
    let find = |k: &str| {
        fields
            .iter()
            .find(|f| f.key == k)
            .unwrap_or_else(|| panic!("{k}"))
    };
    assert_eq!(find("llm_provider").value, "ollama");
    assert_eq!(find("llm_model").value, "llama3");
    assert_eq!(find("llm_endpoint").value, "http://localhost:11434");
}

#[test]
fn an_unset_field_reads_as_unset_rather_than_blank() {
    // A blank row is ambiguous between "empty string" and "never
    // configured", and the two behave differently.
    let fields = assistant_settings(&Config::default());
    let provider = fields.iter().find(|f| f.key == "llm_provider").unwrap();
    assert!(provider.value.is_empty());
    assert!(!provider.placeholder.is_empty(), "nothing to show instead");
}

#[test]
fn the_provider_field_offers_the_providers_that_exist() {
    // Typing a provider name that is not one of the four silently
    // falls back to Anthropic, which is the least discoverable failure
    // in the whole config. The screen offers the list instead.
    let fields = assistant_settings(&cfg_of(CONFIGURED));
    let provider = fields.iter().find(|f| f.key == "llm_provider").unwrap();
    for name in ["echo", "ollama", "openai", "anthropic"] {
        assert!(
            provider.choices.iter().any(|c| c == name),
            "{name} is not offered: {:?}",
            provider.choices
        );
    }
}

#[test]
fn the_key_variable_is_named_but_its_value_is_never_read_onto_the_screen() {
    // The whole point of `llm_key_env`: the secret stays in the
    // environment. A settings screen that helpfully showed the key
    // would put it in every screenshot of this window.
    let cfg = cfg_of(
        "#+BEGIN_SRC closure-config\n\
         llm_provider = openai\n\
         llm_key_env = CLOSURE_TEST_FAKE_KEY\n\
         #+END_SRC\n",
    );
    let fields =
        assistant_settings_with(&cfg, &env(&[("CLOSURE_TEST_FAKE_KEY", "sk-secret-value")]));
    for field in &fields {
        assert!(
            !field.value.contains("sk-secret-value"),
            "the key leaked onto `{}`",
            field.key
        );
        assert!(!field.detail.contains("sk-secret-value"));
    }
}

#[test]
fn the_screen_says_whether_the_key_variable_is_actually_set() {
    // The failure people actually hit: provider configured, variable
    // never exported, and nothing anywhere says so.
    let present = cfg_of(
        "#+BEGIN_SRC closure-config\nllm_provider = openai\nllm_key_env = CLOSURE_TEST_PRESENT\n#+END_SRC\n",
    );
    let absent = cfg_of(
        "#+BEGIN_SRC closure-config\nllm_provider = openai\nllm_key_env = CLOSURE_TEST_ABSENT\n#+END_SRC\n",
    );
    let detail = |c: &Config| {
        assistant_settings_with(c, &env(&[("CLOSURE_TEST_PRESENT", "x")]))
            .into_iter()
            .find(|f| f.key == "llm_key_env")
            .unwrap()
            .detail
    };
    assert!(detail(&present).contains("set"), "{}", detail(&present));
    assert!(
        detail(&absent).contains("not set"),
        "the missing key is not reported: {}",
        detail(&absent)
    );
}

#[test]
fn echo_needs_no_key_and_the_screen_does_not_nag() {
    // The default provider never leaves the process, so demanding a
    // key variable for it would be a warning that is simply wrong.
    let cfg =
        cfg_of("#+BEGIN_SRC closure-config\nllm_provider = echo\nllm_key_env = X\n#+END_SRC\n");
    let key = assistant_settings(&cfg)
        .into_iter()
        .find(|f| f.key == "llm_key_env")
        .unwrap();
    assert!(!key.detail.contains("not set"), "{}", key.detail);
}

#[test]
fn the_endpoint_field_says_where_requests_will_really_go() {
    // The defect this whole area came from: the config named an
    // endpoint and the code dialled somewhere else. An empty endpoint
    // is not "nowhere", it is the provider's default, and the screen
    // should say which.
    let fields = assistant_settings(&cfg_of(
        "#+BEGIN_SRC closure-config\nllm_provider = openai\nllm_key_env = K\n#+END_SRC\n",
    ));
    let endpoint = fields.iter().find(|f| f.key == "llm_endpoint").unwrap();
    assert!(
        endpoint.detail.contains("api.openai.com"),
        "the default is not named: {}",
        endpoint.detail
    );
}

#[test]
fn a_provider_name_this_build_does_not_know_is_a_config_error() {
    // "[#A] Finish and wire the LLM layer completely" — `llm_provider
    // = antropic` used to fall through to Anthropic and then fail on a
    // missing key, so the only thing reported was the only thing that
    // was not wrong. It is caught at load now, where the other typed
    // cross-key constraints are (I9: error at load, not at first use).
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nllm_provider = antropic\nllm_key_env = K\n#+END_SRC\n",
    )
    .expect("config");
    let err = closure_config::Config::from_path(&dir.path().join("config.org"))
        .expect_err("a typo is not a provider");
    let text = format!("{err}");
    assert!(
        text.contains("antropic"),
        "it does not name the typo: {text}"
    );
    assert!(
        text.contains("anthropic") && text.contains("openai") && text.contains("ollama"),
        "and it does not offer the names it knows: {text}"
    );
}

#[test]
fn every_name_it_offers_actually_loads() {
    // The other half: a list in an error message that includes a name
    // the loader rejects is worse than no list.
    for name in ["anthropic", "openai", "openai-compatible", "ollama", "echo"] {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(
            dir.path().join("config.org"),
            format!(
                "#+BEGIN_SRC closure-config\nllm_provider = {name}\nllm_key_env = K\n\
                 llm_endpoint = http://localhost:8080/v1/chat/completions\n#+END_SRC\n"
            ),
        )
        .expect("config");
        assert!(
            closure_config::Config::from_path(&dir.path().join("config.org")).is_ok(),
            "`{name}` is offered and rejected"
        );
    }
}
