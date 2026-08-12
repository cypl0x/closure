//! An abbreviated link says where it goes.
//!
//! `[[gh:cypl0x/closure]]` is shorter to write and, unexpanded, is a
//! link to a scheme no browser has — so the abbreviation cost the
//! author exactly the thing it was meant to save. Ninth construct
//! through the preview seam, and the same rule (I12): the file keeps
//! `[[gh:...]]`, the reader is told where that is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
#+LINK: gh https://github.com/%s
#+LINK: wiki https://en.wikipedia.org/wiki/

* Reading
:PROPERTIES:
:ID: 01LINKROWS0000000001
:END:
the repo is [[gh:cypl0x/closure]] and the format is [[wiki:Org-mode]]
* Plain
:PROPERTIES:
:ID: 01LINKROWS0000000002
:END:
an ordinary [[https://example.com][link]] and an [[id:01LINKROWS0000000001][internal one]]
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn rows(app: &mut ModalApp, shell: &Shell, title: &str) -> Vec<(String, String)> {
    let list = app.rows(shell);
    let i = list.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.abbreviated_link_rows(shell)
}

#[test]
fn an_abbreviation_is_shown_with_where_it_goes() {
    let (_d, shell, mut app) = app(VAULT);
    let got = rows(&mut app, &shell, "Reading");
    assert!(
        got.contains(&(
            "gh:cypl0x/closure".to_owned(),
            "https://github.com/cypl0x/closure".to_owned()
        )),
        "{got:?}"
    );
}

#[test]
fn every_abbreviation_in_the_body_is_listed() {
    let (_d, shell, mut app) = app(VAULT);
    assert_eq!(rows(&mut app, &shell, "Reading").len(), 2);
}

#[test]
fn an_ordinary_link_is_not_listed() {
    // The row exists to answer "where does this actually go". For a
    // link that already says so, the answer is on screen already, and
    // repeating it is noise.
    let (_d, shell, mut app) = app(VAULT);
    assert!(rows(&mut app, &shell, "Plain").is_empty());
}

#[test]
fn the_body_still_reads_as_the_author_wrote_it() {
    // I12. The whole point of an abbreviation is the short form.
    let (_d, shell, mut app) = app(VAULT);
    let list = app.rows(&shell);
    let i = list.iter().position(|r| r.title == "Reading").expect("row");
    app.select(i, &shell);
    let d = app.selected_detail(&shell).expect("a detail");
    assert!(d.body.contains("[[gh:cypl0x/closure]]"), "{}", d.body);
}

#[test]
fn a_file_with_no_declarations_lists_nothing() {
    let (_d, shell, mut app) =
        app("* A\n:PROPERTIES:\n:ID: 01LINKROWS0000000003\n:END:\nsee [[gh:owner/repo]]\n");
    assert!(rows(&mut app, &shell, "A").is_empty());
}
