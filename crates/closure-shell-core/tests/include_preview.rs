//! `#+INCLUDE:` shows what it names, in the pane that reads.
//!
//! `resolve_includes` parses and resolves and no shell called it, so a
//! document assembled out of includes previewed as a list of
//! filenames — which is what the file says and not what it means.
//!
//! Third construct through the same seam, and by now the rule is worth
//! stating as a rule rather than rediscovering: I12 splits every one of
//! these the same way. The editor shows the file because it writes back
//! what it shows. The preview shows what the file means because it
//! never writes. Widgets, entities and includes are one decision
//! applied three times.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn app(files: &[(&str, &str)]) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
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

const MAIN: &str = "\
* Assembled
:PROPERTIES:
:ID: 01INCPREV0000000000001
:END:
before
#+INCLUDE: \"part.txt\"
after
";

#[test]
fn the_preview_shows_what_the_include_names() {
    let (_d, shell, mut app) = app(&[("notes.org", MAIN), ("part.txt", "the included words\n")]);
    let d = detail_of(&mut app, &shell, "Assembled");
    assert!(d.body.contains("the included words"), "{}", d.body);
}

#[test]
fn the_editor_still_holds_the_directive() {
    let (_d, mut shell, mut app) =
        app(&[("notes.org", MAIN), ("part.txt", "the included words\n")]);
    let _ = detail_of(&mut app, &shell, "Assembled");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    let buffer = app.body_buffer().to_owned();
    assert!(
        buffer.contains("#+INCLUDE:"),
        "the editor is showing content it would write back as its own: {buffer}"
    );
}

#[test]
fn a_file_that_is_not_there_says_so_instead_of_vanishing() {
    // A directive that resolves to nothing must not read as an empty
    // note. Same reasoning as a composition that fails to expand.
    let (_d, shell, mut app) = app(&[("notes.org", MAIN)]);
    let d = detail_of(&mut app, &shell, "Assembled");
    let said = d
        .composition_error
        .clone()
        .unwrap_or_else(|| d.body.clone());
    assert!(said.contains("part.txt"), "{said}");
}

#[test]
fn a_headline_with_no_include_is_untouched() {
    let (_d, shell, mut app) = app(&[(
        "notes.org",
        "* Plain\n:PROPERTIES:\n:ID: 01INCPREV0000000000002\n:END:\njust prose\n",
    )]);
    let d = detail_of(&mut app, &shell, "Plain");
    assert_eq!(d.body.trim(), "just prose");
}

#[test]
fn the_files_on_disk_are_untouched() {
    let (dir, shell, mut app) = app(&[("notes.org", MAIN), ("part.txt", "the included words\n")]);
    let _ = detail_of(&mut app, &shell, "Assembled");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.org")).unwrap(),
        MAIN
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("part.txt")).unwrap(),
        "the included words\n"
    );
}
