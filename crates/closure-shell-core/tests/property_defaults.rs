//! A file's `#+PROPERTY:` defaults show on the headlines under it.
//!
//! Eighth construct through the preview seam, and the same rule (I12):
//! the file keeps its own bytes, the reader sees what the file means. A
//! default that no shell displays is a line the author wrote and nobody
//! can act on — the parsed-and-invisible state this run has spent its
//! time removing.
//!
//! Inherited properties are marked as inherited rather than mixed in
//! silently. A reader deciding whether to edit a drawer needs to know
//! whether the value is *in* it: "delete this line to change it" and
//! "change the line at the top of the file" are different instructions.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
#+PROPERTY: header-args :tangle no
#+PROPERTY: category reading

* Inherits
:PROPERTIES:
:ID: 01PROPDEFAULT00000001
:END:
* Overrides
:PROPERTIES:
:ID: 01PROPDEFAULT00000002
:HEADER-ARGS: :tangle yes
:END:
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn props(app: &mut ModalApp, shell: &Shell, title: &str) -> Vec<(String, String, bool)> {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.effective_properties(shell)
}

#[test]
fn a_headline_shows_the_files_default() {
    let (_d, shell, mut app) = app(VAULT);
    let got = props(&mut app, &shell, "Inherits");
    let ha = got
        .iter()
        .find(|(k, _, _)| k == "HEADER-ARGS")
        .expect("the default");
    assert_eq!(ha.1, ":tangle no");
    assert!(ha.2, "an inherited value is not marked as inherited");
}

#[test]
fn a_drawer_of_its_own_wins() {
    // The whole point of a default. If the file's value overrode the
    // headline's, `#+PROPERTY:` would be a way to break every file
    // that used it.
    let (_d, shell, mut app) = app(VAULT);
    let got = props(&mut app, &shell, "Overrides");
    let ha = got
        .iter()
        .find(|(k, _, _)| k == "HEADER-ARGS")
        .expect("the property");
    assert_eq!(ha.1, ":tangle yes");
    assert!(!ha.2, "an own property is marked inherited");
}

#[test]
fn a_default_it_does_not_override_is_still_inherited() {
    let (_d, shell, mut app) = app(VAULT);
    let got = props(&mut app, &shell, "Overrides");
    let cat = got
        .iter()
        .find(|(k, _, _)| k == "CATEGORY")
        .expect("category");
    assert_eq!(cat.1, "reading");
    assert!(cat.2);
}

#[test]
fn a_file_with_no_defaults_shows_only_the_drawer() {
    let (_d, shell, mut app) =
        app("* Alone\n:PROPERTIES:\n:ID: 01PROPDEFAULT00000003\n:OWN: yes\n:END:\n");
    let got = props(&mut app, &shell, "Alone");
    assert!(got.iter().all(|(_, _, inherited)| !inherited), "{got:?}");
}

#[test]
fn the_file_is_not_rewritten() {
    let (dir, shell, mut app) = app(VAULT);
    let _ = props(&mut app, &shell, "Inherits");
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(
        !on_disk.contains(":CATEGORY:"),
        "a default reached a drawer"
    );
    assert!(
        on_disk.contains("#+PROPERTY: category reading"),
        "{on_disk}"
    );
}
