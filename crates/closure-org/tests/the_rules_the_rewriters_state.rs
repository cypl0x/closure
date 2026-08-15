//! The rules these functions document, tested against what they say.
//!
//! Each one here carries a doc comment making a specific promise that
//! is easy to break and hard to notice: a drawer that must disappear
//! with its last entry, a preview that must be exactly the stripped
//! source cut short, a delimiter reader that every shell shares so the
//! parser and the painter cannot disagree.
//!
//! They are grouped because they share a failure mode. None of them
//! throws when it is wrong — they return a slightly different string,
//! and the difference reaches a file or a screen.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{
    BlockDelimiter, block_delimiter_of, children_source, children_source_preview, parse,
    rewrite_headline_remove_property, strip_property_drawers,
};

const SRC: &str = "\
* Parent
:PROPERTIES:
:ID: 01ORGRULES0000000001
:CATEGORY: build
:END:
the parent body
** First child
:PROPERTIES:
:ID: 01ORGRULES0000000002
:END:
first child body
** Second child
:PROPERTIES:
:ID: 01ORGRULES0000000003
:END:
second child body
";

fn doc() -> closure_org::OrgDoc {
    parse(SRC).expect("parses")
}

// === removing a property ===

#[test]
fn removing_one_of_two_properties_leaves_the_drawer() {
    let out = rewrite_headline_remove_property(&doc(), &[0], "CATEGORY").expect("remove");
    let src = out.source();
    assert!(!src.contains(":CATEGORY:"), "{src}");
    assert!(
        src.contains(":PROPERTIES:"),
        "the drawer went with one entry: {src}"
    );
    assert!(src.contains(":ID: 01ORGRULES0000000001"), "{src}");
}

#[test]
fn removing_the_last_property_takes_the_drawer_with_it() {
    // The documented rule, and the reason it matters is undo: "an empty
    // :PROPERTIES: / :END: pair is not what the file looked like before
    // the property existed, and undo has to leave the drawer as it
    // found it (I3)".
    let src = "* Only\n:PROPERTIES:\n:CATEGORY: build\n:END:\nbody\n";
    let d = parse(src).expect("parses");
    let out = rewrite_headline_remove_property(&d, &[0], "CATEGORY").expect("remove");
    let text = out.source();
    assert!(!text.contains(":CATEGORY:"), "{text}");
    assert!(
        !text.contains(":PROPERTIES:") && !text.contains(":END:"),
        "an empty drawer was left behind: {text:?}"
    );
    assert!(text.contains("* Only"), "{text}");
    assert!(
        text.contains("body"),
        "the body was taken with the drawer: {text:?}"
    );
}

#[test]
fn removing_a_key_that_is_not_there_changes_nothing() {
    let d = doc();
    let before = d.source().to_owned();
    let out = rewrite_headline_remove_property(&d, &[0], "NOSUCHKEY").expect("no-op");
    assert_eq!(out.source(), before);
}

#[test]
fn removing_from_a_headline_with_no_drawer_changes_nothing() {
    let d = parse("* Bare\nbody\n").expect("parses");
    let out = rewrite_headline_remove_property(&d, &[0], "CATEGORY").expect("no-op");
    assert_eq!(out.source(), "* Bare\nbody\n");
}

#[test]
fn removing_from_a_path_that_names_no_headline_is_not_found() {
    assert!(rewrite_headline_remove_property(&doc(), &[99], "ID").is_err());
    assert!(rewrite_headline_remove_property(&doc(), &[0, 99], "ID").is_err());
}

#[test]
fn a_property_is_removed_from_the_headline_named_and_no_other() {
    // Both children carry an :ID:. Removing one child's must leave the
    // other's, or the rewriter is matching on the key alone.
    let out = rewrite_headline_remove_property(&doc(), &[0, 0], "ID").expect("remove");
    let src = out.source();
    assert!(!src.contains("01ORGRULES0000000002"), "{src}");
    assert!(
        src.contains("01ORGRULES0000000003"),
        "the other child's id went too: {src}"
    );
    assert!(
        src.contains("01ORGRULES0000000001"),
        "the parent's id went too: {src}"
    );
}

// === previews ===

