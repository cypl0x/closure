//! ACP agent card served over MCP JSON subset + in-process handshake.
//!
//! TDD: these tests are written first. They must fail (compile or runtime)
//! until the ACP crate implements agent card (name + tools + schemas) and
//! a handshake between two in-process agents (I8: via registry/vault only).
//!
//! Research (done before writing): acp currently only has text resolve/run
//! mirroring early mcp; mcp has handle_message + TOOLS + json helpers for
//! initialize/tools/list/tools/call over Vault::run_tool. ACP must serve
//! "agent/card" analog over same JSON subset for agent discovery.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_acp::handle_message;
use closure_core::{Registry, RenameHeadline};
use closure_store::Vault;
use tempfile::TempDir;

fn registry_with_rename() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n:PROPERTIES:\n:ID: 01HQX0000000000000000000\n:END:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn resolve_line_compat_still_works_for_registry() {
    // ACP keeps the text protocol surface (resolve_line) for compat with mcp-style.
    let reg = registry_with_rename();
    // We call through the crate; if ACP re-exports or keeps the fn from before.
    // This will initially fail if we moved things; documents the invariant.
    let out = closure_acp::resolve_line(&reg, "rename-headline target Foo");
    // Outcome is re-exported or defined in acp; assert shape via string or match once defined.
    // For now, existence + basic: we will assert after impl that Found/Unknown preserved.
    assert!(
        format!("{:?}", out).contains("rename-headline") || format!("{:?}", out).contains("Found")
    );
}

#[test]
fn agent_card_json_served_over_mcp_subset() {
    let (_td, mut v) = vault();
    // Per ROADMAP: agent card (name, tools, schemas) served over the MCP JSON subset.
    let req = r#"{"jsonrpc":"2.0","id":42,"method":"agent/card"}"#;
    let resp = handle_message(&mut v, req).expect("agent/card must respond");
    assert!(resp.contains("\"id\":42"), "id echoed: {resp}");
    assert!(resp.contains("\"result\""), "has result: {resp}");
    // Card shape per vision for agent discovery.
    assert!(resp.contains("\"name\""), "card must include agent name");
    assert!(resp.contains("\"tools\""), "card must list tools");
    assert!(resp.contains("inputSchema"), "each tool must carry schema");
    // Concrete name from serverInfo style in mcp.
    assert!(resp.contains("closure"), "name should identify closure");
}

#[test]
fn agent_card_includes_core_tools_with_schemas() {
    let (_td, mut v) = vault();
    let resp =
        handle_message(&mut v, r#"{"jsonrpc":"2.0","id":1,"method":"agent/card"}"#).expect("card");
    for t in [
        "list-files",
        "read",
        "search",
        "capture",
        "rename",
        "set-property",
    ] {
        assert!(resp.contains(t), "card missing tool {t}");
    }
    // At least one schema fragment from the MCP-subset style.
    assert!(
        resp.contains("\"type\":\"object\""),
        "schemas are object typed"
    );
}

#[test]
fn in_process_handshake_between_two_agents() {
    // Handshake test between two in-process agents (no real transport;
    // pure function calls + json over the MCP subset as per ROADMAP).
    // Agent A "discovers" agent B's card and vice-versa; both accept.
    let (_td1, mut agent_b) = vault();
    let (_td2, mut agent_a) = vault();

    // A requests B's card (in-process "send" is just calling handle on peer).
    let card_b = handle_message(
        &mut agent_b,
        r#"{"jsonrpc":"2.0","id":10,"method":"agent/card"}"#,
    )
    .expect("B serves its card to A");
    assert!(card_b.contains("\"name\""));
    assert!(card_b.contains("tools"));

    // B requests A's card.
    let card_a = handle_message(
        &mut agent_a,
        r#"{"jsonrpc":"2.0","id":11,"method":"agent/card"}"#,
    )
    .expect("A serves its card to B");
    assert!(card_a.contains("\"name\""));

    // "Handshake" succeeds if both cards look like closure agents with overlapping tools.
    // We encode the handshake success as: cards contain closure name + common tool.
    // (Real handshake could negotiate version/capabilities; this is the minimal
    // that proves two agents can discover each other via ACP card over the json.)
    assert!(card_a.contains("closure") && card_b.contains("closure"));
    assert!(card_a.contains("list-files") && card_b.contains("list-files"));
    // If we reach here without error, the in-process exchange (handshake) worked.
}

#[test]
fn unknown_acp_method_is_method_not_found() {
    let (_td, mut v) = vault();
    let r = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","id":99,"method":"agent/secret"}"#,
    )
    .expect("error response");
    assert!(
        r.contains("-32601") || r.contains("method not found"),
        "ACP should use same error shape: {r}"
    );
}

#[test]
fn agent_card_notification_gets_no_response() {
    let (_td, mut v) = vault();
    let r = handle_message(
        &mut v,
        r#"{"jsonrpc":"2.0","method":"notifications/agent-ready"}"#,
    );
    assert!(r.is_none());
}
