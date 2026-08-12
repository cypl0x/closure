//! A footnote is readable where it is referenced.
//!
//! `[fn:1]` sits in the body and its text sits somewhere else — further
//! down the file, or in another file entirely. `find_footnotes` could
//! see both and nothing put them together, so the pane showed a
//! reference to something the reader could not read. A footnote you
//! cannot follow is a pointer to nothing.
//!
//! Fourth construct through the seam the composition work opened, and
//! the same rule as the other three (I12): the editor shows the file,
//! the preview shows what the file means. The reference stays where the
//! author put it — a footnote marker is part of the sentence — and its
//! text is gathered underneath, which is where a footnote goes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Argument
:PROPERTIES:
:ID: 01FOOTNOTE0000000000001
:END:
the claim rests on a survey[fn:survey] and a reading[fn:reading]

[fn:survey] Mostly of people who already agreed.
[fn:reading] Of one paragraph, twice.
* No notes
:PROPERTIES:
:ID: 01FOOTNOTE0000000000002
:END:
plain prose with a [bracket] in it
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn detail_of(
    app: &mut ModalApp,
    shell: &Shell,
    title: &str,
) -> std::sync::Arc<closure_shell_core::Detail> {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail")
}

#[test]
fn the_notes_are_offered_beside_the_headline_that_cites_them() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Argument");
    let notes = app.footnote_rows(&shell);
    let names: Vec<&str> = notes.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"survey"), "{names:?}");
    assert!(names.contains(&"reading"), "{names:?}");
    // And the body still reads as the author wrote it.
    assert!(d.body.contains("[fn:survey]"), "{}", d.body);
}

#[test]
fn each_note_carries_its_text() {
    let (_d, shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "Argument");
    let notes = app.footnote_rows(&shell);
    let survey = notes.iter().find(|(n, _)| n == "survey").expect("survey");
    assert!(
        survey.1.contains("already agreed"),
        "the note came back without its text: {survey:?}"
    );
}

#[test]
fn a_reference_with_no_definition_says_so_rather_than_vanishing() {
    // A dangling footnote is a mistake in the file, and showing
    // nothing hides it — the same call the dangling relation made.
    let (_d, shell, mut app) =
        app("* A\n:PROPERTIES:\n:ID: 01FOOTNOTE0000000000003\n:END:\ncites nothing[fn:missing]\n");
    let _ = detail_of(&mut app, &shell, "A");
    let notes = app.footnote_rows(&shell);
    let (_, text) = notes.first().expect("the dangling reference is listed");
    assert!(text.contains('?'), "{text}");
}

#[test]
fn a_headline_that_cites_none_offers_none() {
    let (_d, shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "No notes");
    assert!(app.footnote_rows(&shell).is_empty());
}

#[test]
fn a_bracket_that_is_not_a_footnote_is_not_one() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "No notes");
    assert!(d.body.contains("[bracket]"), "{}", d.body);
    assert!(app.footnote_rows(&shell).is_empty());
}
