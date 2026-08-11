//! Inline tasks: `*************** TODO something`.
//!
//! Org's inline task is a headline that is deliberately *not* part of
//! the outline. Fifteen stars is more than any real document nests, and
//! that is the point — it marks a task attached to the paragraph it
//! sits in rather than a section of the document, and org's own
//! folding, exporting and agenda treat it as an aside.
//!
//! closure read it as an ordinary headline of level fifteen. So a note
//! with one in it had a section fifteen deep in the outline, the
//! headline count was wrong, and anything walking the tree walked into
//! a branch that is not one.
//!
//! Recognising it is the whole of this: it stops being outline
//! structure and starts being what it is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{parse, print};

const WITH_INLINE: &str = "\
* A real section
:PROPERTIES:
:ID: 01INLINE00000000000001A
:END:
some prose
*************** TODO ring the plumber
*************** END
more prose
* Another real section
:PROPERTIES:
:ID: 01INLINE00000000000002A
:END:
";

#[test]
fn an_inline_task_is_not_an_outline_headline() {
    let doc = parse(WITH_INLINE).expect("parse");
    assert_eq!(
        doc.roots().len(),
        2,
        "the inline task became part of the outline"
    );
    assert!(
        doc.roots()[0].children().is_empty(),
        "the inline task became a child: {:?}",
        doc.roots()[0].children().len()
    );
}

#[test]
fn it_does_not_change_the_headline_count() {
    let doc = parse(WITH_INLINE).expect("parse");
    let n = doc.iter_headlines().len();
    assert_eq!(n, 2, "counted the inline task as a headline");
}

#[test]
fn the_file_still_roundtrips_byte_exact() {
    // I1 first: whatever this changes about the tree, the bytes are
    // the bytes.
    let doc = parse(WITH_INLINE).expect("parse");
    assert_eq!(print(&doc), WITH_INLINE);
}

#[test]
fn it_can_still_be_found_as_an_inline_task() {
    // Not part of the outline is not the same as invisible: an agenda
    // wants it, which is why anybody writes one.
    let doc = parse(WITH_INLINE).expect("parse");
    let tasks = closure_org::inline_tasks(&doc);
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    assert_eq!(tasks[0].todo.as_deref(), Some("TODO"));
    assert_eq!(tasks[0].title, "ring the plumber");
}

#[test]
fn a_task_without_an_end_line_is_still_recognised() {
    // Org allows the short form for a one-line task.
    let src = "* S\n:PROPERTIES:\n:ID: 01INLINE00000000000003A\n:END:\n\
               *************** TODO short form\nprose after\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(closure_org::inline_tasks(&doc).len(), 1);
    assert_eq!(print(&doc), src, "I1");
}

#[test]
fn fourteen_stars_is_still_an_ordinary_headline() {
    // The boundary matters: org's threshold is fifteen, and a parser
    // that guesses turns deep outlines into asides.
    let src = format!(
        "{} Deep but real\n:PROPERTIES:\n:ID: 01INLINE00000000000004A\n:END:\n",
        "*".repeat(14)
    );
    let doc = parse(&src).expect("parse");
    assert_eq!(doc.iter_headlines().len(), 1);
    assert!(closure_org::inline_tasks(&doc).is_empty());
}

#[test]
fn a_document_with_none_is_unaffected() {
    let src = "* One\n:PROPERTIES:\n:ID: 01INLINE00000000000005A\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.iter_headlines().len(), 1);
    assert!(closure_org::inline_tasks(&doc).is_empty());
    assert_eq!(print(&doc), src);
}
