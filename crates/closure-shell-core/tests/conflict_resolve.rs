//! V9b: interactive conflict resolution. `ConflictApp` lists the 3-way
//! conflicts (V9a) and lets the user pick ours/theirs per field; the
//! choice applies through the vault command path (undoable, I3/I8) and
//! persists. Rendered as a `ViewTree` with the resolve keybindings.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_config::InputMode;
use closure_core::{BlockId, Document};
use closure_crdt::{Replica, conflicts};
use closure_shell_core::{ConflictApp, Node, Shell};
use closure_store::Vault;

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn setup() -> (tempfile::TempDir, Shell, ConflictApp) {
    let dir = tempfile::tempdir().expect("tmp");
    // The vault holds the base document (the live file to resolve).
    let base_src = format!("* Base\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n");
    fs::write(dir.path().join("n.org"), &base_src).expect("write");
    let shell = Shell::new(Vault::open(dir.path()).expect("open"));

    let base = Document::load_str(&base_src).expect("base");
    let ours = Document::load_str(&format!("* Ours\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"))
        .expect("ours");
    let theirs = Document::load_str(&format!("* Theirs\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"))
        .expect("theirs");
    let rb = Replica::snapshot(&base, 1, "base");
    let ro = Replica::snapshot_against(&rb, &ours, 2, "ours");
    let rt = Replica::snapshot_against(&rb, &theirs, 3, "theirs");
    let cs = conflicts(&rb, &ro, &rt);
    let app = ConflictApp::new(cs, InputMode::Notion);
    (dir, shell, app)
}

#[test]
fn lists_the_detected_conflicts() {
    let (_d, _s, app) = setup();
    assert_eq!(app.conflicts().len(), 1, "one title conflict");
}

#[test]
fn resolve_ours_applies_through_the_vault_and_persists() {
    let (_d, mut shell, mut app) = setup();
    app.select(0);
    app.resolve_ours(&mut shell).expect("resolve");
    // The headline now holds our value, via the command path (undoable).
    let (h, _p) = shell
        .vault
        .find_by_id(&BlockId::from_existing(ID))
        .expect("resolves");
    assert_eq!(h.title(), "Ours");
    // The resolved conflict is removed from the list.
    assert!(app.conflicts().is_empty());
}

#[test]
fn resolve_theirs_picks_the_other_side() {
    let (_d, mut shell, mut app) = setup();
    app.select(0);
    app.resolve_theirs(&mut shell).expect("resolve");
    let (h, _p) = shell
        .vault
        .find_by_id(&BlockId::from_existing(ID))
        .expect("resolves");
    assert_eq!(h.title(), "Theirs");
}

#[test]
fn view_offers_resolve_actions_with_chords() {
    let (_d, _s, app) = setup();
    let Node::Pane { children, .. } = app.view() else {
        panic!("pane")
    };
    let detail = children
        .iter()
        .find_map(|n| match n {
            Node::Detail { fields } => Some(fields),
            _ => None,
        })
        .expect("detail");
    let ours = detail
        .iter()
        .find(|f| {
            f.action
                .as_ref()
                .is_some_and(|a| a.command() == "resolve-ours")
        })
        .expect("resolve-ours action");
    assert!(!ours.action.as_ref().unwrap().chord().is_empty());
    assert!(detail.iter().any(|f| {
        f.action
            .as_ref()
            .is_some_and(|a| a.command() == "resolve-theirs")
    }));
}
