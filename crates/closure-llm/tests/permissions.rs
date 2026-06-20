//! V3b: live, configurable LLM render permission. Render access is
//! opt-in (off by default), grantable via config, and revocable/grantable
//! at runtime by the toggle command.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{LlmPermissions, RENDER_TOOL};

#[test]
fn render_is_off_by_default_other_tools_unrestricted() {
    let p = LlmPermissions::from_config(vec![]);
    assert!(!p.allows(RENDER_TOOL), "render is opt-in");
    assert!(
        p.allows("read"),
        "other tools unrestricted when no allowlist"
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
