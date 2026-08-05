//! "headlines are queried in a Vec<String>. Improve."
//!
//! `headline_rows` handed back `Vec<(String, String)>` and
//! `BlockRow` was a type *alias* for `(String, String, String)`. Two
//! and three positional strings, with nothing in the type to say which
//! is the title and which is the id — so every reader has to go and
//! find the constructor, and every caller destructures by position:
//!
//!     |(title, _)| title.clone()
//!     |(file, lang, line)| format!("{file} {lang} {line}")
//!
//! Get one of those the wrong way round and it compiles. The file
//! already has `RefileRow`, `TagRow`, `LinkCompletion` — named fields
//! for exactly this reason; the headline and block rows were the two
//! that never got them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Alpha
:PROPERTIES:
:ID: 01TYPEDAAAAAAAAAAAAAAAAAAA
:END:
#+BEGIN_SRC shell
echo one
#+END_SRC
** Beta
:PROPERTIES:
:ID: 01TYPEDBBBBBBBBBBBBBBBBBBB
:END:
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn a_headline_row_says_which_field_is_which() {
    let (_d, shell, mut app) = app();
    app.select(0, &shell);
    let rows = app.headline_rows(&shell);
    let first = rows.first().expect("a headline");
    assert_eq!(first.title, "Alpha");
    assert_eq!(first.id, "01TYPEDAAAAAAAAAAAAAAAAAAA");
}

#[test]
fn every_headline_in_the_file_is_there() {
    let (_d, shell, mut app) = app();
    app.select(0, &shell);
    let rows = app.headline_rows(&shell);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Alpha", "Beta"]);
}

#[test]
fn a_block_row_says_which_field_is_which() {
    let (_d, shell, app) = app();
    let rows = app.block_rows(&shell);
    let first = rows.first().expect("a source block");
    assert!(first.file.ends_with("notes.org"), "{:?}", first.file);
    assert_eq!(first.lang, "shell");
    assert!(!first.line.is_empty(), "the line it is on");
}
