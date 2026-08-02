//! "enhanced prompt — powerline style prompt with Doom Vibrant Color
//! theme; Optimize readibility. You can use the different Maple NF
//! fonts and fontweights and sizes and whatnot; Make it pretty"
//!
//! A powerline prompt is segments: each one a block of colour with the
//! next one's arrow biting into it. What the segments *say* is not a
//! gpui question — every shell wants the same words — but the whole
//! label-and-hint table lived in the painter, so the terminal and the
//! web shell had no way to show the same prompt.
//!
//! It is data now: what this prompt is called, what it will do, and
//! which tone it carries. The arrows and the weights stay in the shell
//! that can draw them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, PromptTone, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQCHROME000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQCHROME000000000001"));
    (dir, shell, app)
}

#[test]
fn a_prompt_says_what_it_is() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "search");
    let c = app.prompt_chrome(&shell).expect("a prompt");
    assert!(!c.label.is_empty(), "{c:?}");
}

#[test]
fn each_kind_of_prompt_has_its_own_tone() {
    // Colour carries meaning: a rename is not a search is not a
    // destructive `:` line, and a powerline of one colour is a stripe.
    let (_d, mut shell, mut app) = fixture();
    let mut tones = std::collections::BTreeSet::new();
    for command in ["search", "rename", "ex-command", "refile"] {
        app.on_key(&mut shell, "escape", false, false, None);
        app.run(&mut shell, command);
        if let Some(c) = app.prompt_chrome(&shell) {
            tones.insert(c.tone);
        }
    }
    assert!(tones.len() > 1, "every prompt the same colour: {tones:?}");
}

#[test]
fn the_ex_line_reads_as_a_command_line() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "ex-command");
    let c = app.prompt_chrome(&shell).expect("a prompt");
    assert_eq!(c.tone, PromptTone::Command, "{c:?}");
}

#[test]
fn a_search_says_how_many_it_found() {
    // The hint is the segment on the right, and for a filter the
    // useful thing to put there is the count.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "search");
    let c = app.prompt_chrome(&shell).expect("a prompt");
    assert!(c.hint.contains("match"), "{c:?}");
}

#[test]
fn a_surface_that_is_not_a_prompt_has_no_chrome() {
    let (_d, shell, app) = fixture();
    assert!(app.prompt_chrome(&shell).is_none());
}

#[test]
fn the_label_never_ends_in_its_own_punctuation() {
    // A powerline segment is a block of colour, so the `:` that used
    // to separate a label from its field is the arrow's job now —
    // "rename:" inside a coloured block reads as a typo.
    let (_d, mut shell, mut app) = fixture();
    for command in ["search", "rename", "refile", "capture"] {
        app.on_key(&mut shell, "escape", false, false, None);
        app.run(&mut shell, command);
        if let Some(c) = app.prompt_chrome(&shell) {
            assert!(
                !c.label.trim_end().ends_with(':'),
                "{command}: {:?}",
                c.label
            );
        }
    }
}

// === the keyword in a prompt label is a keyword ===
//
// "new sibling TODO: (shift+c aka C) should color the word TODO just
// in the same color as TODO is in the headline tree view — This is
// still not fixed", with a screenshot of `+ new sibling TODO:` and the
// note "Is still not red, despite you have said it is fixed".
//
// The label was one string in one colour, so the word TODO inside it
// was prompt-chrome text like any other. A shell cannot colour part of
// a string it is handed whole: the keyword has to be its own field,
// the way the icon had to be its own field for the same reason.

#[test]
fn the_todo_prompt_hands_the_keyword_over_separately() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "add-todo-heading");
    let chrome = app.prompt_chrome(&shell).expect("a prompt");
    assert_eq!(
        chrome.keyword.as_deref(),
        Some("TODO"),
        "the keyword has to be paintable on its own: {chrome:?}"
    );
}

#[test]
fn the_label_no_longer_carries_the_keyword_in_its_text() {
    // Otherwise it is drawn twice — once in the label and once in the
    // keyword's own colour.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "add-todo-heading");
    let chrome = app.prompt_chrome(&shell).expect("a prompt");
    assert!(
        !chrome.label.contains("TODO"),
        "the keyword is still inside the label: {:?}",
        chrome.label
    );
    assert!(
        chrome.label.contains("sibling"),
        "and the rest of it survived: {:?}",
        chrome.label
    );
}

#[test]
fn a_plain_sibling_has_no_keyword_to_paint() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "add-sibling");
    let chrome = app.prompt_chrome(&shell).expect("a prompt");
    assert_eq!(chrome.keyword, None);
    assert!(chrome.label.contains("sibling"));
}

#[test]
fn the_keyword_is_the_vaults_own_first_keyword() {
    // Not the literal string "TODO": a vault that declares
    // `todo_keywords = NEXT | DONE` gets NEXT here, and the colour it
    // is painted in is that keyword's.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "add-todo-heading");
    let chrome = app.prompt_chrome(&shell).expect("a prompt");
    let first = shell.vault.todo_keywords().first().cloned();
    assert_eq!(chrome.keyword, first.or_else(|| Some("TODO".to_owned())));
}
