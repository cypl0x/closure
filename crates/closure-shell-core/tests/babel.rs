//! Running a source block from the GUI.
//!
//! org-babel is one of the vision's load-bearing features, and the
//! kernel has had it for a while (`Vault::eval_block`, the trust gate,
//! the language backends). The GUI could only ever *list* blocks: the
//! Blocks surface was read-only, so the reference shell was the one
//! place you could see a code block and not run it.
//!
//! The execution path itself is the kernel's, untouched — including
//! the `eval_trust` gate, which is the reason an org file cannot run
//! arbitrary code just by being opened. What is new is that the GUI can
//! reach it, and that the output comes back somewhere a shell can paint.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

/// A vault whose config trusts `sh`, with two blocks in one file.
///
/// The notes file is named to sort *before* `config.org`, so rows 0
/// and 1 are the two `sh` blocks and the vault's own `closure-config`
/// block — which is a source block like any other — is row 2.
fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = sh\n#+END_SRC\n",
    )
    .expect("config");
    fs::write(
        dir.path().join("a-notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQBABEL00000000000001\n:END:\n\
         #+BEGIN_SRC sh\necho first\n#+END_SRC\n\
         #+BEGIN_SRC sh\necho second\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_blocks_surface_lists_every_block_in_path_order() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    assert_eq!(app.surface(), ModalSurface::Blocks);
    let rows = app.block_rows(&shell);
    let langs: Vec<&str> = rows.iter().map(|(_, lang, _)| lang.as_str()).collect();
    // `sh` is listed under its canonical name, `shell`.
    assert_eq!(langs, vec!["shell", "shell", "closure-config"], "{rows:?}");
}

#[test]
fn running_the_selected_block_captures_its_output() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    assert_eq!(app.block_output(), None, "nothing run yet");
    app.run(&mut shell, "eval-block");
    let out = app.block_output().expect("output");
    assert!(out.contains("first"), "ran block 0: {out:?}");
    assert!(!out.contains("second"), "and only block 0: {out:?}");
}

#[test]
fn the_cursor_chooses_which_block_runs() {
    // Walked with `down` rather than a bare `j` since 2026-08-02: the
    // user asked for the list commands to behave like the palette, so
    // letters narrow the list and the arrows (and `C-n`/`C-j`) walk it.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.on_key(&mut shell, "down", false, false, None);
    app.run(&mut shell, "eval-block");
    let out = app.block_output().expect("output");
    assert!(out.contains("second"), "ran block 1: {out:?}");
}

#[test]
fn an_untrusted_language_is_refused_not_run() {
    // The trust gate is the whole reason opening a file is safe. The
    // GUI must surface the refusal, never bypass it.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = sh\n#+END_SRC\n",
    )
    .expect("config");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n#+BEGIN_SRC python\nprint('nope')\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut shell, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "eval-block");
    assert_eq!(app.block_output(), None, "nothing ran");
    assert!(
        app.status().contains("blocked") || app.status().contains("trust"),
        "the refusal must be visible: {}",
        app.status()
    );
}

#[test]
fn running_with_no_blocks_says_so() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Just prose\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut shell, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    app.run(&mut shell, "eval-block");
    assert_eq!(app.block_output(), None);
    assert!(!app.status().is_empty(), "silence would look like a hang");
}

#[test]
fn leaving_the_surface_clears_the_previous_output() {
    // Stale output next to a different block is a lie about what ran.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "eval-block");
    assert!(app.block_output().is_some());
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.block_output(), None, "output does not outlive the pane");
}

#[test]
fn moving_the_cursor_clears_the_previous_output() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "eval-block");
    assert!(app.block_output().is_some());
    app.on_key(&mut shell, "down", false, false, None);
    assert_eq!(
        app.block_output(),
        None,
        "the output belonged to the block we just left"
    );
}
