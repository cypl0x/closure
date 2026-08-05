//! "[#A] Finish and wire the LLM layer completely."
//!
//! No body on the item, so this is what "completely" turned out to
//! mean when I went looking: two places where the layer is wired to
//! something other than what a reader of `config.org` would expect.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_llm::{LlmPermissions, ProviderKind, provider_kind};

#[test]
fn no_llm_tools_line_still_lets_the_model_read() {
    // `llm_tools` bounds a model you chose to invoke, so an absent
    // line letting it read the vault you asked it about is reasonable
    // — and an assistant that cannot read reads as broken, not as
    // safe.
    let none = LlmPermissions::from_config(Vec::new());
    for tool in ["read", "search", "list-files", "view-state"] {
        assert!(none.allows(tool), "`{tool}` only reads");
    }
}

#[test]
fn no_llm_tools_line_does_not_let_it_write() {
    // The model reads your vault, and a note in it can tell the model
    // what to do — the `eval_trust` threat one step removed: content
    // someone sent you, deciding to rename your headlines. Reading
    // costs nothing; writing has to be asked for.
    let none = LlmPermissions::from_config(Vec::new());
    for tool in closure_llm::WRITING_TOOLS {
        assert!(
            !none.allows(tool),
            "an absent `llm_tools` let the model run `{tool}`, which writes"
        );
    }
}

#[test]
fn naming_a_write_tool_grants_it() {
    let asked = LlmPermissions::from_config(vec!["read".to_owned(), "capture".to_owned()]);
    assert!(asked.allows("capture"), "it was asked for by name");
    assert!(!asked.allows("rename"), "and this one was not");
}

#[test]
fn what_the_line_lists_is_what_is_allowed() {
    let some = LlmPermissions::from_config(vec!["read".to_owned(), "search".to_owned()]);
    assert!(some.allows("read"));
    assert!(some.allows("search"));
    assert!(!some.allows("capture"), "capture writes to the vault");
    assert!(!some.allows("set-property"));
}

#[test]
fn a_misspelt_provider_is_not_silently_anthropic() {
    // `llm_provider = antropic` became Anthropic, then failed on a
    // missing key with a message about the key — so the one thing the
    // user got told was the one thing that was not wrong.
    assert_eq!(provider_kind(Some("anthropic")), ProviderKind::Anthropic);
    assert_eq!(provider_kind(Some("openai")), ProviderKind::OpenAi);
    assert_eq!(provider_kind(Some("ollama")), ProviderKind::Ollama);
    assert_eq!(provider_kind(None), ProviderKind::Echo);
    assert_eq!(provider_kind(Some("echo")), ProviderKind::Echo);
    assert_eq!(
        provider_kind(Some("antropic")),
        ProviderKind::Unknown,
        "a typo is a typo, not a provider"
    );
}

#[test]
fn the_known_names_are_listed_for_a_message_to_use() {
    // A refusal that says "unknown provider" and stops is the same
    // shape as the eval refusal that named a concept and not a fix.
    let names = closure_llm::known_providers();
    for want in ["anthropic", "openai", "ollama", "echo"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    for name in names {
        assert_ne!(
            provider_kind(Some(name)),
            ProviderKind::Unknown,
            "`{name}` is advertised and not understood"
        );
    }
}

#[test]
fn an_unknown_provider_still_answers_locally() {
    // It must not be a crash or a hang: the shell shows the assistant
    // screen, which is where the name gets reported, and something has
    // to be there to talk to in the meantime.
    let p = closure_llm::build_provider(ProviderKind::Unknown, "m", "http://localhost", "");
    assert!(p.complete("hello").is_ok());
}

#[test]
fn openai_compatible_speaks_the_openai_wire() {
    // The name says which wire format it is. It mapped to Anthropic
    // through a catch-all arm, so a config the loader validated —
    // `openai-compatible` + `llm_endpoint`, which the loader *insists*
    // on — sent Anthropic-shaped JSON to an OpenAI endpoint. Wired at
    // both ends and wrong in the middle.
    assert_eq!(
        provider_kind(Some("openai-compatible")),
        ProviderKind::OpenAi
    );
}

#[test]
fn the_config_loader_and_this_crate_know_the_same_providers() {
    // `closure-config` sits below this crate and spells the list
    // itself rather than inverting the dependency to validate a
    // string. Two copies of one fact is a thing to pin, not a thing to
    // hope about.
    for name in closure_llm::known_providers() {
        let src = format!(
            "#+BEGIN_SRC closure-config\nllm_provider = {name}\nllm_key_env = K\n\
             llm_endpoint = http://localhost:8080/v1/chat/completions\n#+END_SRC\n"
        );
        assert!(
            closure_config::Config::from_org_source(&src).is_ok(),
            "this crate offers `{name}` and the loader refuses it"
        );
    }
}
