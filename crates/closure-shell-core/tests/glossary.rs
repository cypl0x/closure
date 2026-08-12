//! Description lists are a glossary you can look a word up in.
//!
//! `- closure :: the program` is the shape a glossary has, and reading
//! it as an ordinary item meant the terms were spread across a vault
//! with no way to see them together or find one by its word.
//!
//! The vault-wide view is the offer, not a per-headline row. A glossary
//! whose entries you can only see by already standing on the note that
//! defines them answers a question nobody has.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const A: &str = "\
* Glossary
:PROPERTIES:
:ID: 01GLOSSARY0000000001
:END:
- closure :: the program you are reading this in
- vault :: a directory of org files
";

const B: &str = "\
* Notes on Rust
:PROPERTIES:
:ID: 01GLOSSARY0000000002
:END:
- borrow :: a reference that outlives nothing
see std::collections for the rest
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), A).unwrap();
    std::fs::write(dir.path().join("b.org"), B).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn every_term_in_the_vault_is_listed() {
    let (_d, shell, app) = app();
    let terms: Vec<String> = app
        .glossary_rows(&shell)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(terms.contains(&"closure".to_owned()), "{terms:?}");
    assert!(terms.contains(&"borrow".to_owned()), "{terms:?}");
    assert_eq!(terms.len(), 3, "{terms:?}");
}

#[test]
fn a_term_carries_its_definition() {
    let (_d, shell, app) = app();
    let rows = app.glossary_rows(&shell);
    let v = rows.iter().find(|(t, _)| t == "vault").expect("vault");
    assert_eq!(v.1, "a directory of org files");
}

#[test]
fn the_terms_are_sorted_so_the_list_is_lookupable() {
    // A glossary in document-then-file order is a list you read; sorted
    // is a list you look something up in, which is what it is for.
    let (_d, shell, app) = app();
    let terms: Vec<String> = app
        .glossary_rows(&shell)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let mut sorted = terms.clone();
    sorted.sort();
    assert_eq!(terms, sorted);
}

#[test]
fn a_rust_path_in_prose_is_not_a_term() {
    // `std::collections` sits in b.org and must not be a glossary entry.
    let (_d, shell, app) = app();
    let terms: Vec<String> = app
        .glossary_rows(&shell)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(!terms.iter().any(|t| t.contains("std")), "{terms:?}");
}

#[test]
fn a_vault_with_no_descriptions_has_no_glossary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* A\n:PROPERTIES:\n:ID: 01GLOSSARY0000000003\n:END:\n- an item\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let app = ModalApp::new(InputMode::Doom);
    assert!(app.glossary_rows(&shell).is_empty());
}
