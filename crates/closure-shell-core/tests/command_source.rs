//! Where a command is implemented.
//!
//! Emacs' `describe-function` ends with a link to the source, and that
//! link is most of why the manual is trustworthy: the answer to "what
//! does this really do" is one keystroke from the answer to "what does
//! it say it does".
//!
//! In a workspace of thirty-four crates, "which file" is the expensive
//! half of that question and the one closure can answer honestly. A
//! *line* it cannot: the commands are a `const` table of names matched
//! in a dispatch, so there is no construction site to record and no
//! runtime location to capture. Generating one would mean a build
//! script that greps its own source, which is a fragile answer to a
//! question a grep already answers well.
//!
//! So this says the file, every file it names exists, and it says
//! nothing it cannot back.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, palette_command_names};

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn a_command_says_which_file_implements_it() {
    let app = ModalApp::new(InputMode::Doom);
    let told = app.describe_command("toggle-wrap").expect("a real command");
    assert!(
        told.source.ends_with(".rs"),
        "not a source file: {}",
        told.source
    );
}

#[test]
fn every_command_names_a_file_that_exists() {
    // A pointer into a file nobody has is worse than no pointer: it
    // reads as an answer.
    let app = ModalApp::new(InputMode::Doom);
    let root = repo_root();
    for name in palette_command_names() {
        let told = app
            .describe_command(name)
            .unwrap_or_else(|| panic!("{name} is in the palette and describes as nothing"));
        assert!(
            root.join(&told.source).exists(),
            "{name} points at {}, which does not exist",
            told.source
        );
    }
}

#[test]
fn the_file_it_names_actually_mentions_the_command() {
    // The cheap version of "is this the right file": the dispatch arm
    // and the palette row both spell the name, so a file that never
    // says it is the wrong file.
    let app = ModalApp::new(InputMode::Doom);
    let root = repo_root();
    for name in ["toggle-wrap", "capture", "manual", "describe-command"] {
        let told = app.describe_command(name).expect("a real command");
        let text = std::fs::read_to_string(root.join(&told.source)).expect("read");
        assert!(
            text.contains(&format!("\"{name}\"")),
            "{name} points at {} and that file never mentions it",
            told.source
        );
    }
}

#[test]
fn a_name_that_is_not_a_command_has_no_source() {
    let app = ModalApp::new(InputMode::Doom);
    assert!(app.describe_command("frobnicate").is_none());
}
