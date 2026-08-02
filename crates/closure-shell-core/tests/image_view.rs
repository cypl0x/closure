//! "in editor view pressing enter on an image link should show the
//! image in full size" and "editor view inline image toggle
//! (plugin?)".
//!
//! Inline images already paint under the line that links them, and
//! `toggle-inline-images` already hides them — the second item is
//! largely already true. What is missing is the other half: an inline
//! preview is deliberately small, and a picture you actually want to
//! look at needs the window.
//!
//! So `RET` on a line whose link is an image opens it full size, and
//! `Esc` closes it. Every other link keeps doing what it did: an
//! `id:` still jumps, a `https:` still opens outside, and a `file:`
//! pointing at a note still opens the note — an image is the one file
//! this shell can show better than anything it could hand off to.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Pictures
:PROPERTIES:
:ID: 01HQIMG000000000000001
:END:
prose before

[[file:assets/shot.png]]

[[id:01HQIMG000000000000002]]

* Elsewhere
:PROPERTIES:
:ID: 01HQIMG000000000000002
:END:
";

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::create_dir_all(dir.path().join("assets")).expect("mkdir");
    // A one-pixel PNG is a real file with a real header, which is what
    // the "is this an image" question is about.
    fs::write(
        dir.path().join("assets/shot.png"),
        b"\x89PNG\r\n\x1a\n-not-really-decoded-here",
    )
    .expect("write");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQIMG000000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

fn line_of(app: &ModalApp, needle: &str) -> usize {
    app.body_buffer()
        .split('\n')
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in {:?}", app.body_buffer()))
}

#[test]
fn enter_on_an_image_link_opens_it_full_size() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(line_of(&app, "shot.png"), 0);
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::ImageView, "{}", app.status());
    assert!(
        app.image_shown().is_some_and(|p| p.ends_with("shot.png")),
        "{:?}",
        app.image_shown()
    );
}

#[test]
fn escape_closes_it_and_comes_back_to_the_buffer() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(line_of(&app, "shot.png"), 0);
    app.on_key(&mut shell, "enter", false, false, None);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(app.image_shown().is_none());
}

#[test]
fn the_picture_floats_over_the_buffer_it_came_from() {
    // Found on screen, not by a test: the overlay opened correctly and
    // `Esc` came back correctly, but the window painted the *outline*
    // underneath it, because `surface_beneath` had no arm for a picture
    // and fell through to the surface itself. So opening an image from
    // a note made the note appear to close behind it — the same
    // complaint as "everything is shifting and I always get confused",
    // arriving by a new route.
    let (_d, mut shell, mut app) = editing();
    app.body_click(line_of(&app, "shot.png"), 0);
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.surface_beneath(),
        ModalSurface::EditBody,
        "the picture floats over the buffer, not over home"
    );
}

#[test]
fn a_picture_opened_from_the_outline_floats_over_the_outline() {
    // The other half, so nothing starts painting a buffer that was
    // never open.
    let dir = tempfile::tempdir().expect("tmp");
    fs::create_dir_all(dir.path().join("assets")).expect("mkdir");
    fs::write(dir.path().join("assets/shot.png"), b"\x89PNG\r\n\x1a\n").expect("write");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQIMG000000000000001"));
    app.show_image(dir.path().join("assets/shot.png"));
    assert_eq!(app.surface_beneath(), ModalSurface::Browse);
}

#[test]
fn a_link_that_is_not_an_image_is_left_alone() {
    // An `id:` link still jumps; taking Enter for images everywhere
    // would break the one link type the outline is built on.
    let (_d, mut shell, mut app) = editing();
    app.body_click(line_of(&app, "id:01HQIMG000000000000002"), 0);
    app.on_key(&mut shell, "enter", false, false, None);
    assert_ne!(app.surface(), ModalSurface::ImageView, "{}", app.status());
}

#[test]
fn enter_on_ordinary_prose_still_makes_a_newline() {
    // The editor's Enter is a newline first and a link-opener only
    // where there is a link.
    let (_d, mut shell, mut app) = editing();
    app.on_key(&mut shell, "i", false, false, Some('i'));
    let line = line_of(&app, "prose before");
    app.body_click(line, 12);
    let before = app.body_buffer().lines().count();
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.body_buffer().lines().count() > before, "no newline");
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn the_toggle_hides_and_shows_the_inline_previews() {
    // The second item, which was already true: kept as cover.
    let (_d, mut shell, mut app) = editing();
    assert!(app.images_shown(), "shown by default");
    app.run(&mut shell, "toggle-inline-images");
    assert!(!app.images_shown());
    app.run(&mut shell, "toggle-inline-images");
    assert!(app.images_shown());
}

#[test]
fn every_mode_can_reach_the_toggle() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "toggle-inline-images").is_some(),
            "{mode:?} cannot toggle images"
        );
    }
}
