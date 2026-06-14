//! Modal command-surface state machine (the "modal GUI" experiment):
//! keys resolve against closure-input::mode_keymap, so the five editing
//! modes drive a GUI command surface (vim j/k, g g, emacs C-x C-c),
//! with a search overlay for typing. Proves the modes work outside the
//! TUI; fully headless. Browse never types-to-filter (unlike the
//! launcher App) — that's the whole point.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn vim_jk_navigate_on_the_command_surface() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "j", false, false, Some('j'));
    assert_eq!(app.selected(), 1);
    assert_eq!(app.query(), "", "browse is a command surface, not a filter");
    app.on_key(&mut sh, "k", false, false, Some('k'));
    assert_eq!(app.selected(), 0);
}

#[test]
fn vim_gg_and_capital_g_jump() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "G", false, false, Some('G'));
    assert_eq!(app.selected(), 2, "G -> last");
    app.on_key(&mut sh, "g", false, false, Some('g')); // pending
    app.on_key(&mut sh, "g", false, false, Some('g')); // g g -> first
    assert_eq!(app.selected(), 0);
}

#[test]
fn emacs_ctrl_nav_and_ctrl_x_ctrl_c_quit() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "n", true, false, None); // C-n
    assert_eq!(app.selected(), 1);
    app.on_key(&mut sh, "p", true, false, None); // C-p
    assert_eq!(app.selected(), 0);
    app.on_key(&mut sh, "x", true, false, None); // C-x (pending)
    assert!(!app.should_quit());
    app.on_key(&mut sh, "c", true, false, None); // C-x C-c -> quit
    assert!(app.should_quit());
}

#[test]
fn emacs_meta_less_jumps_first() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "n", true, false, None);
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.selected(), 2);
    app.on_key(&mut sh, "<", false, true, Some('<')); // M-< -> first
    assert_eq!(app.selected(), 0);
}

#[test]
fn browse_ignores_unbound_printable_no_filter() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "z", false, false, Some('z')); // unbound in vim
    assert_eq!(app.query(), "");
    assert_eq!(app.selected(), 0);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn search_overlay_types_filters_and_picks() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/')); // search-start
    assert_eq!(app.surface(), ModalSurface::Search);
    for c in "wiki".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(app.query(), "wiki");
    let rows = app.rows(&sh);
    assert!(rows.iter().any(|r| r.title == "Personal wiki"));
    assert!(!rows.iter().any(|r| r.title == "Ship parser"));
    app.on_key(&mut sh, "enter", false, false, None); // pick -> back to Browse
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.query(), "", "query cleared after picking");
}

#[test]
fn search_escape_cancels_back_to_browse() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/'));
    app.on_key(&mut sh, "x", false, false, Some('x'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.query(), "");
}

#[test]
fn capture_via_command_then_commit() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "c", false, false, Some('c')); // capture-start
    assert_eq!(app.surface(), ModalSurface::Capture);
    for c in "Modal task".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(sh.vault.find_by_title("Modal task").is_some());
}

#[test]
fn cycle_mode_rotates() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    // Doom binds "M" -> cycle-mode (bare, fine on a command surface).
    let start = format!("{:?}", app.input_mode());
    app.on_key(&mut sh, "M", false, false, Some('M'));
    assert_ne!(format!("{:?}", app.input_mode()), start);
}

#[test]
fn key_hints_list_real_bindings_for_the_mode() {
    let app = ModalApp::new(InputMode::Emacs);
    let hints = app.key_hints();
    assert!(hints.contains("C-n"), "emacs hints show C-n: {hints}");
    assert!(hints.contains("quit"));
}
