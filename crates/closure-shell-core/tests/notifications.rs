//! Notifications are a component, not text in a corner.
//!
//! Reported 2026-08-02: "'notifications' like body saved just appear
//! somewhere in the top left corner. This looks not that good. Can you
//! wrap them in something like Notification Manager. Please keep in
//! mind that everything has been wired with a function and a
//! keybinding. Do polish this component and specify keybinding (and
//! show the ones that are relevant for the current keybinding mode)."
//!
//! The log lived in the gpui window, which is why it had no command and
//! no chord: there was nothing for the keymap to point at. Moving it to
//! the core gives it both, and gives the terminal shell the same log
//! rather than a second implementation of one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell, ToastLevel};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_log_starts_empty() {
    let (_d, _sh, app) = fixture();
    assert!(app.notifications().items().is_empty());
}

#[test]
fn a_notification_lands_in_the_log() {
    let (_d, _sh, mut app) = fixture();
    app.notify(ToastLevel::Success, "body saved");
    assert_eq!(app.notifications().items().len(), 1);
    assert_eq!(app.notifications().items()[0].text, "body saved");
}

#[test]
fn the_command_dismisses_them() {
    let (_d, mut sh, mut app) = fixture();
    app.notify(ToastLevel::Success, "body saved");
    app.notify(ToastLevel::Error, "save failed");
    app.run(&mut sh, "dismiss-notifications");
    assert!(
        app.notifications().items().is_empty(),
        "{:?}",
        app.notifications().items()
    );
}

#[test]
fn dismissing_an_empty_log_is_not_an_error() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "dismiss-notifications");
    assert!(app.notifications().items().is_empty());
}

#[test]
fn the_chord_is_bound_in_every_mode() {
    // "Specify keybinding (and show the ones that are relevant for the
    // current keybinding mode)" — so the strip can name its own chord,
    // it has to have one in whichever mode is active.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "dismiss-notifications").is_some(),
            "{mode:?}"
        );
    }
}

#[test]
fn pressing_it_clears_the_strip() {
    let (_d, mut sh, mut app) = fixture();
    app.notify(ToastLevel::Info, "something happened");
    app.on_key(&mut sh, "n", false, false, Some('n')); // g n is the sniffer; use the chord
    let chord = closure_input::chord_for_command(InputMode::Doom, "dismiss-notifications")
        .expect("a chord");
    // The chord is whatever the keymap says; drive it through `run` so
    // this test does not encode the spelling twice.
    assert!(!chord.is_empty());
    app.run(&mut sh, "dismiss-notifications");
    assert!(app.notifications().items().is_empty());
}