#[test]
fn the_preview_is_the_stripped_source_cut_to_length() {
    // The documented equivalence: "same answer as
    // strip_property_drawers(&children_source(..)) cut to max_lines,
    // without building the two whole strings on the way". Two paths to
    // one answer is the shape that lets them drift, and the fast one is
    // the one nobody reads.
    let d = doc();
    let full = children_source(&d, &[0]).expect("children");
    let stripped = strip_property_drawers(&full);
    for max in [0usize, 1, 2, 3, 5, 50] {
        let mut want = String::new();
        for l in stripped.lines().take(max) {
            want.push_str(l);
            want.push('\n');
        }
        let got = children_source_preview(&d, &[0], max).expect("preview");
        assert_eq!(got, want, "preview at max_lines={max}");
    }
}

#[test]
fn the_preview_carries_no_property_drawer() {
    let got = children_source_preview(&doc(), &[0], 100).expect("preview");
    assert!(!got.contains(":PROPERTIES:"), "{got}");
    assert!(!got.contains(":ID:"), "{got}");
    assert!(got.contains("First child"), "{got}");
    assert!(got.contains("first child body"), "{got}");
}

#[test]
fn a_headline_with_no_children_previews_as_nothing() {
    let got = children_source_preview(&doc(), &[0, 0], 10).expect("preview");
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn a_preview_of_a_path_that_is_not_there_is_none() {
    assert!(children_source_preview(&doc(), &[99], 10).is_none());
}

#[test]
fn stripping_drawers_leaves_everything_else_alone() {
    let src = "text before\n:PROPERTIES:\n:ID: x\n:END:\ntext after\n";
    assert_eq!(strip_property_drawers(src), "text before\ntext after\n");
}

#[test]
fn stripping_a_drawer_that_never_ends_does_not_eat_the_rest_of_the_file() {
    // A truncated drawer is what a half-written file looks like.
    // Swallowing everything after it would make a preview of a broken
    // file look like an empty one.
    let src = "before\n:PROPERTIES:\n:ID: x\n";
    let out = strip_property_drawers(src);
    assert!(out.starts_with("before"), "{out:?}");
}

#[test]
fn a_source_with_no_drawers_comes_back_unchanged() {
    for src in ["", "just text\n", "* A headline\nbody\n"] {
        assert_eq!(strip_property_drawers(src), src);
    }
}

// === block delimiters ===

#[test]
fn a_begin_line_is_read_with_its_name_and_arguments() {
    match block_delimiter_of("#+BEGIN_SRC sh :results output") {
        Some(BlockDelimiter::Begin { name, args }) => {
            assert_eq!(name, "SRC");
            assert_eq!(args, Some("sh :results output"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_begin_with_no_arguments_has_none_rather_than_an_empty_string() {
    match block_delimiter_of("#+BEGIN_QUOTE") {
        Some(BlockDelimiter::Begin { name, args }) => {
            assert_eq!(name, "QUOTE");
            assert_eq!(args, None, "no arguments is None, not Some(\"\")");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_end_line_names_the_block_it_closes() {
    match block_delimiter_of("#+END_SRC") {
        Some(BlockDelimiter::End { name }) => assert_eq!(name, "SRC"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn either_case_and_any_indent_is_still_a_delimiter() {
    // Stated in the doc: "org allows the delimiters to be indented and
    // spells them in either case". A shell that missed the indented
    // form would paint the inside of an indented block as prose.
    for line in [
        "#+begin_src sh",
        "#+BEGIN_SRC sh",
        "#+Begin_Src sh",
        "    #+begin_src sh",
        "\t#+BEGIN_SRC sh",
    ] {
        assert!(
            matches!(block_delimiter_of(line), Some(BlockDelimiter::Begin { .. })),
            "{line:?} was not read as a begin"
        );
    }
    for line in ["#+end_src", "  #+END_SRC", "\t#+End_Src"] {
        assert!(
            matches!(block_delimiter_of(line), Some(BlockDelimiter::End { .. })),
            "{line:?} was not read as an end"
        );
    }
}

#[test]
fn a_keyword_line_is_not_a_block_delimiter() {
    // The doc calls this out: "a `#+KEYWORD:` line is not a block".
    // Reading `#+TITLE:` as a block opener would swallow the file.
    for line in [
        "#+TITLE: Notes",
        "#+PROPERTY: header-args :tangle no",
        "#+NAME: hello",
        "#+RESULTS:",
        "* A headline",
        "plain text",
        "",
        "#+",
        "#+beginning of the end",
    ] {
        assert!(
            block_delimiter_of(line).is_none(),
            "{line:?} was read as a delimiter"
        );
    }
}
