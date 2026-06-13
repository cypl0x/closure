//! Command-registry and Edit log tests.
//!
//! Cross-refs to spec.md invariants (Quality):
//! - I3: every mutation goes through a `Command` and produces an `Edit`
//!   that the undo-tree can replay in reverse. Property-tested apply+undo identity.
//! - I4: every command carries a keybinding entry in the registry (no hand tables;
//!   which-key / doc / modes read from it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::{
    AddSibling, Demote, Document, EnsureId, KeyChord, MoveSubtree, Promote, Registry,
    RemoveSubtree, RenameHeadline, SetBody, SetPriority, SetTags, SetTodo,
};

#[test]
fn registry_stores_command_by_name() {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    assert!(r.get("rename-headline").is_some());
    assert_eq!(r.get("rename-headline").unwrap().name(), "rename-headline");
}

#[test]
fn every_command_has_keybinding_entry() {
    let cmd = RenameHeadline::new_placeholder();
    // I4: keys() must return at least one chord (may be a placeholder
    // until a user reassigns).
    assert!(!closure_core::Command::keys(&cmd).is_empty());
}

#[test]
fn rename_headline_changes_title_and_returns_edit() {
    let mut doc = Document::load_str("* Old title\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = RenameHeadline::new(id.clone(), "New title".into());
    let edit = closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(
        doc.headline_by_id(&id).expect("lookup").title(),
        "New title"
    );
    // Edit carries enough info to reverse the change.
    assert!(edit.is_reversible());
}

#[test]
fn applied_edit_appears_in_history() {
    let mut doc = Document::load_str("* X\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = RenameHeadline::new(id, "Y".into());
    let _ = closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.history_len(), 1);
}

#[test]
fn undo_reverses_rename() {
    let mut doc = Document::load_str("* Original\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = RenameHeadline::new(id.clone(), "Changed".into());
    let _ = closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.headline_by_id(&id).expect("lookup").title(), "Changed");
    doc.undo().expect("undo");
    assert_eq!(doc.headline_by_id(&id).expect("lookup").title(), "Original");
    // UndoTree retains the node; history_len stays 1 because the
    // edit can still be redone.
    assert_eq!(doc.history_len(), 1);
}

#[test]
fn redo_after_undo_replays_edit() {
    let mut doc = Document::load_str("* Original\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = RenameHeadline::new(id.clone(), "Changed".into());
    let _ = closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    doc.undo().expect("undo");
    doc.redo(None).expect("redo");
    assert_eq!(doc.headline_by_id(&id).expect("lookup").title(), "Changed");
}

#[test]
fn keychord_from_str_parses_c_c() {
    let k: KeyChord = "C-c C-x".parse().expect("parse");
    assert_eq!(k.to_string(), "C-c C-x");
}

#[test]
fn ensure_id_writes_id_to_property_drawer() {
    let mut doc = Document::load_str("* Hello\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = EnsureId::new(id.clone());
    let _ = closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    let src = doc.source();
    assert!(src.contains(":PROPERTIES:"));
    assert!(src.contains(&format!(":ID: {id}")));
    assert!(src.contains(":END:"));
}

#[test]
fn ensure_id_is_noop_when_id_already_present() {
    let pinned = "01HXQZ7F0000000000000000AA";
    let src = format!("* H\n:PROPERTIES:\n:ID: {pinned}\n:END:\n");
    let mut doc = Document::load_str(&src).expect("load");
    let id = doc.roots()[0].id().clone();
    assert_eq!(id.as_str(), pinned);
    let cmd = EnsureId::new(id);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.source(), src);
}

#[test]
fn set_todo_adds_keyword() {
    let mut doc = Document::load_str("* Fix bug\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetTodo::new(id.clone(), Some("TODO".into()));
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(
        doc.headline_by_id(&id).expect("lookup").todo(),
        Some("TODO")
    );
    assert!(doc.source().starts_with("* TODO Fix bug"));
}

#[test]
fn set_todo_clears_keyword() {
    let mut doc = Document::load_str("* TODO Fix bug\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetTodo::new(id.clone(), None);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert!(doc.headline_by_id(&id).expect("lookup").todo().is_none());
    assert!(doc.source().starts_with("* Fix bug"));
}

#[test]
fn set_todo_undo_restores_keyword() {
    let mut doc = Document::load_str("* TODO Fix\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetTodo::new(id.clone(), Some("DONE".into()));
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(
        doc.headline_by_id(&id).expect("lookup").todo(),
        Some("DONE")
    );
    doc.undo().expect("undo");
    assert_eq!(
        doc.headline_by_id(&id).expect("lookup").todo(),
        Some("TODO")
    );
}

#[test]
fn set_priority_adds_cookie() {
    let mut doc = Document::load_str("* Urgent task\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetPriority::new(id.clone(), Some('A'));
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.headline_by_id(&id).expect("h").priority(), Some('A'));
    assert!(doc.source().starts_with("* [#A] Urgent task"));
}

#[test]
fn set_priority_with_existing_todo() {
    let mut doc = Document::load_str("* TODO Urgent\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetPriority::new(id.clone(), Some('B'));
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.headline_by_id(&id).expect("h").priority(), Some('B'));
    assert!(doc.source().starts_with("* TODO [#B] Urgent"));
}

#[test]
fn set_priority_undo_clears_cookie() {
    let mut doc = Document::load_str("* X\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetPriority::new(id.clone(), Some('A'));
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    doc.undo().expect("undo");
    assert!(doc.headline_by_id(&id).expect("h").priority().is_none());
}

#[test]
fn set_tags_replaces_block() {
    let mut doc = Document::load_str("* Hello :old:\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetTags::new(id.clone(), vec!["work".into(), "urgent".into()]);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    let h = doc.headline_by_id(&id).expect("h");
    assert_eq!(h.tags(), &["work".to_owned(), "urgent".to_owned()]);
    assert!(doc.source().contains(":work:urgent:"));
}

#[test]
fn set_tags_clears_block() {
    let mut doc = Document::load_str("* Hello :old:\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetTags::new(id.clone(), Vec::new());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert!(doc.headline_by_id(&id).expect("h").tags().is_empty());
}

#[test]
fn demote_then_promote_round_trips() {
    let mut doc = Document::load_str("* Hello\n").expect("load");
    let id = doc.roots()[0].id().clone();
    closure_core::Command::apply(&Demote::new(id.clone()), &mut doc).expect("demote");
    assert_eq!(doc.headline_by_id(&id).expect("h").level(), 2);
    closure_core::Command::apply(&Promote::new(id.clone()), &mut doc).expect("promote");
    assert_eq!(doc.headline_by_id(&id).expect("h").level(), 1);
}

#[test]
fn promote_at_level_one_is_error() {
    let mut doc = Document::load_str("* Hello\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let err = closure_core::Command::apply(&Promote::new(id), &mut doc).unwrap_err();
    let _ = err;
}

#[test]
fn demote_undo_returns_to_original_level() {
    let mut doc = Document::load_str("* Hello\n").expect("load");
    let id = doc.roots()[0].id().clone();
    closure_core::Command::apply(&Demote::new(id.clone()), &mut doc).expect("demote");
    assert_eq!(doc.headline_by_id(&id).expect("h").level(), 2);
    doc.undo().expect("undo");
    assert_eq!(doc.headline_by_id(&id).expect("h").level(), 1);
}

#[test]
fn add_sibling_inserts_new_headline_with_id() {
    let mut doc = Document::load_str("* First\n").expect("load");
    let first_id = doc.roots()[0].id().clone();
    let cmd = AddSibling::new(first_id.clone(), "Second".into());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.roots().len(), 2);
    assert_eq!(doc.roots()[1].title(), "Second");
    assert_ne!(doc.roots()[1].id(), &first_id);
}

#[test]
fn add_sibling_undo_removes_inserted_headline() {
    let mut doc = Document::load_str("* First\n").expect("load");
    let first_id = doc.roots()[0].id().clone();
    let cmd = AddSibling::new(first_id, "Second".into());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.roots().len(), 2);
    doc.undo().expect("undo");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].title(), "First");
}

#[test]
fn add_sibling_redo_reinserts_with_same_id() {
    let mut doc = Document::load_str("* First\n").expect("load");
    let first_id = doc.roots()[0].id().clone();
    let cmd = AddSibling::new(first_id, "Second".into());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    let new_id = doc.roots()[1].id().clone();
    doc.undo().expect("undo");
    doc.redo(None).expect("redo");
    assert_eq!(doc.roots()[1].id(), &new_id);
    assert_eq!(doc.roots()[1].title(), "Second");
}

#[test]
fn remove_subtree_drops_headline_and_descendants() {
    let mut doc = Document::load_str("* A\n** A.1\n** A.2\n* B\n").expect("load");
    let a_id = doc.roots()[0].id().clone();
    let cmd = RemoveSubtree::new(a_id);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].title(), "B");
}

#[test]
fn remove_subtree_undo_restores_headline() {
    let mut doc = Document::load_str("* A\n** A.1\n* B\n").expect("load");
    let a_id = doc.roots()[0].id().clone();
    let cmd = RemoveSubtree::new(a_id);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert_eq!(doc.roots().len(), 1);
    doc.undo().expect("undo");
    assert_eq!(doc.roots().len(), 2);
    let titles: Vec<&str> = doc.roots().iter().map(|h| h.title()).collect();
    assert!(titles.contains(&"A"));
    assert!(titles.contains(&"B"));
}

#[test]
fn move_subtree_relocates_headline() {
    let mut doc = Document::load_str("* A\n* B\n* C\n").expect("load");
    let a_id = doc.roots()[0].id().clone();
    let c_id = doc.roots()[2].id().clone();
    let cmd = MoveSubtree::new(a_id.clone(), c_id);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    let titles: Vec<&str> = doc.roots().iter().map(|h| h.title()).collect();
    assert_eq!(titles, vec!["B", "C", "A"]);
    assert!(doc.headline_by_id(&a_id).is_some());
}

#[test]
fn set_body_replaces_paragraph() {
    let mut doc = Document::load_str("* H\nold body\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetBody::new(id, "new body\n".into());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert!(doc.source().contains("new body"));
    assert!(!doc.source().contains("old body"));
}

#[test]
fn set_body_undo_restores_old_body() {
    let mut doc = Document::load_str("* H\nfirst version\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let cmd = SetBody::new(id, "second version\n".into());
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    assert!(doc.source().contains("second version"));
    doc.undo().expect("undo");
    assert!(doc.source().contains("first version"));
    assert!(!doc.source().contains("second version"));
}

#[test]
fn move_subtree_undo_restores_position() {
    let mut doc = Document::load_str("* A\n* B\n* C\n").expect("load");
    let b_id = doc.roots()[1].id().clone();
    let c_id = doc.roots()[2].id().clone();
    let cmd = MoveSubtree::new(b_id, c_id);
    closure_core::Command::apply(&cmd, &mut doc).expect("apply");
    let titles_after: Vec<&str> = doc.roots().iter().map(|h| h.title()).collect();
    assert_eq!(titles_after, vec!["A", "C", "B"]);
    doc.undo().expect("undo");
    let titles_back: Vec<&str> = doc.roots().iter().map(|h| h.title()).collect();
    assert_eq!(titles_back, vec!["A", "B", "C"]);
}
