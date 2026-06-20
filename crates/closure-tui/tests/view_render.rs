//! V1b: the TUI renders the shared `closure_shell_core::ViewTree` — the
//! same `Node` tree the web shell renders — to text lines (hermetic, no
//! terminal). Proves one declarative description, many embedders.
//! Actionable nodes surface their chord.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{App, Node, Shell};
use closure_store::Vault;
use closure_tui::render_view;

fn browse_tree() -> (tempfile::TempDir, Node) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n",
    )
    .expect("write");
    let sh = Shell::new(Vault::open(dir.path()).expect("open"));
    let tree = App::new().view(&sh);
    (dir, tree)
}

#[test]
fn renders_rows_to_lines() {
    let (_d, tree) = browse_tree();
    let lines = render_view(&tree);
    assert!(
        lines.iter().any(|l| l.contains("Ship parser")),
        "headline row present: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("Personal wiki")));
}

#[test]
fn actionable_field_line_shows_its_chord() {
    let (_d, tree) = browse_tree();
    let lines = render_view(&tree);
    // The detail "title" field is click-to-rename → its line carries the
    // rename chord in brackets.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("title:") && l.contains('[')),
        "title field line with a chord: {lines:?}"
    );
}

#[test]
fn render_is_deterministic() {
    let (_d, tree) = browse_tree();
    assert_eq!(render_view(&tree), render_view(&tree));
}
