//! "command/function for jump to or generate config.org (if not
//! already existent)"
//!
//! Both verbs in one command, because they are the same intention:
//! you want to be looking at your configuration. Whether the file
//! exists yet is closure's problem, not yours.
//!
//! `Config::default_org()` already renders the whole file from the
//! defaults — every key, with the ones that have no default commented
//! out — so generating it costs nothing and the generated file cannot
//! drift from the schema. That is the file that gets written.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQCONF00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQCONF00000000000001"));
    (dir, shell, app)
}

#[test]
fn it_opens_the_config_that_is_already_there() {
    let (dir, mut shell, mut app) = fixture();
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\ntheme = dark\n#+END_SRC\n",
    )
    .expect("write");
    app.run(&mut shell, "open-config");
    assert!(app.surface().is_editor(), "{}", app.status());
    assert!(
        app.body_buffer().contains("theme = dark"),
        "opened something else: {:?}",
        app.body_buffer()
    );
}

#[test]
fn it_writes_one_when_there_is_none() {
    // "(if not already existent)". A vault with no config is the
    // normal starting state, and being told the file does not exist is
    // not what anybody wants from a command called open-config.
    let (dir, mut shell, mut app) = fixture();
    assert!(!dir.path().join("config.org").exists());
    app.run(&mut shell, "open-config");
    assert!(
        dir.path().join("config.org").exists(),
        "nothing was written: {}",
        app.status()
    );
    assert!(app.surface().is_editor(), "{}", app.status());
}

#[test]
fn the_generated_file_is_the_full_default() {
    // Generated from the defaults rather than a hand-written sample,
    // so it cannot drift from the schema and every key is discoverable
    // by reading it.
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "open-config");
    let text = fs::read_to_string(dir.path().join("config.org")).expect("read");
    assert!(text.contains("closure-config"), "{text}");
    assert!(text.contains("input_mode"), "{text}");
    assert!(text.contains("theme"), "{text}");
}

#[test]
fn generating_it_does_not_overwrite_anything() {
    // The one outcome that would be unforgivable: a command that reads
    // "open my config" destroying the config.
    let (dir, mut shell, mut app) = fixture();
    let mine = "#+BEGIN_SRC closure-config\ntheme = light\n#+END_SRC\n";
    fs::write(dir.path().join("config.org"), mine).expect("write");
    app.run(&mut shell, "open-config");
    assert_eq!(
        fs::read_to_string(dir.path().join("config.org")).expect("read"),
        mine
    );
}

#[test]
fn it_says_which_of_the_two_things_it_did() {
    // Writing a file is worth a word; opening one you already had is
    // not a surprise.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "open-config");
    let status = app.status().to_lowercase();
    assert!(
        status.contains("created") || status.contains("wrote") || status.contains("new"),
        "did not say it made one: {status}"
    );
}

#[test]
fn it_is_in_the_palette() {
    assert!(closure_shell_core::palette_command_names().contains(&"open-config"));
}

#[test]
fn every_mode_can_reach_it() {
    // Unlike `build-info`, this is a thing you reach for often enough
    // to want a key: it is where every preference lives.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "open-config").is_some(),
            "{mode:?} cannot open its configuration"
        );
    }
}

#[test]
fn the_buffer_is_the_file_not_a_headline() {
    // config.org has no headlines worth outlining; it is a file you
    // edit whole.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "open-config");
    assert_eq!(app.surface(), ModalSurface::EditFile, "{}", app.status());
}
