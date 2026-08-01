//! The body of a headline *is* its subtree.
//!
//! Reported 2026-08-01: "sub tree should be visible and editable in the
//! body and editor — the body and the tree view should sync the
//! content", and "items go out of sight real quick". The body editor
//! showed a headline's own prose and nothing else, so a note's children
//! existed only in the tree on the left: you could not read a subtree
//! as a document, and anything you added went out of view the moment it
//! became a headline.
//!
//! Contract revised from `body_children.rs`: existing children used to
//! be untouchable, on the grounds that the editor never showed them and
//! so must not be able to delete them. Now it shows them, so the buffer
//! is the whole truth about what is under a headline — which is what
//! makes "edit it and save" mean the same thing here as it does in the
//! full-window editor.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{children_source, parse, rewrite_subtree_content};

const SRC: &str = "* Parent\nparent prose\n** One\none prose\n*** Deep\n** Two\n* Next\n";

#[test]
fn children_source_is_every_child_verbatim() {
    let doc = parse(SRC).expect("parse");
    let got = children_source(&doc, &[0]).expect("children");
    assert_eq!(got, "** One\none prose\n*** Deep\n** Two\n");
}

#[test]
fn a_headline_with_no_children_has_an_empty_children_source() {
    let doc = parse(SRC).expect("parse");
    assert_eq!(children_source(&doc, &[1]).expect("Next"), "");
}

#[test]
fn the_source_stops_at_the_next_sibling() {
    // `* Next` is not Parent's, and picking it up would move it under
    // Parent the moment the buffer was written back.
    let doc = parse(SRC).expect("parse");
    let got = children_source(&doc, &[0]).expect("children");
    assert!(!got.contains("Next"), "{got}");
}

#[test]
fn writing_back_what_was_read_changes_nothing() {
    // The round trip is the whole feature: if opening a note and saving
    // it without typing rewrites the file, every other guarantee here
    // is worthless.
    let doc = parse(SRC).expect("parse");
    let kids = children_source(&doc, &[0]).expect("children");
    let out = rewrite_subtree_content(&doc, &[0], "parent prose\n", &kids).expect("rewrite");
    assert_eq!(out.source(), SRC);
}

#[test]
fn a_child_removed_from_the_buffer_is_removed_from_the_file() {
    let doc = parse(SRC).expect("parse");
    let out = rewrite_subtree_content(&doc, &[0], "parent prose\n", "** Two\n").expect("rewrite");
    assert_eq!(out.source(), "* Parent\nparent prose\n** Two\n* Next\n");
}

#[test]
fn every_child_removed_leaves_a_childless_headline() {
    let doc = parse(SRC).expect("parse");
    let out = rewrite_subtree_content(&doc, &[0], "parent prose\n", "").expect("rewrite");
    assert_eq!(out.source(), "* Parent\nparent prose\n* Next\n");
}

#[test]
fn a_child_added_in_the_buffer_lands_under_the_headline() {
    let doc = parse(SRC).expect("parse");
    let kids = children_source(&doc, &[0]).expect("children");
    let out = rewrite_subtree_content(&doc, &[0], "parent prose\n", &format!("{kids}** Three\n"))
        .expect("rewrite");
    assert!(
        out.source().contains("** Two\n** Three\n"),
        "{}",
        out.source()
    );
    assert!(out.source().ends_with("* Next\n"), "{}", out.source());
}

#[test]
fn the_body_can_be_emptied_without_taking_the_children() {
    let doc = parse(SRC).expect("parse");
    let kids = children_source(&doc, &[0]).expect("children");
    let out = rewrite_subtree_content(&doc, &[0], "", &kids).expect("rewrite");
    assert_eq!(
        out.source(),
        "* Parent\n** One\none prose\n*** Deep\n** Two\n* Next\n"
    );
}

#[test]
fn a_properties_drawer_on_the_parent_survives() {
    let src = "* Parent\n:PROPERTIES:\n:ID: 01HQSUB0000000000000001\n:END:\nprose\n** Kid\n";
    let doc = parse(src).expect("parse");
    let kids = children_source(&doc, &[0]).expect("children");
    assert_eq!(kids, "** Kid\n");
    let out = rewrite_subtree_content(&doc, &[0], "prose\n", &kids).expect("rewrite");
    assert_eq!(out.source(), src, "the drawer is not part of the body");
}

#[test]
fn the_childrens_own_drawers_come_back_untouched() {
    // Identity is what makes this safe: a child read out with its `:ID:`
    // and written back keeps it, so links, sync and the undo tree still
    // point at the same block.
    let src = "* P\n** Kid\n:PROPERTIES:\n:ID: 01HQSUB0000000000000002\n:END:\nkid prose\n";
    let doc = parse(src).expect("parse");
    let kids = children_source(&doc, &[0]).expect("children");
    assert!(kids.contains("01HQSUB0000000000000002"), "{kids}");
    let out = rewrite_subtree_content(&doc, &[0], "", &kids).expect("rewrite");
    assert_eq!(out.source(), src);
}

#[test]
fn children_without_a_trailing_newline_do_not_swallow_the_next_sibling() {
    // Found in the running shell: typing `** Three` on the last line of
    // the buffer leaves no newline after it, and the region being
    // replaced ends exactly where the next top-level headline begins.
    // Spliced in verbatim it produced `** Three* Next` — one corrupt
    // line, and a headline silently gone from the file.
    let doc = parse(SRC).expect("parse");
    let out = rewrite_subtree_content(&doc, &[0], "parent prose\n", "** Only").expect("rewrite");
    assert_eq!(out.source(), "* Parent\nparent prose\n** Only\n* Next\n");
}

#[test]
fn a_body_without_a_trailing_newline_is_terminated_too() {
    let doc = parse(SRC).expect("parse");
    let out = rewrite_subtree_content(&doc, &[0], "no newline here", "").expect("rewrite");
    assert_eq!(out.source(), "* Parent\nno newline here\n* Next\n");
}
