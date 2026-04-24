//! closure-query integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn build_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    dir
}

#[test]
fn by_tag_finds_matching_headlines() {
    let td = build_vault(&[
        ("a.org", "* One :work:\n"),
        ("b.org", "* Two :home:\n"),
        ("c.org", "* Three :work:urgent:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let matches = closure_query::by_tag(&v, "work");
    assert_eq!(matches.len(), 2);
    let titles: Vec<&str> = matches.iter().map(|m| m.headline.title()).collect();
    assert!(titles.contains(&"One"));
    assert!(titles.contains(&"Three"));
}

#[test]
fn by_todo_filters_by_keyword() {
    let td = build_vault(&[("t.org", "* TODO A\n* DONE B\n* TODO C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let todos = closure_query::by_todo(&v, "TODO");
    assert_eq!(todos.len(), 2);
    let dones = closure_query::by_todo(&v, "DONE");
    assert_eq!(dones.len(), 1);
}

#[test]
fn by_title_substring_matches() {
    let td = build_vault(&[("x.org", "* hello world\n* goodbye world\n* unrelated\n")]);
    let v = Vault::open(td.path()).expect("open");
    let matches = closure_query::by_title_substring(&v, "world");
    assert_eq!(matches.len(), 2);
}

#[test]
fn by_level_filters_by_depth() {
    let td = build_vault(&[("n.org", "* A\n** B\n*** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(closure_query::by_level(&v, 1).len(), 1);
    assert_eq!(closure_query::by_level(&v, 2).len(), 1);
    assert_eq!(closure_query::by_level(&v, 3).len(), 1);
}

#[test]
fn backlinks_find_id_link_references() {
    let td = build_vault(&[
        (
            "target.org",
            "* Target\n:PROPERTIES:\n:ID: 01HXTGTTARGET0000000000AA\n:END:\n",
        ),
        (
            "src.org",
            "* Source\nSee [[id:01HXTGTTARGET0000000000AA][the target]].\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let id = closure_core::BlockId::from_existing("01HXTGTTARGET0000000000AA");
    let back = closure_query::backlinks(&v, &id);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].headline.title(), "Source");
}
