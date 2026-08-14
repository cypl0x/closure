//! What the undo history says each edit was.
//!
//! `history_view` builds the rows a shell paints in its undo pane, and
//! each row's text comes from `edit_label` — a match with one arm per
//! `Edit` variant, of which two had ever been reached.
//!
//! A wrong label is not a crash and not a data loss; it is worse in one
//! specific way. The undo pane is what somebody reads when they are
//! already lost, trying to work out which edit to go back to. A row
//! that describes the wrong change, or every row reading the same,
//! sends them to the wrong place — and they will not doubt the pane,
//! they will doubt their memory.
//!
//! `default_registry` is here because it is the list of commands every
//! shell offers (I8) and nothing asserted it is complete.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command, Document, default_registry};

const ID: &str = "01COREHIST0000000001";
const CHILD: &str = "01COREHIST0000000002";

fn doc() -> Document {
    Document::load_str(&format!(
        "* TODO [#B] Ship it :work:\n\
         SCHEDULED: <2026-06-20 Sat>\n\
         :PROPERTIES:\n:ID: {ID}\n:END:\n\
         the body\n\
         ** A child\n\
         :PROPERTIES:\n:ID: {CHILD}\n:END:\n"
    ))
    .expect("loads")
}

fn id(s: &str) -> BlockId {
    BlockId::from_existing(s)
}

/// The label of the single edit in a fresh document's history.
fn label_after(cmd: &dyn Command) -> String {
    let mut d = doc();
    Command::apply(cmd, &mut d).expect("applies");
    let rows = d.history_view();
    assert_eq!(rows.len(), 1, "expected one history row, got {rows:?}");
    rows[0].label.clone()
}

#[test]
fn a_rename_says_what_it_was_and_what_it_became() {
    let l = label_after(&closure_core::RenameHeadline::new(
        id(ID),
        "Renamed".to_owned(),
    ));
    assert!(l.contains("Ship it"), "the old title is missing: {l}");
    assert!(l.contains("Renamed"), "the new title is missing: {l}");
}

#[test]
fn a_todo_change_names_both_states_and_marks_an_absent_one() {
    let set = label_after(&closure_core::SetTodo::new(id(ID), Some("DONE".to_owned())));
    assert!(set.contains("TODO"), "{set}");
    assert!(set.contains("DONE"), "{set}");

    // Clearing it: there is no keyword afterwards, and the label has to
    // show that rather than an empty gap somebody reads as truncation.
    let cleared = label_after(&closure_core::SetTodo::new(id(ID), None));
    assert!(cleared.contains("TODO"), "{cleared}");
    assert!(
        cleared.contains('∅'),
        "an absent keyword is not marked: {cleared}"
    );
}

#[test]
fn every_command_produces_a_label_of_its_own() {
    // The failure this guards is every row reading the same. Applying
    // a different command must give a different sentence, or the pane
    // cannot be used to choose between them.
    let cases: Vec<(&str, Box<dyn Command>)> = vec![
        (
            "rename",
            Box::new(closure_core::RenameHeadline::new(id(ID), "R".to_owned())),
        ),
        (
            "set-todo",
            Box::new(closure_core::SetTodo::new(id(ID), Some("DONE".to_owned()))),
        ),
        (
            "set-priority",
            Box::new(closure_core::SetPriority::new(id(ID), Some('A'))),
        ),
        (
            "set-tags",
            Box::new(closure_core::SetTags::new(id(ID), vec!["home".to_owned()])),
        ),
        ("promote", Box::new(closure_core::Promote::new(id(CHILD)))),
        ("demote", Box::new(closure_core::Demote::new(id(CHILD)))),
        (
            "set-body",
            Box::new(closure_core::SetBody::new(id(ID), "new\n".to_owned())),
        ),
        (
            "set-property",
            Box::new(closure_core::SetProperty::new(
                id(ID),
                "CATEGORY".to_owned(),
                "build".to_owned(),
            )),
        ),
        (
            "set-planning",
            Box::new(closure_core::SetPlanning::new(
                id(ID),
                Some("<2026-09-01 Tue>".to_owned()),
                None,
                None,
            )),
        ),
        (
            "toggle-archive",
            Box::new(closure_core::ToggleArchive::new(id(ID))),
        ),
        (
            "toggle-comment",
            Box::new(closure_core::ToggleComment::new(id(ID))),
        ),
        (
            "add-sibling",
            Box::new(closure_core::AddSibling::new(id(ID), "New".to_owned())),
        ),
        (
            "remove-subtree",
            Box::new(closure_core::RemoveSubtree::new(id(CHILD))),
        ),
    ];

    let mut seen: Vec<(String, String)> = Vec::new();
    for (name, cmd) in &cases {
        let l = label_after(cmd.as_ref());
        assert!(!l.trim().is_empty(), "`{name}` produced an empty label");
        if let Some((other, _)) = seen.iter().find(|(_, prev)| prev == &l) {
            panic!("`{name}` and `{other}` produce the same label: {l}");
        }
        seen.push(((*name).to_owned(), l));
    }
    assert_eq!(seen.len(), cases.len());
}

#[test]
fn the_history_rows_carry_the_index_a_click_addresses() {
    // The doc comment on `index` says why it exists: rows come out in
    // walk order, which is not insertion order once the history forks,
    // "so a row that did not carry this would send a click to whatever
    // edit happened to be there".
    let mut d = doc();
    Command::apply(
        &closure_core::RenameHeadline::new(id(ID), "One".to_owned()),
        &mut d,
    )
    .expect("first");
    Command::apply(
        &closure_core::SetTodo::new(id(ID), Some("DONE".to_owned())),
        &mut d,
    )
    .expect("second");

    let rows = d.history_view();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let mut indices: Vec<usize> = rows.iter().map(|r| r.index).collect();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(indices.len(), 2, "two rows share one index: {rows:?}");
}

#[test]
fn an_untouched_document_has_no_history() {
    assert!(doc().history_view().is_empty());
}

#[test]
fn the_default_registry_offers_every_command_this_crate_defines() {
    // I8: a shell offers the kernel's vocabulary. A command missing
    // here is one no shell can reach, however well it works.
    let r = default_registry();
    let names: Vec<&str> = r.names().collect();
    for expected in [
        "rename-headline",
        "ensure-id",
        "set-todo",
        "set-priority",
        "set-tags",
        "set-body",
        "set-planning",
        "toggle-archive",
        "toggle-comment",
        "set-property",
        // Not `promote`/`demote`: the registry names carry the noun,
        // and the struct name is not the command name. Worth pinning,
        // because a rename on either side breaks every shell's keymap
        // and config.org silently.
        "promote-headline",
        "demote-headline",
        "add-sibling",
        "remove-subtree",
        "move-subtree",
    ] {
        assert!(
            names.contains(&expected),
            "`{expected}` is not in the default registry: {names:?}"
        );
    }
}

#[test]
fn no_command_is_registered_twice_under_one_name() {
    // Two commands under one name means one of them is unreachable and
    // which one depends on registration order.
    let r = default_registry();
    let mut names: Vec<&str> = r.names().collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(total, names.len(), "a name is registered twice");
}
