//! `#+INCLUDE: "other.org"` — composition by file.
//!
//! The line was preserved and the file was never read, so a document
//! assembled out of includes showed a list of filenames.
//!
//! It is the same shape as a widget and gets the same rules, which is
//! the point of having settled them: the expansion is a *view* and is
//! never written back (I12), a cycle is reported with the chain that
//! caused it rather than recursing until the stack goes, and a depth
//! limit catches a nest that is deep without being circular.
//!
//! Org's `:lines "5-10"` selects part of a file. That is here because
//! including a fragment of something is most of what includes are for
//! — the alternative is a file per fragment.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{IncludeError, resolve_includes};

/// A directory with `files` written into it.
fn dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(d.path().join(name), body).unwrap();
    }
    d
}

#[test]
fn an_include_brings_the_other_file_in() {
    let d = dir(&[("part.org", "the included words\n")]);
    let out = resolve_includes("before\n#+INCLUDE: \"part.org\"\nafter\n", d.path()).unwrap();
    assert!(out.contains("the included words"), "{out}");
    assert!(out.contains("before") && out.contains("after"), "{out}");
}

#[test]
fn the_include_line_itself_is_gone_from_the_view() {
    // A view that shows both the directive and its result shows the
    // reader something the file does not say.
    let d = dir(&[("part.org", "x\n")]);
    let out = resolve_includes("#+INCLUDE: \"part.org\"\n", d.path()).unwrap();
    assert!(!out.contains("#+INCLUDE"), "{out}");
}

#[test]
fn the_file_on_disk_is_not_touched() {
    // I12, again: an expansion is a view and never a write.
    let d = dir(&[
        ("part.org", "x\n"),
        ("main.org", "#+INCLUDE: \"part.org\"\n"),
    ]);
    let before = std::fs::read(d.path().join("main.org")).unwrap();
    let _ = resolve_includes("#+INCLUDE: \"part.org\"\n", d.path()).unwrap();
    assert_eq!(std::fs::read(d.path().join("main.org")).unwrap(), before);
}

#[test]
fn an_include_inside_an_include_resolves() {
    let d = dir(&[
        ("a.org", "from a\n#+INCLUDE: \"b.org\"\n"),
        ("b.org", "from b\n"),
    ]);
    let out = resolve_includes("#+INCLUDE: \"a.org\"\n", d.path()).unwrap();
    assert!(out.contains("from a") && out.contains("from b"), "{out}");
}

#[test]
fn a_cycle_names_the_ring_rather_than_recursing() {
    let d = dir(&[
        ("a.org", "#+INCLUDE: \"b.org\"\n"),
        ("b.org", "#+INCLUDE: \"a.org\"\n"),
    ]);
    let IncludeError::Cycle(path) =
        resolve_includes("#+INCLUDE: \"a.org\"\n", d.path()).expect_err("a cycle")
    else {
        panic!("not reported as a cycle");
    };
    assert!(path.len() >= 2, "{path:?}");
    assert_eq!(path.first(), path.last(), "{path:?} is not a ring");
}

#[test]
fn a_file_that_is_not_there_says_which_one() {
    let d = dir(&[]);
    let err = resolve_includes("#+INCLUDE: \"missing.org\"\n", d.path()).expect_err("missing");
    assert!(
        format!("{err}").contains("missing.org"),
        "the message does not name the file: {err}"
    );
}

#[test]
fn a_line_range_includes_only_those_lines() {
    let d = dir(&[("part.org", "one\ntwo\nthree\nfour\n")]);
    let out = resolve_includes("#+INCLUDE: \"part.org\" :lines \"2-3\"\n", d.path()).unwrap();
    assert!(out.contains("two"), "{out}");
    assert!(!out.contains("one"), "{out}");
    assert!(!out.contains("four"), "{out}");
}

#[test]
fn a_document_with_no_includes_comes_back_unchanged() {
    // The common case, and it must cost nothing and change nothing.
    let d = dir(&[]);
    let src = "* A headline\nbody\n";
    assert_eq!(resolve_includes(src, d.path()).unwrap(), src);
}

#[test]
fn the_result_still_parses_as_org() {
    let d = dir(&[(
        "part.org",
        "** Included\n:PROPERTIES:\n:ID: 01INC0000000000000000AA\n:END:\nbody\n",
    )]);
    let out = resolve_includes("* Top\n#+INCLUDE: \"part.org\"\n", d.path()).unwrap();
    let doc = closure_org::parse(&out).expect("the assembled view parses");
    assert_eq!(doc.roots().len(), 1, "one root with a child under it");
}
