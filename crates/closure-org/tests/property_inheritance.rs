//! `#+PROPERTY: key value` — a default for every headline in the file.
//!
//! Org's way of saying "unless a headline says otherwise". The usual
//! case is `#+PROPERTY: header-args :tangle no` at the top of a
//! literate file: every block inherits it, and the one block that
//! wants tangling says so in its own drawer.
//!
//! Preserved and never read, so the line was decoration: a file that
//! set a default got no default, and every headline that relied on one
//! behaved as though the line were absent.
//!
//! Reading, not rewriting (I1/I12). The default is not copied into
//! anybody's drawer — a lookup that falls through is a view, and
//! writing `:HEADER-ARGS:` onto forty headlines because one line said
//! so would turn opening a file into a commit.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{document_properties, parse};

const DOC: &str = "\
#+TITLE: A literate file
#+PROPERTY: header-args :tangle no
#+PROPERTY: category reading

* Block that inherits
:PROPERTIES:
:ID: 01PROPINHERIT00000001
:END:
* Block that overrides
:PROPERTIES:
:ID: 01PROPINHERIT00000002
:HEADER-ARGS: :tangle yes
:END:
";

#[test]
fn a_file_wide_default_is_read() {
    let props = document_properties(DOC);
    assert_eq!(
        props.get("HEADER-ARGS").map(String::as_str),
        Some(":tangle no")
    );
}

#[test]
fn several_defaults_are_all_read() {
    let props = document_properties(DOC);
    assert_eq!(props.len(), 2, "{props:?}");
    assert_eq!(props.get("CATEGORY").map(String::as_str), Some("reading"));
}

#[test]
fn the_key_is_normalised_the_way_a_drawer_writes_it() {
    // `#+PROPERTY: header-args` and `:HEADER-ARGS:` are the same
    // property. Org writes drawer keys upper-case and the keyword
    // lower-case, so a lookup that did not agree on one of them would
    // find the default only for keys somebody happened to type twice
    // the same way.
    let props = document_properties("#+PROPERTY: Mixed-Case value\n");
    assert!(props.contains_key("MIXED-CASE"), "{props:?}");
}

#[test]
fn a_document_with_no_defaults_has_none() {
    assert!(document_properties("* Just a headline\nbody\n").is_empty());
}

#[test]
fn a_property_line_inside_a_block_is_not_a_default() {
    // The same rule `#+STARTUP:` follows: a directive quoted inside an
    // example block is an example of a directive.
    let src = "#+BEGIN_EXAMPLE\n#+PROPERTY: header-args :tangle yes\n#+END_EXAMPLE\n";
    assert!(
        document_properties(src).is_empty(),
        "read a quoted directive"
    );
}

#[test]
fn a_value_with_spaces_survives_whole() {
    let props = document_properties("#+PROPERTY: header-args :tangle no :results silent\n");
    assert_eq!(
        props.get("HEADER-ARGS").map(String::as_str),
        Some(":tangle no :results silent")
    );
}

#[test]
fn the_source_is_not_rewritten() {
    // I1: reading a default must not put it in anybody's drawer.
    let doc = parse(DOC).expect("parses");
    let _ = document_properties(DOC);
    assert_eq!(doc.source(), DOC);
    assert!(
        !doc.source().contains(":CATEGORY:"),
        "a default reached a drawer"
    );
}
