//! "eval-block src —
//!   `#+BEGIN_SRC` shell
//!   pwd
//!   `#+END_SRC`
//!   #+RESULTS:
//!   : /home/wap/dev/closure
//!  If this go straight to terminal I am not sure that's how Emacs does
//!  it. The example above do work in Emacs."
//!
//! The results machinery was all there: both eval paths attach
//! `#+RESULTS:` span-preserving, exactly as org does. What was missing
//! is that `C-c C-c` never reached them. When that chord became "save
//! and close" for the body editor, it took the key unconditionally —
//! so pressing it on a source block saved the buffer and said "body
//! saved".
//!
//! org's `C-c C-c` is context-sensitive, which the commit that made
//! the change said in as many words and then did not do: on a source
//! block it is `org-babel-execute-src-block`, and everywhere else in a
//! buffer it is the accept. The block wins where there is a block.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Run me
:PROPERTIES:
:ID: 01HQBABEL00000000000001
:END:
prose before

#+BEGIN_SRC shell
echo hello-from-babel
#+END_SRC

prose after
";

/// A vault that trusts shell blocks, and an open body editor.
fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    fs::write(
        dir.path().join("config.org"),
        "#+TITLE: t\n\n#+BEGIN_SRC closure-config\neval_trust = shell\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQBABEL00000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

/// `C-c C-c`, one stroke at a time.
fn accept(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "c", true, false, None);
    app.on_key(shell, "c", true, false, None);
}

/// The line of the buffer holding `needle`.
fn line_of(app: &ModalApp, needle: &str) -> usize {
    app.body_buffer()
        .split('\n')
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in {:?}", app.body_buffer()))
}

#[test]
fn c_c_c_c_on_a_source_block_runs_it() {
    let (_d, mut shell, mut app) = editing();
    let line = line_of(&app, "echo hello-from-babel");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);

    assert!(
        app.body_buffer().contains("hello-from-babel") && app.body_buffer().contains("#+RESULTS:"),
        "no results attached: {:?}",
        app.body_buffer()
    );
}

#[test]
fn running_a_block_keeps_the_buffer_open() {
    // org runs the block and leaves you where you were; it does not
    // close the buffer, which is what "save and close" would do.
    let (_d, mut shell, mut app) = editing();
    let line = line_of(&app, "echo hello-from-babel");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);
    assert_eq!(app.surface(), ModalSurface::EditBody, "{}", app.status());
}

#[test]
fn c_c_c_c_off_a_block_still_saves_and_closes() {
    // The other half of context-sensitive: everywhere that is not a
    // source block, the chord is still the accept.
    let (_d, mut shell, mut app) = editing();
    let line = line_of(&app, "prose after");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);
    assert_eq!(app.surface(), ModalSurface::Browse, "{}", app.status());
}

#[test]
fn a_block_the_vault_does_not_trust_says_how_to_trust_it() {
    // The companion item — "tutorial how to add something into the
    // eval trust" — is somebody who could not work out the remedy from
    // the refusal. Naming the concept is not naming the fix.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQBABEL00000000000001"));
    app.run(&mut shell, "edit-body");
    let line = line_of(&app, "echo hello-from-babel");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);

    let msg = app.status().to_owned();
    assert!(msg.contains("eval_trust"), "{msg}");
    assert!(msg.contains("shell"), "the language it wants: {msg}");
    assert!(msg.contains("config.org"), "and where to put it: {msg}");
}

#[test]
fn the_results_replace_the_previous_run_rather_than_stacking() {
    // org replaces a `#+RESULTS:` block it already wrote; two runs
    // that appended would grow the file every time.
    let (_d, mut shell, mut app) = editing();
    let line = line_of(&app, "echo hello-from-babel");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);
    let line = line_of(&app, "echo hello-from-babel");
    app.body_click(line, 0);
    accept(&mut app, &mut shell);

    let n = app.body_buffer().matches("#+RESULTS:").count();
    assert_eq!(n, 1, "{} results blocks: {:?}", n, app.body_buffer());
}
