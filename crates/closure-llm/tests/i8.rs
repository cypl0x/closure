//! L3: the I8 boundary end-to-end. A scripted "model" emits CALL then
//! DONE; `tool_loop` routes each CALL through the command registry
//! (`Vault::run_tool`) — the ONLY way the model can affect state.
//! Proves the document changes solely via that path. Hermetic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;

use closure_llm::{tool_loop, LlmError, Provider};
use closure_store::Vault;
use tempfile::TempDir;

/// A provider that replays a fixed script of replies, one per turn.
struct ScriptedProvider {
    replies: RefCell<std::collections::VecDeque<String>>,
}
impl ScriptedProvider {
    fn new(lines: &[&str]) -> Self {
        Self {
            replies: RefCell::new(lines.iter().map(|s| (*s).to_owned()).collect()),
        }
    }
}
impl Provider for ScriptedProvider {
    fn complete(&self, _prompt: &str) -> Result<String, LlmError> {
        self.replies
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| LlmError::Provider("script exhausted".into()))
    }
}

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn model_mutates_vault_only_through_the_registry() {
    let (_d, mut v) = vault();
    assert!(v.find_by_title("Buy milk").is_none(), "not captured yet");

    let provider = ScriptedProvider::new(&["CALL capture Buy milk", "DONE captured it"]);
    // The execute closure is the ONLY handle the loop has to state — it
    // routes the command line through Vault::run_tool (the registry, I8).
    let answer = tool_loop(&provider, |cmd| v.run_tool(cmd), "capture a todo", 5).expect("loop");

    assert_eq!(answer, "captured it", "loop returns the model's DONE answer");
    assert!(
        v.find_by_title("Buy milk").is_some(),
        "the CALL ran through the registry and changed the vault (I8)"
    );
}

#[test]
fn bare_answer_without_call_changes_nothing() {
    let (_d, mut v) = vault();
    let before = v.headline_count();
    let provider = ScriptedProvider::new(&["just an answer, no command"]);
    let answer = tool_loop(&provider, |cmd| v.run_tool(cmd), "say hi", 5).expect("loop");
    assert_eq!(answer, "just an answer, no command");
    assert_eq!(v.headline_count(), before, "no CALL => no state change");
}
