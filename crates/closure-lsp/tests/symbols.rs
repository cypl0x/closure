//! LSP basics: document symbols from org headlines and
//! go-to-definition for id: links.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_lsp::{definition_of, document_symbols};
use closure_store::Vault;

#[test]
fn symbols_list_headlines_with_lines_and_levels() {
    let src = "* Top\nbody\n** Child\n* Second\n";
    let syms = document_symbols(src);
    let view: Vec<(&str, u32, u8)> = syms
        .iter()
        .map(|s| (s.name.as_str(), s.line, s.level))
        .collect();
    assert_eq!(view, vec![("Top", 0, 1), ("Child", 2, 2), ("Second", 3, 1)]);
}

#[test]
fn symbols_strip_todo_and_tags() {
    let syms = document_symbols("* TODO [#A] Pay rent :money:\n");
    assert_eq!(syms[0].name, "Pay rent");
}

#[test]
fn symbols_ignore_non_headline_stars() {
    let syms = document_symbols("para\n *not a headline\n*also-not\n");
    assert!(syms.is_empty());
}

#[test]
fn definition_resolves_id_link_to_file_and_headline_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("target.org"),
        "* Filler\n* The target\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let (path, line) = definition_of(&v, "id:01HXAAAAAAAAAAAAAAAAAAAAAA").expect("resolves");
    assert!(path.ends_with("target.org"));
    assert_eq!(line, 1, "headline line, not the drawer line");
}

#[test]
fn definition_accepts_bare_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("t.org"),
        "* X\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    assert!(definition_of(&v, "01HXAAAAAAAAAAAAAAAAAAAAAA").is_some());
}

#[test]
fn definition_of_unknown_id_is_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("t.org"), "* X\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    assert!(definition_of(&v, "id:01HXZZZZZZZZZZZZZZZZZZZZZZ").is_none());
}
