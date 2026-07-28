//! A file declares its own TODO keywords (`#+TODO:`).
//!
//! The parser knew exactly two keywords, `TODO` and `DONE`, so a vault
//! configured with `todo_keywords = TODO, NEXT, WAIT, DONE` could be
//! *written* — and read back with `NEXT` as part of the title. Org's own
//! answer is in the file: `#+TODO: TODO NEXT WAIT | DONE` declares the
//! sequence, and Emacs reads the same line.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{parse, print};

const DECLARED: &str = "\
#+TODO: TODO NEXT WAIT | DONE CANCELLED
* NEXT Ship it
:PROPERTIES:
:ID: 01HQKEYWORD000000000000001
:END:
body
* CANCELLED Old plan
body
";

#[test]
fn a_declared_keyword_is_a_keyword() {
    let doc = parse(DECLARED).expect("parse");
    let roots = doc.roots();
    assert_eq!(roots[0].todo(), Some("NEXT"));
    assert_eq!(roots[0].title(), "Ship it");
    assert_eq!(roots[1].todo(), Some("CANCELLED"));
    assert_eq!(roots[1].title(), "Old plan");
}

#[test]
fn declaring_keywords_does_not_move_a_byte() {
    let doc = parse(DECLARED).expect("parse");
    assert_eq!(print(&doc), DECLARED, "I1");
}

#[test]
fn the_declared_list_is_readable() {
    let doc = parse(DECLARED).expect("parse");
    assert_eq!(
        doc.todo_keywords(),
        vec!["TODO", "NEXT", "WAIT", "DONE", "CANCELLED"],
        "the bar is a separator, not a keyword"
    );
}

#[test]
fn a_file_that_declares_nothing_keeps_the_defaults() {
    let doc = parse("* TODO Ship it\n* DONE Shipped\n* NEXT Not a keyword here\n").expect("parse");
    let roots = doc.roots();
    assert_eq!(roots[0].todo(), Some("TODO"));
    assert_eq!(roots[1].todo(), Some("DONE"));
    assert_eq!(
        roots[2].todo(),
        None,
        "undeclared keywords stay part of the title"
    );
    assert_eq!(roots[2].title(), "NEXT Not a keyword here");
    assert_eq!(doc.todo_keywords(), vec!["TODO", "DONE"]);
}

#[test]
fn seq_todo_and_typ_todo_declare_too() {
    let doc = parse("#+SEQ_TODO: TODO STARTED | DONE\n* STARTED Going\n").expect("parse");
    assert_eq!(doc.roots()[0].todo(), Some("STARTED"));
    let doc = parse("#+TYP_TODO: Fred Sara | DONE\n* Sara Owns this\n").expect("parse");
    assert_eq!(doc.roots()[0].todo(), Some("Sara"));
}

#[test]
fn a_keyword_only_counts_at_the_front_of_the_title() {
    let doc = parse("#+TODO: TODO NEXT | DONE\n* Ship the NEXT thing\n").expect("parse");
    assert_eq!(doc.roots()[0].todo(), None);
    assert_eq!(doc.roots()[0].title(), "Ship the NEXT thing");
}
