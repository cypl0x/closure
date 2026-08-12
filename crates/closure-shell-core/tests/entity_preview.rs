//! `\alpha` reads as α in the pane, and stays `\alpha` in the file.
//!
//! `closure-org` learned to find entities and LaTeX fragments and
//! nothing asked it to. The conformance matrix called the construct
//! understood, which was true of the parser and invisible to a reader:
//! a note about `\alpha` showed a backslash and a word.
//!
//! The rule is already settled and this is the same split for a
//! different construct. I12: the editor shows the file, because it
//! writes back what it shows; the preview shows what the file means,
//! because it never writes. A widget expands there and an entity
//! resolves there, for one reason.
//!
//! LaTeX is deliberately *not* rendered. `$x^2$` needs a typesetter,
//! and a half-rendered formula is worse than the source — the source
//! at least says what it is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Notes
:PROPERTIES:
:ID: 01ENTITY000000000000001
:END:
the \\alpha particle and the \\beta one
a range of 1\\ndash 10 \\rightarrow done
maths stays: $x^2$
* Plain
:PROPERTIES:
:ID: 01ENTITY000000000000002
:END:
a windows path C:\\notes and a \\nosuchentity
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
fn a_known_entity_reads_as_its_character() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains('α'), "{}", d.body);
    assert!(d.body.contains('β'), "{}", d.body);
    assert!(!d.body.contains("\\alpha"), "{}", d.body);
}

#[test]
fn arrows_and_dashes_too() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains('–'), "ndash: {}", d.body);
    assert!(d.body.contains('→'), "rightarrow: {}", d.body);
}

#[test]
fn maths_is_left_as_it_was_written() {
    // A fragment needs a typesetter. Half-rendering it would be worse
    // than the source, which at least says what it is.
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains("$x^2$"), "{}", d.body);
}

#[test]
fn a_name_closure_does_not_know_is_left_alone() {
    // `None` from `entity_char` means leave the source; a guess would
    // be a wrong character, which is worse than a backslash.
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Plain");
    assert!(d.body.contains("\\nosuchentity"), "{}", d.body);
}

#[test]
fn a_backslash_that_is_not_an_entity_survives() {
    // `C:\notes` is a path somebody typed, and `notes` is not an entity
    // name closure knows — so the line has to come back unchanged.
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Plain");
    assert!(d.body.contains("C:\\notes"), "{}", d.body);
}

#[test]
fn the_editor_still_holds_the_backslash() {
    // I12's other half: what writes back shows the file.
    let (_d, mut shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "Notes");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    let buffer = app.body_buffer().to_owned();
    assert!(
        buffer.contains("\\alpha"),
        "the editor is showing a character it would write back as one: {buffer}"
    );
}

#[test]
fn the_file_on_disk_is_untouched() {
    let (dir, shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "Notes");
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert_eq!(on_disk, VAULT);
}

// === org's own macros, same seam (2026-08-12) ===

#[test]
fn an_org_macro_reads_as_its_text() {
    let (_d, shell, mut app) = app(
        "#+MACRO: project closure\n* Notes\n:PROPERTIES:\n:ID: 01ENTITY000000000000003\n:END:\n\
         built with {{{project}}} today\n",
    );
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains("built with closure today"), "{}", d.body);
}

#[test]
fn a_widget_reference_is_not_touched_by_the_macro_pass() {
    // Two braces are closure's and three are org's, and the preview
    // runs both expanders — so this is where confusing them would show.
    let (_d, shell, mut app) = app(
        "#+MACRO: project closure\n* Notes\n:PROPERTIES:\n:ID: 01ENTITY000000000000004\n:END:\n\
         a {{card}} stays a widget reference\n",
    );
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains("{{card}}"), "{}", d.body);
}

#[test]
fn a_macro_from_a_setupfile_works_like_a_local_one() {
    // The point of a setupfile: one file of shared definitions. A macro
    // it brings in has to be as usable as one defined in the document.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("theme.org"), "#+MACRO: project closure\n").unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "#+SETUPFILE: \"theme.org\"\n* Notes\n:PROPERTIES:\n:ID: 01ENTITY000000000000005\n:END:\n\
         built with {{{project}}} today\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    let d = detail_of(&mut app, &shell, "Notes");
    assert!(d.body.contains("built with closure today"), "{}", d.body);
}
