//! `#+SETUPFILE: "theme.org"` — another file's keywords, in this one.
//!
//! The same shape as `#+INCLUDE:` and deliberately not the same thing.
//! An include pulls in *content*: whatever the file says, wherever the
//! directive sits. A setupfile pulls in *settings* — `#+MACRO:`,
//! `#+TODO:`, `#+COLUMNS:` — and nothing else, which is what lets a
//! vault keep one file of shared definitions without every document
//! that references it inheriting its prose.
//!
//! That distinction is the whole of this. Reusing `resolve_includes`
//! and calling it done would have quietly turned every setupfile into
//! an include, and the difference only shows up in somebody's document
//! months later.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::setup_keywords;

fn dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(d.path().join(name), body).unwrap();
    }
    d
}

const THEME: &str = "\
#+MACRO: project closure
#+TODO: TODO NEXT | DONE
* A headline the setupfile happens to contain
its prose
";

#[test]
fn the_keywords_come_through() {
    let d = dir(&[("theme.org", THEME)]);
    let got = setup_keywords("#+SETUPFILE: \"theme.org\"\n", d.path());
    assert!(got.contains("#+MACRO: project closure"), "{got}");
    assert!(got.contains("#+TODO: TODO NEXT | DONE"), "{got}");
}

#[test]
fn the_prose_does_not() {
    // The difference from an include, and the reason this is its own
    // function: settings, not content.
    let d = dir(&[("theme.org", THEME)]);
    let got = setup_keywords("#+SETUPFILE: \"theme.org\"\n", d.path());
    assert!(!got.contains("its prose"), "{got}");
    assert!(!got.contains("A headline the setupfile"), "{got}");
}

#[test]
fn a_document_with_no_setupfile_gets_nothing() {
    let d = dir(&[]);
    assert!(setup_keywords("* Just a headline\n", d.path()).is_empty());
}

#[test]
fn a_file_that_is_not_there_is_silence_rather_than_a_failure() {
    // A missing setupfile costs settings, not the document. An include
    // that resolves to nothing is an error because its content was
    // meant to be read; a setupfile's absence means defaults.
    let d = dir(&[]);
    assert!(setup_keywords("#+SETUPFILE: \"gone.org\"\n", d.path()).is_empty());
}

#[test]
fn a_setupfile_may_point_at_another() {
    let d = dir(&[
        ("a.org", "#+SETUPFILE: \"b.org\"\n#+MACRO: from_a yes\n"),
        ("b.org", "#+MACRO: from_b yes\n"),
    ]);
    let got = setup_keywords("#+SETUPFILE: \"a.org\"\n", d.path());
    assert!(got.contains("from_a"), "{got}");
    assert!(got.contains("from_b"), "{got}");
}

#[test]
fn a_ring_of_setupfiles_ends() {
    // Same bound as every other expander here: a cycle stops rather
    // than spins (I5).
    let d = dir(&[
        ("a.org", "#+SETUPFILE: \"b.org\"\n#+MACRO: a 1\n"),
        ("b.org", "#+SETUPFILE: \"a.org\"\n#+MACRO: b 2\n"),
    ]);
    let got = setup_keywords("#+SETUPFILE: \"a.org\"\n", d.path());
    assert!(got.len() < 10_000, "it ran away: {} bytes", got.len());
}

#[test]
fn the_macros_it_brings_in_are_usable() {
    // The point of the feature: one file of shared definitions.
    let d = dir(&[("theme.org", THEME)]);
    let doc = "#+SETUPFILE: \"theme.org\"\n";
    let settings = setup_keywords(doc, d.path());
    let combined = format!("{settings}{doc}");
    assert_eq!(
        closure_org::expand_org_macros("built with {{{project}}}", &combined),
        "built with closure"
    );
}
