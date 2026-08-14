//! Every command redoes to the same bytes it produced the first time.
//!
//! `every_command_undoes.rs` proves the reverse arms are exact. This is
//! the other half and the larger uncovered region: `Edit::replay` is
//! 127 lines, one arm per command, and it runs only when somebody
//! presses redo — which no test did.
//!
//! Redo is where an approximate implementation hides best. The forward
//! arms of `Command::apply` are exercised constantly, but `replay` is a
//! *second* forward implementation written from the recorded `Edit`
//! rather than from the command's own arguments, and the two can drift.
//! When they do, undo-then-redo silently produces a document different
//! from the one the user had a moment ago — the worst kind of loss,
//! because it looks like the edit worked.
//!
//! So the assertion is byte equality against the state after the first
//! apply, not merely "something changed".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command, Document};

const ID: &str = "01COREREDO0000000001";
const CHILD: &str = "01COREREDO0000000002";
const SIB: &str = "01COREREDO0000000003";

fn doc() -> Document {
    Document::load_str(&format!(
        // `[#B]` so `set-priority` clearing is a real change rather
        // than a no-op — a headline with no priority reports success
        // and rewrites nothing, which is correct and makes it the
        // wrong subject for a did-it-change assertion.
        "* TODO [#B] Ship it :work:\n\
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

/// Apply, undo, redo — and require the redo to land on exactly what the
/// apply produced.
fn round_trips(name: &str, cmd: &dyn Command) {
    let mut d = doc();
    let before = d.source();
    if Command::apply(cmd, &mut d).is_err() {
        assert_eq!(d.source(), before, "`{name}` failed and changed the file");
        return;
    }
    let applied = d.source();
    // A command may legitimately succeed and change nothing —
    // `ensure-id` on a headline that has one, `set-priority` clearing a
    // priority that is not set, `move-subtree` to where the subtree
    // already is. Those still have to undo and redo without error, so
    // they stay in the sweep; only the did-it-change assertion is
    // skipped for them.
    let changed = applied != before;

    d.undo()
        .unwrap_or_else(|e| panic!("`{name}` will not undo: {e:?}"));
    assert_eq!(d.source(), before, "`{name}` did not undo cleanly");
    if !changed {
        // Nothing to put back, but redo must still be a defined
        // operation rather than an error or a second application.
        d.redo(None)
            .unwrap_or_else(|e| panic!("`{name}` is a no-op that will not redo: {e:?}"));
        assert_eq!(d.source(), before, "`{name}` redid a no-op into a change");
        return;
    }

    d.redo(None)
        .unwrap_or_else(|e| panic!("`{name}` will not redo: {e:?}"));
    assert_eq!(
        d.source(),
        applied,
        "`{name}` redid to something else — `replay` and `apply` disagree"
    );
}

#[test]
fn every_command_redoes_to_the_byte() {
    round_trips(
        "rename",
        &closure_core::RenameHeadline::new(id(ID), "Renamed".to_owned()),
    );
    round_trips(
        "set-todo",
        &closure_core::SetTodo::new(id(ID), Some("DONE".to_owned())),
    );
    round_trips(
        "set-todo-cleared",
        &closure_core::SetTodo::new(id(ID), None),
    );
    round_trips(
        "set-priority",
        &closure_core::SetPriority::new(id(ID), Some('A')),
    );
    round_trips(
        "set-priority-cleared",
        &closure_core::SetPriority::new(id(ID), None),
    );
    round_trips(
        "set-tags",
        &closure_core::SetTags::new(id(ID), vec!["home".to_owned()]),
    );
    round_trips(
        "set-tags-cleared",
        &closure_core::SetTags::new(id(ID), Vec::new()),
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
    round_trips(
        "move-subtree",
        &closure_core::MoveSubtree::new(id(SIB), id(ID)),
    );

    let idless = doc().roots().last().expect("the last root").id().clone();
    round_trips("ensure-id", &closure_core::EnsureId::new(idless));
}

#[test]
fn set_planning_redoes_each_stamp_it_can_set() {
    // `SetPlanning` carries three independent stamps, so one arm being
    // wrong in `replay` is invisible unless each is exercised alone.
    for (name, sched, dead, closed) in [
        ("scheduled", Some("<2026-09-01 Tue>"), None, None),
        ("deadline", None, Some("<2026-09-02 Wed>"), None),
        ("closed", None, None, Some("[2026-09-03 Thu]")),
        (
            "all three",
            Some("<2026-09-01 Tue>"),
            Some("<2026-09-02 Wed>"),
            Some("[2026-09-03 Thu]"),
        ),
    ] {
        round_trips(
            name,
            &closure_core::SetPlanning::new(
                id(ID),
                sched.map(str::to_owned),
                dead.map(str::to_owned),
                closed.map(str::to_owned),
            ),
        );
    }
}

#[test]
fn redo_with_nothing_to_redo_is_refused() {
    // At the tip of the history there is no forward edit. Redo must
    // say so rather than replaying the last one a second time, which
    // would apply it twice.
    let mut d = doc();
    let before = d.source();
    let cmd = closure_core::RenameHeadline::new(id(ID), "Renamed".to_owned());
    Command::apply(&cmd, &mut d).expect("applies");
    let applied = d.source();

    assert!(d.redo(None).is_err(), "redo at the tip reported success");
    assert_eq!(d.source(), applied, "a refused redo changed the file");

    let _ = d.undo();
    assert_eq!(d.source(), before);
}

#[test]
fn undo_and_redo_can_be_walked_back_and_forth() {
    // Not one round trip but several, because state that accumulates —
    // an index rebuilt slightly differently each pass — shows up on the
    // second lap rather than the first.
    let mut d = doc();
    let before = d.source();
    let cmd = closure_core::SetTodo::new(id(ID), Some("DONE".to_owned()));
    Command::apply(&cmd, &mut d).expect("applies");
    let applied = d.source();

    for lap in 0..3 {
        d.undo().unwrap_or_else(|e| panic!("lap {lap} undo: {e:?}"));
        assert_eq!(d.source(), before, "lap {lap}: undo drifted");
        d.redo(None)
            .unwrap_or_else(|e| panic!("lap {lap} redo: {e:?}"));
        assert_eq!(d.source(), applied, "lap {lap}: redo drifted");
    }
}

#[test]
fn a_redo_after_two_edits_replays_them_in_order() {
    // The history is a sequence, not a set. Redoing twice has to put
    // the second edit back after the first, and an implementation that
    // replayed the newest first would produce a document neither state
    // ever was.
    let mut d = doc();
    let rename = closure_core::RenameHeadline::new(id(ID), "First rename".to_owned());
    Command::apply(&rename, &mut d).expect("first");
    let after_one = d.source();
    let todo = closure_core::SetTodo::new(id(ID), Some("DONE".to_owned()));
    Command::apply(&todo, &mut d).expect("second");
    let after_two = d.source();

    d.undo().expect("undo two");
    assert_eq!(d.source(), after_one);
    d.undo().expect("undo one");

    d.redo(None).expect("redo one");
    assert_eq!(d.source(), after_one, "the first edit came back wrong");
    d.redo(None).expect("redo two");
    assert_eq!(d.source(), after_two, "the second edit came back wrong");
}
