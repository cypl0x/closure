//! Q3-V6 — picking tags instead of retyping them.
//!
//! Tags were a free-text field: you typed the whole line, spelling
//! included, and a vault with `:reading:` and `:readng:` in it is a
//! vault whose tag queries quietly miss things.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Alpha :work:urgent:
:PROPERTIES:
:ID: 01HQTAGS000000000000000001
:END:
* Beta :reading:
:PROPERTIES:
:ID: 01HQTAGS000000000000000002
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn v6_the_picker_offers_every_tag_the_vault_has() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQTAGS000000000000000002"));
    app.run(&mut shell, "tag-picker");

    assert_eq!(app.surface(), ModalSurface::TagPick);
    let rows = app.tag_rows(&shell);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"work"), "{names:?}");
    assert!(names.contains(&"urgent"), "{names:?}");
    assert!(names.contains(&"reading"), "{names:?}");
    let reading = rows.iter().find(|r| r.name == "reading").expect("reading");
    assert!(reading.on, "the tag this note already has is ticked");
    let work = rows.iter().find(|r| r.name == "work").expect("work");
    assert!(!work.on);
}

#[test]
fn v6_space_toggles_and_enter_writes() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQTAGS000000000000000002"));
    app.run(&mut shell, "tag-picker");

    // Filter to `work`, tick it, and commit.
    for c in "wor".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, " ", false, false, Some(' '));
    app.on_key(&mut shell, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Browse);
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* Beta :reading:work:"), "{src}");
}

#[test]
fn v6_toggling_off_takes_a_tag_away() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQTAGS000000000000000001"));
    app.run(&mut shell, "tag-picker");
    for c in "urg".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, " ", false, false, Some(' '));
    app.on_key(&mut shell, "enter", false, false, None);

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* Alpha :work:"), "{src}");
    assert!(!src.contains("urgent"), "{src}");
}

#[test]
fn v6_a_tag_the_vault_has_never_seen_can_be_typed() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQTAGS000000000000000002"));
    app.run(&mut shell, "tag-picker");
    for c in "later".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    // Nothing matches, so the typed text is the tag.
    app.on_key(&mut shell, " ", false, false, Some(' '));
    app.on_key(&mut shell, "enter", false, false, None);

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains(":reading:later:"), "{src}");
}

#[test]
fn v6_escape_writes_nothing() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQTAGS000000000000000001"));
    app.run(&mut shell, "tag-picker");
    for c in "read".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, " ", false, false, Some(' '));
    app.on_key(&mut shell, "escape", false, false, None);

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("* Alpha :work:urgent:"), "unchanged: {src}");
}
