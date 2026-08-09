//! "Add UI to start MCP server."
//!
//! There is nothing to start. `closure mcp` speaks JSON-RPC on stdin
//! and stdout, which is what MCP clients expect: the *client* spawns
//! the server, one process per session, and a button in closure that
//! launched a second one would be a button that does nothing a client
//! can reach.
//!
//! What you actually need to turn it on is the line to paste into the
//! client's config, with this vault's path already in it — and then,
//! once it is running, some way to see that it is. So: a pane that
//! knows every bridge closure serves, gives you the command for each,
//! and shows the servers closure is itself a client of, with the ones
//! that failed to start named.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn shell_with(config: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), "* A note\n").unwrap();
    if !config.is_empty() {
        std::fs::write(
            dir.path().join("config.org"),
            format!("#+begin_src closure-config\n{config}\n#+end_src\n"),
        )
        .unwrap();
    }
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

#[test]
fn every_bridge_closure_serves_is_listed_with_its_command() {
    let (dir, shell) = shell_with("");
    let app = ModalApp::new(InputMode::Doom);
    let rows = app.bridge_rows(&shell);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    for expected in ["mcp", "lsp", "acp", "a2a"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    let mcp = rows.iter().find(|r| r.name == "mcp").unwrap();
    assert!(
        mcp.command.contains("closure mcp")
            && mcp.command.contains(&dir.path().display().to_string()),
        "the command is not one you could paste: {}",
        mcp.command
    );
}

#[test]
fn a_bridge_says_what_it_is_for() {
    let (_d, shell) = shell_with("");
    let app = ModalApp::new(InputMode::Doom);
    let rows = app.bridge_rows(&shell);
    let mcp = rows.iter().find(|r| r.name == "mcp").unwrap();
    assert!(
        !mcp.detail.is_empty(),
        "a row with a command and no sentence is a row you have to look up"
    );
}

#[test]
fn the_servers_closure_consumes_are_listed_too() {
    // The other direction, which exists as of the MCP client work: a
    // pane about MCP that showed only the serving half would be half
    // the picture.
    let (_d, shell) = shell_with("mcp files = mcp-server-filesystem /tmp");
    let app = ModalApp::new(InputMode::Doom);
    let rows = app.bridge_rows(&shell);
    let client = rows
        .iter()
        .find(|r| r.name == "files")
        .expect("the configured server is not listed");
    assert!(
        !client.serving,
        "a server we are a client of is not one we serve"
    );
    assert!(client.command.contains("mcp-server-filesystem"));
}

#[test]
fn a_vault_that_configures_nothing_still_lists_what_it_serves() {
    let (_d, shell) = shell_with("");
    let app = ModalApp::new(InputMode::Doom);
    let rows = app.bridge_rows(&shell);
    assert!(rows.iter().all(|r| r.serving), "{rows:?}");
    assert!(rows.len() >= 4);
}
