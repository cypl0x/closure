//! L4: config-driven provider selection (pure, hermetic). The provider
//! NAME picks a kind; the API key is read from a named env var, never
//! from config/org. No network.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{provider_kind, resolve_key, ProviderKind};

#[test]
fn name_selects_provider_kind() {
    assert_eq!(provider_kind(Some("echo")), ProviderKind::Echo);
    assert_eq!(provider_kind(None), ProviderKind::Echo, "unset -> echo (no key)");
    assert_eq!(provider_kind(Some("ollama")), ProviderKind::Ollama);
    assert_eq!(provider_kind(Some("openai")), ProviderKind::OpenAi);
    assert_eq!(provider_kind(Some("anthropic")), ProviderKind::Anthropic);
}

#[test]
fn key_is_read_from_the_named_env_var() {
    // Reads the env var by name (no unsafe set_var): PATH is set in the
    // shell; a nonsense var is not — the key never comes from config.
    assert!(resolve_key("PATH").is_some(), "reads a set env var by name");
    assert_eq!(
        resolve_key("CLOSURE_DEFINITELY_UNSET_KEY_VAR_L4"),
        None,
        "unset env var -> None (never from config)"
    );
}
