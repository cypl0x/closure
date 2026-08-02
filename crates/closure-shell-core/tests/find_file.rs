//! "add an option to create a file and/or directory — I do like the
//! =SPC f f= behavior from Doom Emacs. It can create directories or
//! can go interactively into a directory if it already exists. In Doom
//! Emacs =SPC f f= maps to =find-file=" and "how to create new org
//! files? File Management is likely to be required".
//!
//! There was no way to make a file at all. `recent-files` opens one
//! you have had open, the outline shows what the vault already
//! contains, and a vault with one file in it stayed a vault with one
//! file in it unless you left the app.
//!
//! find-file, then, as the picker every other list already is: the
//! directory you are in, its subdirectories first and its org files
//! after, narrowed by what you type. Enter on a directory goes in.
//! Enter on a name that is not there yet makes it — with its parent
//! directories, because "create a file and/or directory" is one
//! gesture in Doom and typing `notes/2026/q3.org` should not need
//! three.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::create_dir_all(dir.path().join("projects")).expect("mkdir");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQFIND00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("projects/apollo.org"),
        "* Apollo\n:PROPERTIES:\n:ID: 01HQFIND00000000000002\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

fn labels(app: &ModalApp, shell: &Shell) -> Vec<String> {
    app.picker_view(shell)
        .expect("a picker")
        .rows
        .into_iter()
        .map(|r| r.label)
        .collect()
}

#[test]
fn it_opens_on_the_vault_root() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    assert_eq!(app.surface(), ModalSurface::FindFile);
    let seen = labels(&app, &shell);
    assert!(seen.iter().any(|l| l.contains("notes.org")), "{seen:?}");
    assert!(seen.iter().any(|l| l.contains("projects")), "{seen:?}");
}

#[test]
fn directories_come_before_files() {
    // You are usually narrowing towards a directory, and a list that
    // mixes them is a list you have to read rather than skim.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    let seen = labels(&app, &shell);
    let dir_at = seen
        .iter()
        .position(|l| l.contains("projects"))
        .expect("dir");
    let file_at = seen
        .iter()
        .position(|l| l.contains("notes.org"))
        .expect("file");
    assert!(dir_at < file_at, "{seen:?}");
}

#[test]
fn entering_a_directory_shows_what_is_in_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "proj");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::FindFile, "still picking");
    let seen = labels(&app, &shell);
    assert!(seen.iter().any(|l| l.contains("apollo.org")), "{seen:?}");
}

#[test]
fn there_is_a_way_back_up() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "proj");
    app.on_key(&mut shell, "enter", false, false, None);
    let seen = labels(&app, &shell);
    assert!(seen.iter().any(|l| l.starts_with("..")), "{seen:?}");
}

#[test]
fn a_name_that_is_not_there_yet_is_created() {
    // The item, exactly: "It can create directories".
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "brand-new.org");
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        dir.path().join("brand-new.org").exists(),
        "the file was not made: {}",
        app.status()
    );
}

#[test]
fn creating_makes_the_directories_it_needs() {
    // One gesture, the way Doom's is: typing a path with directories in
    // it should not need three separate steps.
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "notes/2026/q3.org");
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        dir.path().join("notes/2026/q3.org").exists(),
        "{}",
        app.status()
    );
}

#[test]
fn a_new_file_is_a_note_you_can_write_in() {
    // An empty file is not a note: it has no headline, so the outline
    // has nothing to select and the editor nothing to open.
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "fresh.org");
    app.on_key(&mut shell, "enter", false, false, None);
    let text = fs::read_to_string(dir.path().join("fresh.org")).expect("read");
    assert!(text.starts_with("* "), "no headline: {text:?}");
    assert!(text.contains(":ID:"), "no id: {text:?}");
}

#[test]
fn opening_an_existing_file_opens_it_rather_than_complaining() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "notes.org");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_ne!(app.surface(), ModalSurface::FindFile, "{}", app.status());
    assert!(!app.status().contains("exists"), "{}", app.status());
}

#[test]
fn escape_leaves_without_making_anything() {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    type_in(&mut app, &mut shell, "not-wanted.org");
    app.on_key(&mut shell, "escape", false, false, None);
    assert!(!dir.path().join("not-wanted.org").exists());
    assert_ne!(app.surface(), ModalSurface::FindFile);
}

#[test]
fn it_cannot_be_talked_out_of_the_vault() {
    // A vault is a directory, and a picker that walks above it is a
    // file manager with the user's home in reach.
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "find-file");
    // At the root there is nowhere up to go, so no `..` row offers it.
    let seen = labels(&app, &shell);
    assert!(!seen.iter().any(|l| l.starts_with("..")), "{seen:?}");

    // And asking for one by name is refused rather than turned into a
    // file called `..`, which is what a first version did.
    type_in(&mut app, &mut shell, "../escaped.org");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::FindFile, "{}", app.status());
    assert!(
        !dir.path()
            .parent()
            .is_some_and(|p| p.join("escaped.org").exists()),
        "wrote above the vault"
    );
    assert!(
        app.status().contains("inside the vault"),
        "{}",
        app.status()
    );
}

#[test]
fn every_mode_can_reach_it() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "find-file").is_some(),
            "{mode:?} cannot make a file"
        );
    }
}
