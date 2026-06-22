//! V9a: `ConflictSet`. Instead of silently letting LWW pick a winner, a
//! 3-way (base/ours/theirs) detector surfaces fields both sides changed
//! divergently, so a shell can offer a real resolution choice.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_crdt::{ConflictField, Replica, conflicts};

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn doc(title: &str, body: &str) -> Document {
    Document::load_str(&format!(
        "* {title}\n:PROPERTIES:\n:ID: {ID}\n:END:\n{body}\n"
    ))
    .expect("parse")
}

fn three(base: &Document, ours: &Document, theirs: &Document) -> (Replica, Replica, Replica) {
    let b = Replica::snapshot(base, 1, "base");
    let o = Replica::snapshot_against(&b, ours, 2, "ours");
    let t = Replica::snapshot_against(&b, theirs, 3, "theirs");
    (b, o, t)
}

#[test]
fn divergent_titles_are_a_title_conflict() {
    let (b, o, t) = three(
        &doc("Base", "body"),
        &doc("Ours", "body"),
        &doc("Theirs", "body"),
    );
    let cs = conflicts(&b, &o, &t);
    assert_eq!(cs.len(), 1, "one conflict: {cs:?}");
    assert_eq!(cs[0].field, ConflictField::Title);
    assert_eq!(cs[0].base.as_deref(), Some("Base"));
    assert_eq!(cs[0].ours, "Ours");
    assert_eq!(cs[0].theirs, "Theirs");
}

#[test]
fn title_vs_body_is_not_a_conflict() {
    // ours changes the title, theirs changes the body — disjoint fields.
    let (b, o, t) = three(
        &doc("Base", "body"),
        &doc("Ours", "body"),
        &doc("Base", "newbody"),
    );
    assert!(
        conflicts(&b, &o, &t).is_empty(),
        "disjoint fields do not conflict"
    );
}

#[test]
fn divergent_bodies_are_a_body_conflict() {
    let (b, o, t) = three(
        &doc("Base", "base body"),
        &doc("Base", "our body"),
        &doc("Base", "their body"),
    );
    let cs = conflicts(&b, &o, &t);
    assert!(
        cs.iter().any(|c| c.field == ConflictField::Body),
        "body conflict: {cs:?}"
    );
}

#[test]
fn identical_edits_do_not_conflict() {
    let (b, o, t) = three(
        &doc("Same", "body"),
        &doc("Same", "body"),
        &doc("Same", "body"),
    );
    assert!(conflicts(&b, &o, &t).is_empty());
}

#[test]
fn one_sided_change_does_not_conflict() {
    // Only ours changes the title; theirs == base → LWW is unambiguous.
    let (b, o, t) = three(
        &doc("Base", "body"),
        &doc("Ours", "body"),
        &doc("Base", "body"),
    );
    assert!(conflicts(&b, &o, &t).is_empty());
}

#[test]
fn detection_is_deterministic() {
    let (b, o, t) = three(&doc("Base", "x"), &doc("Ours", "x"), &doc("Theirs", "x"));
    assert_eq!(conflicts(&b, &o, &t), conflicts(&b, &o, &t));
}
