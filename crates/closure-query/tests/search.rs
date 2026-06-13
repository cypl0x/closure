//! Pluggable full-text search backends over a vault directory.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_query::{BuiltinSearch, RipgrepSearch, SearchBackend, backend_for};
use tempfile::TempDir;

fn vault() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("a.org"),
        "* Title\nalpha body line\nsecond beta\n",
    )
    .expect("w");
    fs::write(dir.path().join("b.org"), "* Other\ngamma here\n").expect("w");
    dir
}

#[test]
fn builtin_finds_substring_with_locations() {
    let td = vault();
    let hits = BuiltinSearch.search(td.path(), "beta");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].path.ends_with("a.org"));
    assert_eq!(hits[0].line, 3);
    assert!(hits[0].text.contains("beta"));
}

#[test]
fn builtin_is_case_insensitive() {
    let td = vault();
    assert_eq!(BuiltinSearch.search(td.path(), "GAMMA").len(), 1);
}

#[test]
fn builtin_no_match_is_empty() {
    let td = vault();
    assert!(BuiltinSearch.search(td.path(), "zzz").is_empty());
}

#[test]
fn builtin_only_searches_org_files() {
    let td = vault();
    fs::write(td.path().join("notes.txt"), "alpha in txt\n").expect("w");
    let hits = BuiltinSearch.search(td.path(), "alpha");
    assert!(hits.iter().all(|h| h.path.extension().unwrap() == "org"));
}

#[test]
fn backend_for_selects_engine_by_name() {
    assert_eq!(backend_for("builtin").name(), "builtin");
    assert_eq!(backend_for("ripgrep").name(), "ripgrep");
    assert_eq!(backend_for("rg").name(), "ripgrep");
    assert_eq!(backend_for("unknown").name(), "builtin", "fallback");
}

#[test]
fn ripgrep_matches_builtin_when_available() {
    if std::process::Command::new("rg")
        .arg("--version")
        .output()
        .is_err()
    {
        return; // skip when ripgrep absent
    }
    let td = vault();
    let mut rg = RipgrepSearch.search(td.path(), "beta");
    let mut bi = BuiltinSearch.search(td.path(), "beta");
    rg.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    bi.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    assert_eq!(rg.len(), bi.len());
    assert_eq!(rg[0].line, bi[0].line);
}

#[test]
fn builtin_searches_markdown_files_too() {
    let td = vault();
    fs::write(
        td.path().join("notes.md"),
        "# Heading\nmarkdown delta line\n",
    )
    .expect("w");
    let hits = BuiltinSearch.search(td.path(), "delta");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].path.extension().unwrap() == "md");
}

#[test]
fn builtin_still_ignores_non_text_extensions() {
    let td = vault();
    fs::write(td.path().join("data.json"), "alpha in json\n").expect("w");
    let hits = BuiltinSearch.search(td.path(), "alpha");
    assert!(hits.iter().all(|h| {
        let e = h.path.extension().unwrap();
        e == "org" || e == "md"
    }));
}
