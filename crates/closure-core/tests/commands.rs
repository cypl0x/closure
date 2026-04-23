//! Command-registry and Edit log tests.
//!
//! I3: every mutation goes through a `Command` and produces an `Edit`
//! that the undo-tree can replay in reverse.
//! I4: every command carries a keybinding entry in the registry.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::{Document, KeyChord, Registry, RenameHeadline};

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
    assert_eq!(doc.history_len(), 0);
}

#[test]
fn keychord_from_str_parses_c_c() {
    let k: KeyChord = "C-c C-x".parse().expect("parse");
    assert_eq!(k.to_string(), "C-c C-x");
}
