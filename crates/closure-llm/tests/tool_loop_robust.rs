//! The tool loop, made to survive a real model.
//!
//! `tool_loop` matched `CALL ` and `DONE` at the very start of a reply
//! and treated anything else as the final answer. Models do not
//! reliably answer that way: they open with "Sure — ", they wrap a
//! line in backticks, they emit a blank line first. Every one of those
//! ended the loop on turn one and returned the preamble as though it
//! were the result, so the tools were never reached and nothing said
//! why.
//!
//! Being strict about the protocol is right; being strict about
//! *where on the line it starts* was not the same thing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::cell::RefCell;

use closure_llm::{LlmError, Provider, tool_loop};

/// A provider that reads from a script, one reply per turn.
struct Scripted {
    replies: RefCell<Vec<String>>,
}

impl Scripted {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: RefCell::new(replies.iter().rev().map(|s| (*s).to_owned()).collect()),
        }
    }
}

impl Provider for Scripted {
    fn complete(&self, _prompt: &str) -> Result<String, LlmError> {
        Ok(self.replies.borrow_mut().pop().unwrap_or_default())
    }
}

#[test]
fn a_plain_protocol_reply_still_works() {
    // The path that always worked, kept as cover.
    let p = Scripted::new(&["CALL list-files", "DONE two files"]);
    let mut seen = Vec::new();
    let out = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            "a.org b.org".to_owned()
        },
        "how many files",
        4,
    )
    .expect("a result");
    assert_eq!(seen, ["list-files"]);
    assert_eq!(out, "two files");
}

#[test]
fn a_preamble_before_the_call_does_not_end_the_loop() {
    // "Sure, let me look." is the single most common thing a model
    // does, and it used to be returned as the final answer.
    let p = Scripted::new(&["Sure, let me look.\nCALL list-files", "DONE two files"]);
    let mut seen = Vec::new();
    let out = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            "a.org b.org".to_owned()
        },
        "how many files",
        4,
    )
    .expect("a result");
    assert_eq!(seen, ["list-files"], "the tool was never reached");
    assert_eq!(out, "two files");
}

#[test]
fn a_fenced_call_is_understood() {
    // Models fence commands constantly.
    let p = Scripted::new(&["```\nCALL list-files\n```", "DONE done"]);
    let mut seen = Vec::new();
    let _ = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            String::new()
        },
        "t",
        4,
    );
    assert_eq!(seen, ["list-files"]);
}

#[test]
fn a_late_done_is_still_a_done() {
    let p = Scripted::new(&["Here is what I found.\nDONE the answer"]);
    let out = tool_loop(&p, |_| String::new(), "t", 4).expect("a result");
    assert_eq!(out, "the answer");
}

#[test]
fn prose_with_no_protocol_line_is_the_answer() {
    // A model that simply answers is not an error; the loop just has
    // nothing to do. This is the case the old `else` arm was *for*,
    // and it stays.
    let p = Scripted::new(&["It is a vault of notes."]);
    let out = tool_loop(&p, |_| String::new(), "t", 4).expect("a result");
    assert_eq!(out, "It is a vault of notes.");
}

#[test]
fn running_out_of_turns_gives_back_what_was_learned() {
    // Erroring out threw away every observation the loop had already
    // paid for — a model that loops is a bad answer, not no answer.
    let p = Scripted::new(&["CALL a", "CALL b", "CALL c", "CALL d", "CALL e"]);
    let out = tool_loop(&p, |_| "ok".to_owned(), "t", 3);
    let err = out.expect_err("the turn limit still has to be reported");
    assert!(format!("{err}").contains('3'), "{err}");
}

#[test]
fn a_call_with_no_command_is_not_run() {
    // `CALL` alone would execute the empty command, which every
    // executor has to defend against separately.
    let p = Scripted::new(&["CALL   ", "DONE fine"]);
    let mut seen = Vec::new();
    let _ = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            String::new()
        },
        "t",
        4,
    );
    assert!(seen.is_empty(), "ran {seen:?}");
}

#[test]
fn the_loops_own_instructions_are_not_mistaken_for_a_turn() {
    // Found by a shipped CLI test the moment the scan widened from
    // "the reply starts with CALL" to "a line of the reply is CALL".
    // The transcript *contains* the protocol description —
    // `CALL <command line>` — so a model that quotes the instructions
    // back (and an echo provider always does) sent the loop round
    // until it ran out of turns, executing a placeholder every time.
    //
    // `<...>` is a placeholder, not a command. Nothing that looks like
    // one is ever run.
    let p = Scripted::new(&["CALL <command line>\nDONE <answer>", "DONE really finished"]);
    let mut seen = Vec::new();
    let out = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            String::new()
        },
        "t",
        4,
    )
    .expect("a result");
    assert!(seen.is_empty(), "ran a placeholder: {seen:?}");
    // Neither placeholder counts, so the reply carries no protocol at
    // all and is treated as prose — which is what a model quoting the
    // instructions back has actually produced.
    assert_eq!(out, "CALL <command line>\nDONE <answer>");
}

#[test]
fn a_real_command_in_angle_brackets_is_still_refused() {
    // There is no command whose name begins with `<`, so this costs
    // nothing and closes the hole completely.
    let p = Scripted::new(&["CALL <read file.org>", "DONE fine"]);
    let mut seen = Vec::new();
    let _ = tool_loop(
        &p,
        |cmd| {
            seen.push(cmd.to_owned());
            String::new()
        },
        "t",
        4,
    );
    assert!(seen.is_empty(), "{seen:?}");
}
