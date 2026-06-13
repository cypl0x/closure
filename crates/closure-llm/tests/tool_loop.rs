//! Tool-use loop: the model may only act by emitting `CALL <command>`
//! lines that the caller executes (I8 — mutations flow through the
//! command registry), and finishes with `DONE <answer>`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;

use closure_llm::{LlmError, Provider, tool_loop};

/// Scripted provider: pops canned replies in order.
struct Scripted {
    replies: RefCell<Vec<String>>,
    prompts: RefCell<Vec<String>>,
}

impl Scripted {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: RefCell::new(replies.iter().rev().map(|s| (*s).to_owned()).collect()),
            prompts: RefCell::new(Vec::new()),
        }
    }
}

impl Provider for Scripted {
    fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        self.prompts.borrow_mut().push(prompt.to_owned());
        self.replies
            .borrow_mut()
            .pop()
            .ok_or_else(|| LlmError::Provider("script exhausted".to_owned()))
    }
}

#[test]
fn done_reply_returns_answer() {
    let p = Scripted::new(&["DONE forty-two"]);
    let out = tool_loop(&p, |_| String::new(), "answer", 5).expect("loop");
    assert_eq!(out, "forty-two");
}

#[test]
fn call_executes_command_then_done() {
    let p = Scripted::new(&["CALL rename-headline ID New title", "DONE renamed"]);
    let calls = RefCell::new(Vec::new());
    let out = tool_loop(
        &p,
        |cmd| {
            calls.borrow_mut().push(cmd.to_owned());
            "ok".to_owned()
        },
        "rename it",
        5,
    )
    .expect("loop");
    assert_eq!(out, "renamed");
    assert_eq!(
        calls.into_inner(),
        vec!["rename-headline ID New title".to_owned()]
    );
}

#[test]
fn observation_feeds_back_into_next_prompt() {
    let p = Scripted::new(&["CALL list", "DONE done"]);
    let _ = tool_loop(&p, |_| "OBSERVED-VALUE".to_owned(), "look", 5).expect("loop");
    let prompts = p.prompts.into_inner();
    assert_eq!(prompts.len(), 2);
    assert!(
        prompts[1].contains("OBSERVED-VALUE"),
        "second prompt carries the tool result"
    );
}

#[test]
fn task_and_protocol_are_in_the_first_prompt() {
    let p = Scripted::new(&["DONE x"]);
    let _ = tool_loop(&p, |_| String::new(), "MY-TASK", 5).expect("loop");
    let prompts = p.prompts.into_inner();
    assert!(prompts[0].contains("MY-TASK"));
    assert!(prompts[0].contains("CALL"));
    assert!(prompts[0].contains("DONE"));
}

#[test]
fn bare_reply_counts_as_final_answer() {
    let p = Scripted::new(&["just an answer"]);
    let out = tool_loop(&p, |_| String::new(), "q", 5).expect("loop");
    assert_eq!(out, "just an answer");
}

#[test]
fn max_turns_is_enforced() {
    let p = Scripted::new(&["CALL a", "CALL b", "CALL c"]);
    let err = tool_loop(&p, |_| "ok".to_owned(), "spin", 3);
    assert!(matches!(err, Err(LlmError::Provider(_))));
}

// TDD for LLM view-state tool (deep access / "see rendered result" per vision and ROADMAP).
// The LLM can CALL view-state to get a description of the current UI state
// (mode, selection, visible content) so it can 'see' what the user sees.
// Test written first; will fail until the constant and handling exist.
#[test]
fn view_state_tool_is_known_and_observation_fed() {
    // References the (not yet) constant for the tool name.
    let cmd = closure_llm::VIEW_STATE_COMMAND;
    let p = Scripted::new(&[&format!("CALL {cmd}"), "DONE saw it"]);
    let observations = RefCell::new(vec![
        "UI STATE: mode=FileView selected=foo.org visible=[h1 h2]".to_owned(),
    ]);
    let out = tool_loop(
        &p,
        |c| {
            if c == cmd {
                observations.borrow_mut().pop().unwrap_or_default()
            } else {
                "ok".to_owned()
            }
        },
        "describe what you see",
        5,
    )
    .expect("loop");
    assert_eq!(out, "saw it");
    let prompts = p.prompts.into_inner();
    assert!(
        prompts.iter().any(|pr| pr.contains("UI STATE")),
        "state fed back into prompt"
    );
}
