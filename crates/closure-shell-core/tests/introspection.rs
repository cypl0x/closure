//! "Introspection like in Emacs."
//!
//! Emacs' answer to "what does this key do?" is `C-h k`, and to "how do
//! I run this?" is `C-h f` / `where-is`. Both answer *from the running
//! program* rather than from a document someone remembered to update —
//! which is the whole reason they are trusted.
//!
//! closure has every piece and nothing that joins them: the registry
//! knows each command's name, description and section, the keymap knows
//! which chords reach it, `where-is` answers on the CLI and nowhere
//! else, and the palette shows descriptions but cannot be asked about a
//! *key*.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01HELPAAAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn describe_key_says_what_a_chord_runs_and_what_it_does() {
    // `C-h k`: press the key, be told. Not "capture" — the name is
    // half the answer and the sentence is the other half.
    let (_d, _shell, app) = app();
    let told = app.describe_key("c").expect("`c` is bound in Doom");
    assert_eq!(told.chord, "c");
    assert_eq!(told.command, "capture");
    assert!(
        !told.description.is_empty(),
        "the name without the sentence is half an answer"
    );
    assert!(!told.section.is_empty());
}

#[test]
fn describe_key_on_an_unbound_chord_says_so() {
    // Emacs says "M-# is undefined". Silence reads as a broken
    // keyboard, which is the complaint that started the which-key work.
    let (_d, _shell, app) = app();
    assert!(app.describe_key("C-M-S-F13").is_none());
}

#[test]
fn describe_command_lists_every_chord_that_reaches_it() {
    // `where-is`. Every key, not the first the keymap happens to hold —
    // the palette already learned that lesson.
    let (_d, _shell, app) = app();
    let told = app.describe_command("toggle-wrap").expect("a real command");
    assert_eq!(told.command, "toggle-wrap");
    assert!(!told.description.is_empty());
    assert!(
        told.chords.iter().any(|c| *c == "g W"),
        "the chord is missing: {:?}",
        told.chords
    );
}

#[test]
fn describe_command_on_a_name_that_is_not_one_says_so() {
    let (_d, _shell, app) = app();
    assert!(app.describe_command("frobnicate").is_none());
}

#[test]
fn a_command_with_no_chord_still_describes() {
    // Reachable from the palette only is a real state, and "no key"
    // is an answer rather than a failure.
    let (_d, _shell, app) = app();
    let told = app
        .describe_command("trust-language")
        .expect("a real command");
    assert!(told.chords.is_empty(), "{:?}", told.chords);
    assert!(!told.description.is_empty());
}

#[test]
fn the_surface_opens_and_goes_back() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "describe-key");
    assert_eq!(app.surface(), ModalSurface::DescribeKey);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_ne!(app.surface(), ModalSurface::DescribeKey);
}

#[test]
fn pressing_a_key_in_the_surface_describes_it_and_returns() {
    // The Emacs shape: `C-h k` waits for one key, tells you, and the
    // next key is an ordinary key again.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "describe-key");
    app.on_key(&mut shell, "c", false, false, Some('c'));
    assert_ne!(
        app.surface(),
        ModalSurface::DescribeKey,
        "it kept waiting after being answered"
    );
    let said = app.status();
    assert!(
        said.contains("capture"),
        "it did not say what `c` runs: {said}"
    );
}

#[test]
fn every_command_the_registry_lists_can_be_described() {
    // The property, not the three that happened to be tried: a command
    // in the palette with no description is a row you cannot act on.
    let (_d, _shell, app) = app();
    for name in closure_shell_core::palette_command_names() {
        let told = app
            .describe_command(name)
            .unwrap_or_else(|| panic!("`{name}` is in the palette and cannot be described"));
        assert!(!told.description.is_empty(), "`{name}` has no description");
    }
}
