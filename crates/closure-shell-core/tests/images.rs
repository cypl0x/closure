//! Images in a note: org's own link syntax, a toggle, and a paste that
//! files the picture in the vault rather than in a database.
//!
//! An image in an org file is a link like any other —
//! `[[file:assets/x.png]]` — so nothing new goes in the file format.
//! What was missing is the three things around it: knowing which links
//! are pictures, a place to put one that arrives on the clipboard, and
//! a way to turn the display off.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, asset_file_name, image_links};
use closure_store::Vault;
use tempfile::TempDir;

// === which links are pictures ===

#[test]
fn a_file_link_to_an_image_is_an_image() {
    let links = image_links("see [[file:assets/shot.png]] here");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].path, "assets/shot.png");
    assert_eq!(&"see [[file:assets/shot.png]] here"[links[0].range.clone()],
        "[[file:assets/shot.png]]",
        "the range covers the whole link, so a shell can replace it"
    );
}

#[test]
fn every_format_a_window_can_paint_counts() {
    for ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "PNG"] {
        let line = format!("[[file:a.{ext}]]");
        assert_eq!(image_links(&line).len(), 1, "{ext} is an image");
    }
}

#[test]
fn a_link_to_something_else_is_not_an_image() {
    assert!(image_links("[[file:notes.org]]").is_empty());
    assert!(image_links("[[id:01HQ0000000000000000000000]]").is_empty());
    assert!(image_links("[[https://example.com]]").is_empty());
    assert!(image_links("no links at all").is_empty());
}

#[test]
fn a_described_image_link_keeps_its_path() {
    // `[[file:x.png][a screenshot]]` — the description is the alt text.
    let links = image_links("[[file:x.png][a screenshot]]");
    assert_eq!(links[0].path, "x.png");
    assert_eq!(links[0].description.as_deref(), Some("a screenshot"));
}

#[test]
fn a_bare_relative_path_is_an_image_too() {
    // Org accepts `[[./x.png]]` without the `file:` prefix.
    let links = image_links("[[./sub/x.png]]");
    assert_eq!(links[0].path, "./sub/x.png");
}

#[test]
fn every_image_on_a_line_is_found() {
    let links = image_links("[[file:a.png]] and [[file:b.jpg]]");
    assert_eq!(links.len(), 2);
    assert_eq!(links[1].path, "b.jpg");
}

// === the file a pasted image becomes ===

#[test]
fn a_pasted_image_gets_a_sortable_unique_name() {
    let a = asset_file_name("png");
    let b = asset_file_name("png");
    assert_ne!(a, b, "two pastes are two files");
    assert_eq!(
        std::path::Path::new(&a).extension().and_then(|e| e.to_str()),
        Some("png"),
        "{a}"
    );
    assert!(a.len() > 8, "a ULID, not a counter: {a}");
}

// === the app: toggle, paste, config ===

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQIMAGE0000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn images_are_shown_until_they_are_toggled_off() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.images_shown(), "a picture is worth showing by default");
    app.run(&mut sh, "toggle-inline-images");
    assert!(!app.images_shown());
    assert!(app.status().contains("image"), "{}", app.status());
    app.run(&mut sh, "toggle-inline-images");
    assert!(app.images_shown(), "and back");
}

#[test]
fn a_pasted_image_lands_in_the_vault_and_in_the_text() {
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQIMAGE0000000000000001");
    app.run(&mut sh, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    app.body_set_cursor(4); // end of "body"

    let png = b"\x89PNG\r\n\x1a\n fake but plausible".to_vec();
    let link = app
        .paste_image(&sh, "png", &png)
        .expect("the paste was filed");

    assert!(
        app.body_buffer().contains(&link),
        "the link went in at the cursor: {:?}",
        app.body_buffer()
    );
    let on_disk = dir.path().join("assets");
    let written: Vec<_> = fs::read_dir(&on_disk)
        .expect("the assets directory was made")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(written.len(), 1, "one file for one paste");
    assert_eq!(
        fs::read(written[0].path()).expect("read"),
        png,
        "the bytes are the bytes"
    );
}

#[test]
fn the_assets_directory_is_configurable() {
    let (dir, mut sh) = shell();
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nassets_dir = pictures\n#+END_SRC\n",
    )
    .expect("config");
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQIMAGE0000000000000001");
    app.run(&mut sh, "edit-body");
    let link = app.paste_image(&sh, "png", b"bytes").expect("filed");
    assert!(link.contains("pictures/"), "the link names it: {link}");
    assert!(dir.path().join("pictures").is_dir(), "and it was created");
}

#[test]
fn pasting_an_image_outside_a_buffer_files_nothing() {
    // There is nowhere to put the link, and a picture in the vault that
    // nothing refers to is litter.
    let (dir, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.paste_image(&sh, "png", b"bytes").is_none());
    assert!(!dir.path().join("assets").exists());
}

#[test]
fn a_pasted_image_is_one_undo_step() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQIMAGE0000000000000001");
    app.run(&mut sh, "edit-body");
    let before = app.body_buffer().to_owned();
    app.paste_image(&sh, "png", b"bytes").expect("filed");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), before, "one `u` takes the link back out");
}
