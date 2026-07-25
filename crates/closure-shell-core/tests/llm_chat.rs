//! The LLM chat surface.
//!
//! `closure-llm` has had a provider set and a tool loop for a while,
//! and both were reachable only from `closure ask` / `closure chat`.
//! The GUI had the render-permission toggle and nothing behind it —
//! the permission guarded a session that did not exist.
//!
//! Two things matter more than the chat box itself:
//!
//!  * the configuration has to be *legible*: which provider, which
//!    model, where the key comes from, and whether it is actually
//!    resolvable right now. "It didn't answer" is a terrible error
//!    message when the real problem is an unset environment variable;
//!  * `view-render` has to honour the live grant. A model that can
//!    read the screen when the user has revoked that is a broken
//!    promise, not a feature.
//!
//! The provider call itself is I/O and belongs to the shell; what is
//! here is everything that decides *whether* and *what*.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn vault_with_config(config: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    if !config.is_empty() {
        fs::write(
            dir.path().join("config.org"),
            format!("#+BEGIN_SRC closure-config\n{config}\n#+END_SRC\n"),
        )
        .expect("config");
    }
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQCHAT000000000000001\n:END:\nbody text\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

// === the surface ===

#[test]
fn the_llm_command_opens_the_chat() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "llm");
    assert_eq!(app.surface(), ModalSurface::Llm);
    assert!(app.chat_turns().is_empty(), "an empty transcript");
}

#[test]
fn typing_and_enter_records_the_question() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "llm");
    for c in "what is here".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    assert_eq!(app.chat_buffer(), "what is here");
    app.on_key(&mut shell, "enter", false, false, None);
    let turns = app.chat_turns();
    assert_eq!(turns.len(), 1, "{turns:?}");
    assert!(turns[0].from_user, "the first turn is the question");
    assert_eq!(turns[0].text, "what is here");
    assert_eq!(app.chat_buffer(), "", "the field clears");
}

#[test]
fn an_empty_question_is_not_sent() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "llm");
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.chat_turns().is_empty());
}

#[test]
fn an_answer_is_appended_as_the_other_side() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "llm");
    app.chat_ask("q".to_owned());
    app.chat_answer("a".to_owned());
    let turns = app.chat_turns();
    assert_eq!(turns.len(), 2);
    assert!(turns[0].from_user);
    assert!(!turns[1].from_user, "the answer is not from the user");
}

#[test]
fn escape_leaves_the_chat() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "llm");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === what the surface says about the configuration ===

#[test]
fn an_unconfigured_vault_says_exactly_what_to_add() {
    // The worst version of this feature is a chat box that silently
    // does nothing because config.org has no provider.
    let (_d, shell, app) = vault_with_config("");
    let status = app.llm_config_status(&shell);
    assert!(!status.ready, "nothing configured");
    assert!(
        status.detail.contains("llm_provider"),
        "name the key: {}",
        status.detail
    );
    assert!(
        status.detail.contains("config.org"),
        "and the file: {}",
        status.detail
    );
}

#[test]
fn a_configured_provider_is_reported_with_its_model() {
    let (_d, shell, app) = vault_with_config(
        "llm_provider = ollama\nllm_model = llama3\nllm_endpoint = http://localhost:11434/v1/chat/completions",
    );
    let status = app.llm_config_status(&shell);
    assert!(status.ready, "keyless local provider is ready: {status:?}");
    assert_eq!(status.provider.as_deref(), Some("ollama"));
    assert_eq!(status.model.as_deref(), Some("llama3"));
    assert!(status.detail.contains("llama3"), "{}", status.detail);
}

#[test]
fn a_byok_provider_with_an_unset_key_is_not_ready_and_says_why() {
    // The key lives in the environment, so "configured" and
    // "resolvable right now" are different questions, and the second
    // one is the one that fails at 2am.
    let (_d, shell, app) = vault_with_config(
        "llm_provider = anthropic\nllm_model = claude\nllm_key_env = CLOSURE_TEST_UNSET_KEY_VAR",
    );
    let status = app.llm_config_status(&shell);
    assert!(!status.ready, "the variable is not set");
    assert!(
        status.detail.contains("CLOSURE_TEST_UNSET_KEY_VAR"),
        "name the variable so it can be exported: {}",
        status.detail
    );
}

#[test]
fn the_status_never_contains_the_key_itself() {
    // Whatever goes on screen can end up in a screenshot. `HOME` is
    // borrowed here purely as a variable that is reliably set and
    // whose value is easy to recognise — no environment is mutated,
    // because this crate forbids unsafe and a test that reaches for
    // `set_var` is a test that can race another one.
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let (_d, shell, app) =
        vault_with_config("llm_provider = anthropic\nllm_model = claude\nllm_key_env = HOME");
    let status = app.llm_config_status(&shell);
    assert!(status.ready, "the variable is set");
    assert!(
        !status.detail.contains(&home),
        "the value must never reach the screen: {}",
        status.detail
    );
    assert!(
        status.detail.contains("HOME"),
        "only the variable's name: {}",
        status.detail
    );
}

// === the render permission ===

#[test]
fn view_render_is_refused_until_it_is_granted() {
    let (_d, mut shell, app) = vault_with_config("");
    let out = app.llm_tool(&mut shell, "view-render");
    assert!(
        out.contains("not allowed") || out.contains("revoked"),
        "refused, and says so: {out}"
    );
}

#[test]
fn view_render_returns_the_live_view_once_granted() {
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "toggle-llm-render");
    let out = app.llm_tool(&mut shell, "view-render");
    assert!(
        out.contains("Note"),
        "the model reads what is actually on screen: {out}"
    );
}

#[test]
fn revoking_render_access_takes_effect_immediately() {
    // The whole point of a live toggle.
    let (_d, mut shell, mut app) = vault_with_config("");
    app.run(&mut shell, "toggle-llm-render");
    assert!(app.llm_tool(&mut shell, "view-render").contains("Note"));
    app.run(&mut shell, "toggle-llm-render");
    assert!(!app.llm_tool(&mut shell, "view-render").contains("Note"));
}

#[test]
fn the_ordinary_vault_tools_do_not_need_the_render_grant() {
    // Render access is a separate, louder permission; reading the
    // vault is what the assistant is for.
    let (_d, mut shell, app) = vault_with_config("");
    let out = app.llm_tool(&mut shell, "view-state");
    assert!(!out.contains("not allowed"), "{out}");
    assert!(out.contains("headlines"), "{out}");
}
