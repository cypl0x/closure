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
