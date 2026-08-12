//! Radio targets turn later mentions into navigation.
//!
//! `<<<closure>>>` in a glossary makes every later mention of the word
//! a link to it — org's one construct that turns ordinary prose into
//! navigation without the author marking anything. Parsed and offered
//! nowhere, which for this construct in particular means the feature
//! did not exist: nobody writes a radio target for the parser.
//!
//! The cost is the interesting part and the reason this has a test
//! beside the behaviour ones. Finding the targets means reading every
//! document, so it is memoised per revision — landing on a headline
//! must not cost the vault (I11).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Glossary
:PROPERTIES:
:ID: 01RADIOP000000000000001
:END:
<<<closure>>> is the program.
* Notes
:PROPERTIES:
:ID: 01RADIOP000000000000002
:END:
closure reads org files, and enclosure is a different word.
";

fn built(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn select(app: &mut ModalApp, shell: &Shell, title: &str) {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    let _ = app.selected_detail(shell);
}

#[test]
fn a_mention_is_offered_as_a_link() {
    let (_d, shell, mut app) = built(VAULT);
    select(&mut app, &shell, "Notes");
    let rows = app.radio_rows(&shell);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].1, "closure");
}

#[test]
fn a_word_that_merely_contains_it_is_not_offered() {
    let (_d, shell, mut app) = built(VAULT);
    select(&mut app, &shell, "Notes");
    let rows = app.radio_rows(&shell);
    assert!(
        !rows.iter().any(|(w, _)| w.contains("enclosure")),
        "{rows:?}"
    );
}

#[test]
fn the_definition_offers_nothing() {
    let (_d, shell, mut app) = built(VAULT);
    select(&mut app, &shell, "Glossary");
    assert!(app.radio_rows(&shell).is_empty());
}

#[test]
fn a_vault_with_no_targets_offers_nothing() {
    let (_d, shell, mut app) =
        built("* A\n:PROPERTIES:\n:ID: 01RADIOP000000000000003\n:END:\nplain prose\n");
    select(&mut app, &shell, "A");
    assert!(app.radio_rows(&shell).is_empty());
}
