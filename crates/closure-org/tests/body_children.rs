//! A headline typed into a body becomes a child of it.
//!
//! A body line starting with `*` *is* a headline once it is back in the
//! file, so closure escaped it with a comma on the way out — org's own
//! convention, and the reason `,* Foo` shows up in files. Which is
//! correct, and not what anyone means when they type `* Foo` into the
//! body of a note: they mean "this belongs under this".
//!
//! So the typed headlines are lifted out of the body and become real
//! children, rebased to the parent's depth: a single `*` typed under a
//! `***` headline is a `****`, and relative nesting inside what was
//! typed is preserved. Existing children are left where they are —
//! the body editor never showed them, so it must not be able to delete
//! them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{parse, rewrite_body_with_children, split_body_headlines};

const DOC: &str = "\
* Parent
:PROPERTIES:
:ID: 01HQBODY00000000000000001
:END:
Existing body.
** Existing child
:PROPERTIES:
:ID: 01HQBODY00000000000000002
:END:
* Sibling
";

#[test]
fn a_body_with_no_headlines_is_left_alone() {
    let (body, children) = split_body_headlines("just prose\nand more prose");
    assert_eq!(body, "just prose\nand more prose");
    assert!(children.is_empty(), "nothing to lift out");
}

#[test]
fn the_first_headline_line_splits_body_from_children() {
    let (body, children) = split_body_headlines("prose here\n* Typed\nits body\n");
    assert_eq!(body, "prose here");
    assert_eq!(children, "* Typed\nits body\n");
}

#[test]
fn a_body_that_is_only_headlines_leaves_an_empty_body() {
    let (body, children) = split_body_headlines("* One\n* Two\n");
    assert_eq!(body, "");
    assert_eq!(children, "* One\n* Two\n");
}

#[test]
fn stars_inside_a_block_are_not_headlines() {
    // The one case that must not be lifted: a source block full of
    // asterisks is code, not an outline.
    let src = "prose\n#+BEGIN_SRC sh\n* not a headline\n#+END_SRC\nmore prose";
    let (body, children) = split_body_headlines(src);
    assert_eq!(body, src, "{body}");
    assert!(children.is_empty());
}

#[test]
fn bold_at_the_start_of_a_line_is_not_a_headline() {
    let (body, children) = split_body_headlines("*bold* opening\ntext");
    assert!(children.is_empty(), "{children}");
    assert_eq!(body, "*bold* opening\ntext");
}

#[test]
fn typed_headlines_are_rebased_under_the_parent() {
    // The ask, exactly: "inserting a single `*` into the body of a
    // `***` headline should be treated as `****`".
    let doc = parse(DOC).expect("parse");
    let out =
        rewrite_body_with_children(&doc, &[0], "Kept body.\n", "* Typed\n").expect("rewritten");
    let src = out.source();
    assert!(
        src.contains("** Typed"),
        "a `*` typed under a level-1 headline is a level-2: {src}"
    );
}

#[test]
fn relative_nesting_inside_what_was_typed_is_preserved() {
    let doc = parse(DOC).expect("parse");
    let out = rewrite_body_with_children(&doc, &[0], "", "* One\n** Under one\n* Two\n")
        .expect("rewritten");
    let src = out.source();
    assert!(src.contains("** One"), "{src}");
    assert!(src.contains("*** Under one"), "the nesting survives: {src}");
    assert!(src.contains("** Two"), "{src}");
}

#[test]
fn deeper_typed_headlines_rebase_from_the_shallowest_one() {
    // Typing `**` and `***` means the same shape as `*` and `**`.
    let doc = parse(DOC).expect("parse");
    let out =
        rewrite_body_with_children(&doc, &[0], "", "** First\n*** Second\n").expect("rewritten");
    let src = out.source();
    assert!(src.contains("** First"), "{src}");
    assert!(src.contains("*** Second"), "{src}");
}

#[test]
fn existing_children_survive() {
    // The body editor never showed them, so it must not be able to
    // delete them.
    let doc = parse(DOC).expect("parse");
    let out =
        rewrite_body_with_children(&doc, &[0], "Kept body.\n", "* Typed\n").expect("rewritten");
    let src = out.source();
    assert!(src.contains("** Existing child"), "{src}");
    assert!(
        src.contains("01HQBODY00000000000000002"),
        "with its id (I2): {src}"
    );
}

#[test]
fn the_following_sibling_is_untouched() {
    let doc = parse(DOC).expect("parse");
    let out = rewrite_body_with_children(&doc, &[0], "", "* Typed\n").expect("rewritten");
    let src = out.source();
    assert!(src.contains("\n* Sibling"), "still a sibling: {src}");
}

#[test]
fn the_result_is_still_valid_org() {
    let doc = parse(DOC).expect("parse");
    let out = rewrite_body_with_children(&doc, &[0], "Body.\n", "* A\n** B\n").expect("rewritten");
    let reparsed = parse(out.source()).expect("valid org");
    // Parent gained two descendants and kept the one it had.
    let parent = &reparsed.roots()[0];
    assert_eq!(parent.title(), "Parent");
    assert!(
        parent.children().len() >= 2,
        "children: {:?}",
        parent
            .children()
            .iter()
            .map(closure_org::Headline::title)
            .collect::<Vec<_>>()
    );
}
