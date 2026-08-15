//! Every command is undoable, and undo puts the bytes back exactly.
//!
//! I3 says every mutation is undoable. The commands each have their own
//! test for what they *do*; what nothing asserted is the invariant
//! across all of them — apply then undo returns the document to the
//! byte it started at.
//!
//! That is the one property a shell relies on without ever checking. A
//! command whose reverse is approximate loses a character somewhere,
//! and the loss shows up three edits later as a file that no longer
//! roundtrips, by which time nobody can say which command did it.
//!
//! Also the largest uncovered region in `closure-core` (262 lines): the
//! reverse arms run far less often than the forward ones.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Document};

const ID: &str = "01COREUNDO0000000001";
const CHILD: &str = "01COREUNDO0000000002";
const SIB: &str = "01COREUNDO0000000003";

fn doc() -> Document {
    Document::load_str(&format!(
        "* TODO Ship it :work:\n\
         SCHEDULED: <2026-06-20 Sat>\n\
         :PROPERTIES:\n:ID: {ID}\n:END:\n\
         the body\n\
         | a | b |\n\
         #+TBLFM: $2=$1\n\
         ** A child\n\
         :PROPERTIES:\n:ID: {CHILD}\n:END:\n\
         * Another :home:\n\
         :PROPERTIES:\n:ID: {SIB}\n:END:\n\
         * With no id of its own\n"
    ))
    .expect("loads")
}

fn id(s: &str) -> BlockId {
    BlockId::from_existing(s)
}

/// Apply `cmd`, then undo it, and require the source to come back
/// byte-for-byte.
fn round_trips(name: &str, cmd: &dyn closure_core::Command) {
    let mut d = doc();
    let before = d.source();
    if closure_core::Command::apply(cmd, &mut d).is_err() {
        // A command that refuses is allowed; a command that
        // half-applies is not, so the document still has to be
        // untouched.
        assert_eq!(d.source(), before, "`{name}` failed and changed the file");
        return;
    }
    assert_ne!(
        d.source(),
        before,
        "`{name}` reported success and changed nothing"
    );
    d.undo()
        .unwrap_or_else(|e| panic!("`{name}` will not undo: {e:?}"));
    assert_eq!(
        d.source(),
        before,
        "`{name}` undid to something else — the reverse is approximate"
    );
}

#[test]
fn every_command_undoes_to_the_byte() {
    round_trips(
        "rename",
        &closure_core::RenameHeadline::new(id(ID), "Renamed".to_owned()),
    );
    round_trips(
        "set-todo",
        &closure_core::SetTodo::new(id(ID), Some("DONE".to_owned())),
    );
    round_trips(
        "set-priority",
        &closure_core::SetPriority::new(id(ID), Some('A')),
    );
    round_trips(
        "set-tags",
        &closure_core::SetTags::new(id(ID), vec!["home".to_owned()]),
    );
    round_trips("promote", &closure_core::Promote::new(id(CHILD)));
    round_trips("demote", &closure_core::Demote::new(id(CHILD)));
    round_trips(
        "set-body",
        &closure_core::SetBody::new(id(ID), "a different body\n".to_owned()),
    );
    round_trips(
        "set-property",
        &closure_core::SetProperty::new(id(ID), "CATEGORY".to_owned(), "build".to_owned()),
    );
    round_trips("toggle-comment", &closure_core::ToggleComment::new(id(ID)));
    round_trips("toggle-archive", &closure_core::ToggleArchive::new(id(ID)));
    round_trips(
        "remove-subtree",
        &closure_core::RemoveSubtree::new(id(CHILD)),
    );
    round_trips(
        "add-sibling",
        &closure_core::AddSibling::new(id(ID), "A new one".to_owned()),
    );
    // Against a headline that *has* no id — on one that already does,
    // `ensure-id` is a no-op that reports success, which is right and
    // makes it the wrong subject for a did-it-change assertion.
    let idless = doc().roots().last().expect("the last root").id().clone();
    round_trips("ensure-id", &closure_core::EnsureId::new(idless));
}

#[test]
fn undoing_twice_is_refused_rather_than_corrupting() {
    // An `Edit` describes one transition. Replaying its reverse against
    // a document already back at the start must not "undo" further.
    let mut d = doc();
    let before = d.source();
    let cmd = closure_core::RenameHeadline::new(id(ID), "Renamed".to_owned());
    closure_core::Command::apply(&cmd, &mut d).expect("applies");
    d.undo().expect("undoes");
    let _ = d.undo();
    assert_eq!(
        d.source(),
        before,
        "undoing an already-undone edit changed the file"
    );
}

#[test]
fn a_command_against_an_id_that_is_not_there_leaves_the_file_alone() {
    let missing = id("01NOSUCH000000000000");
    for (name, cmd) in [
        (
            "rename",
            Box::new(closure_core::RenameHeadline::new(
                missing.clone(),
                "x".to_owned(),
            )) as Box<dyn closure_core::Command>,
        ),
        (
            "set-todo",
            Box::new(closure_core::SetTodo::new(
                missing.clone(),
                Some("DONE".to_owned()),
            )),
        ),
        (
            "promote",
            Box::new(closure_core::Promote::new(missing.clone())),
        ),
        (
            "remove-subtree",
            Box::new(closure_core::RemoveSubtree::new(missing)),
        ),
    ] {
        let mut d = doc();
        let before = d.source();
        let out = closure_core::Command::apply(cmd.as_ref(), &mut d);
        assert!(out.is_err(), "`{name}` succeeded against a missing id");
        assert_eq!(
            d.source(),
            before,
            "`{name}` failed and still changed the file"
        );
    }
}

#[test]
fn undoing_a_property_on_a_headline_that_had_no_drawer_keeps_the_body() {
    // The shape the byte-exact sweep above cannot reach. Its fixture's
    // headline already carries an :ID:, so `set-property` adds a second
    // entry and undo removes one of two — the drawer stays either way.
    //
    // A headline with *no* drawer is the other case: the property
    // creates the drawer, and undo has to take the whole drawer back
    // out. `rewrite_headline_remove_property` was removing one line too
    // many there and deleting the body with it, silently, because the
    // result still parsed.
    let src = "* Bare headline\nthe body that must survive\n";
    let mut d = Document::load_str(src).expect("loads");
    let bare = d.roots().first().expect("a root").id().clone();

    closure_core::Command::apply(
        &closure_core::SetProperty::new(bare, "CATEGORY".to_owned(), "build".to_owned()),
        &mut d,
    )
    .expect("applies");
    assert!(d.source().contains(":CATEGORY:"), "{}", d.source());

    d.undo().expect("undoes");
    let after = d.source();
    assert!(
        after.contains("the body that must survive"),
        "undo ate the body: {after:?}"
    );
    assert!(
        !after.contains(":CATEGORY:"),
        "the property stayed: {after}"
    );

    // Not byte-exact, and that is correct rather than a second bug:
    // addressing a headline gives it an `:ID:`, and an id once issued
    // is never withdrawn (I2). So the drawer remains with the id in it
    // — what must not remain is the property, and what must not vanish
    // is the body.
    assert!(after.contains(":ID:"), "the id was withdrawn: {after}");
}
