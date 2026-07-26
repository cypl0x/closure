//! No bound chord may be dead in the terminal shell either.
//!
//! `closure-input` is the single source of truth for keybindings (I4),
//! and the TUI renders its which-key popup and palette straight from
//! it — so every chord it binds is *advertised* to the user. The gpui
//! shell already guards this (`closure-shell-core/tests/no_dead_chords`
//! ); this is the same guard for the terminal.

use std::path::PathBuf;

use closure_config::InputMode;
use closure_tui::App;

fn app() -> App {
    App::new(vec![PathBuf::from("a.org")])
}

/// Every distinct command any mode binds a chord to.
fn every_bound_command() -> std::collections::BTreeSet<&'static str> {
    let mut out = std::collections::BTreeSet::new();
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for (_, command) in closure_input::mode_keymap(mode) {
            out.insert(*command);
        }
    }
    out
}

#[test]
fn every_bound_command_is_implemented() {
    let commands = every_bound_command();
    assert!(commands.len() > 20, "sanity: {} commands", commands.len());
    let mut dead = Vec::new();
    for command in &commands {
        let mut app = app();
        app.apply_command_for_test(command);
        let status = app.status();
        if status.contains("not available") || status.contains("unknown command") {
            dead.push(format!("{command}: {status}"));
        }
    }
    assert!(
        dead.is_empty(),
        "chords are bound to commands the terminal shell does not implement:\n  {}",
        dead.join("\n  ")
    );
}
