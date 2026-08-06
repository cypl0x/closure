//! V3b: live, configurable LLM render permission. Render access is
//! opt-in (off by default), grantable via config, and revocable/grantable
//! at runtime by the toggle command.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{LlmPermissions, RENDER_TOOL};

#[test]
fn render_is_off_by_default_and_so_is_writing() {
    // Narrowed 2026-08-05, working "[#A] Finish and wire the LLM layer
    // completely": with no allowlist the *reading* tools stay
    // unrestricted, because an assistant that cannot read the vault
    // you asked it about reads as broken rather than as safe. The
    // writing ones do not, because the model reads your vault and a
    // note in it can tell the model what to do.
    let p = LlmPermissions::from_config(vec![]);
    assert!(!p.allows(RENDER_TOOL), "render is opt-in");
    assert!(
        p.allows("read"),
        "reading tools unrestricted when no allowlist"
    );
    assert!(
        !p.allows("rename"),
        "an absent allowlist let the model rename headlines"
    );
}

#[test]
fn config_can_grant_render() {
    let p = LlmPermissions::from_config(vec!["read".to_owned(), "view-render".to_owned()]);
    assert!(p.allows(RENDER_TOOL), "config granted render");
    assert!(p.allows("read"));
    assert!(
        !p.allows("capture"),
        "allowlist still restricts other tools"
    );
}

#[test]
fn live_toggle_grants_then_revokes() {
    let mut p = LlmPermissions::from_config(vec![]);
    assert!(!p.allows(RENDER_TOOL));
    assert!(p.toggle_render(), "toggle grants");
    assert!(p.allows(RENDER_TOOL));
    assert!(!p.toggle_render(), "toggle revokes");
    assert!(!p.allows(RENDER_TOOL), "render revoked at runtime");
}

#[test]
fn explicit_grant_revoke() {
    let mut p = LlmPermissions::from_config(vec!["view-render".to_owned()]);
    assert!(p.allows(RENDER_TOOL));
    p.revoke_render();
    assert!(
        !p.allows(RENDER_TOOL),
        "revoked live even though config granted"
    );
    p.grant_render();
    assert!(p.allows(RENDER_TOOL));
}

/// "Consume MCP Servers (?)" — a tool from someone else's server is
/// asked for by name, the way a write is.
///
/// The reasoning is the one already written above for writes, one step
/// further out: `llm_tools` bounds a model you chose to invoke, and an
/// absent line letting it read *your vault* is reasonable. A tool from
/// a server you configured is not your vault — it is a filesystem, a
/// browser, an issue tracker, and the model learned it exists from a
/// menu that a note in your vault can talk it into using.
#[test]
fn a_tool_from_another_server_is_named_or_it_does_not_run() {
    let open = closure_llm::LlmPermissions::from_config(Vec::new());
    assert!(
        open.allows("search"),
        "reading the vault still needs no line"
    );
    assert!(
        !open.allows("files/read_file"),
        "an unnamed external tool ran on an empty llm_tools"
    );

    let named = closure_llm::LlmPermissions::from_config(vec!["files/read_file".to_owned()]);
    assert!(named.allows("files/read_file"));
    assert!(
        !named.allows("issues/close"),
        "naming one external tool granted another"
    );

    // A whole server at once, which is the line most people will write.
    let server = closure_llm::LlmPermissions::from_config(vec!["files/".to_owned()]);
    assert!(server.allows("files/read_file"));
    assert!(!server.allows("issues/close"));
}
