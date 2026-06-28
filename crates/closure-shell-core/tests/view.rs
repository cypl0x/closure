//! V1a: the declarative, type-level `ViewTree`. `App::view` derives a
//! pure description of the screen that any embedder renders. The
//! invariant under test: every *actionable* node carries its command
//! AND the chord bound to it (the "show keybinding everywhere" rule made
//! type-level — an `Action` cannot exist without a chord).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{Action, App, Mode, Node, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser :work:\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody line\n\
         * Personal wiki\n** Subtopic\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Collect every `Action` reachable in a tree.
fn actions(node: &Node) -> Vec<&Action> {
    let mut out = Vec::new();
    collect(node, &mut out);
    out
}

fn collect<'a>(node: &'a Node, out: &mut Vec<&'a Action>) {
    match node {
        Node::Pane { children, .. } => {
            for c in children {
                collect(c, out);
            }
        }
        Node::Split { panes, .. } => {
            for p in panes {
                collect(p, out);
            }
        }
        Node::Detail { fields } => {
            for f in fields {
                if let Some(a) = &f.action {
                    out.push(a);
                }
            }
        }
        Node::Palette { items, .. } => {
            for i in items {
                out.push(&i.action);
            }
        }
        Node::Rows { .. }
        | Node::Input { .. }
        | Node::Hints { .. }
        | Node::Widget { .. }
        | Node::Text(_) => {}
    }
}

#[test]
fn browse_view_has_rows_detail_and_hints() {
    let (_d, sh) = shell();
    let app = App::new();
    let Node::Pane { children, .. } = app.view(&sh) else {
        panic!("root is a pane");
    };
    let rows = children
        .iter()
        .find_map(|n| match n {
            Node::Rows { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("a rows node");
    assert_eq!(rows.len(), 4, "every headline listed");
    assert!(
        children.iter().any(|n| matches!(n, Node::Detail { .. })),
        "a detail pane"
    );
    assert!(
        children.iter().any(|n| matches!(n, Node::Hints { .. })),
        "an always-on which-key hint line"
    );
}

#[test]
fn detail_title_field_is_actionable_with_a_real_chord() {
    let (_d, sh) = shell();
    let app = App::new();
    let Node::Pane { children, .. } = app.view(&sh) else {
        panic!()
    };
    let fields = children
        .iter()
        .find_map(|n| match n {
            Node::Detail { fields } => Some(fields),
            _ => None,
        })
        .expect("detail");
    let title = fields
        .iter()
        .find(|f| f.label == "title")
        .expect("title field");
    let action = title.action.as_ref().expect("title is click-to-rename");
    assert_eq!(action.command(), "rename");
    assert!(!action.chord().is_empty(), "carries the keybinding");
}

#[test]
fn action_cannot_exist_without_a_chord() {
    // A command no keymap binds yields no Action — the type can only be
    // constructed with a chord (V1 invariant).
    assert!(Action::new(InputMode::Notion, "definitely-not-a-command").is_none());
    // A bound command yields an Action whose chord is non-empty.
    let a = Action::new(InputMode::Notion, "rename").expect("rename is bound");
    assert!(!a.chord().is_empty());
}

#[test]
fn every_actionable_node_in_every_surface_has_a_nonempty_chord() {
    let (_d, mut sh) = shell();
    for build in [
        |a: &mut App, s: &mut Shell| a.on_key(s, "/", false, Some('/')), // palette
        |a: &mut App, _s: &mut Shell| a.begin_capture(),
        |a: &mut App, s: &mut Shell| a.begin_edit_body(s),
    ] {
        let mut app = App::new();
        build(&mut app, &mut sh);
        let tree = app.view(&sh);
        for a in actions(&tree) {
            assert!(
                !a.chord().is_empty(),
                "actionable node `{}` missing its chord",
                a.command()
            );
        }
    }
}

#[test]
fn palette_view_lists_actionable_commands() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    assert_eq!(app.mode(), Mode::Palette);
    let tree = app.view(&sh);
    let acts = actions(&tree);
    assert!(
        acts.iter().any(|a| a.command() == "rename"),
        "palette offers rename"
    );
    assert!(acts.iter().all(|a| !a.chord().is_empty()));
}

#[test]
fn capture_view_shows_the_input_buffer() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.begin_capture();
    app.on_key(&mut sh, "x", false, Some('x'));
    let Node::Pane { children, .. } = app.view(&sh) else {
        panic!()
    };
    let input = children
        .iter()
        .find_map(|n| match n {
            Node::Input { label, buffer } => Some((label, buffer)),
            _ => None,
        })
        .expect("an input node");
    assert_eq!(input.1, "x", "buffer reflects typed text");
}

#[test]
fn view_is_deterministic() {
    let (_d, sh) = shell();
    let app = App::new();
    assert_eq!(app.view(&sh), app.view(&sh));
}
