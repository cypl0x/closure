//! `C-c C-c`: running the block your cursor is in.
//!
//! Evaluation was reachable from exactly one place — the Blocks list,
//! where `g x` ran the row under the cursor. Two things were wrong with
//! that. Org's own chord for "run this" was bound to nothing at all,
//! and `eval-block` outside the Blocks list indexed the block list by
//! the *outline's* selection: pressing it on the third headline ran the
//! third source block in the vault, whatever that was.
//!
//! So the command now asks where it was pressed. In a buffer it runs
//! the block the cursor is inside and writes `#+RESULTS:` back into the
//! buffer, which is what `C-c C-c` means in org. Anywhere else it says
//! so rather than running something else.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell, attach_results, code_block_at};
use closure_store::Vault;

const NOTE: &str = "* Note\n\
                    :PROPERTIES:\n\
                    :ID: 01HQEVAL000000000000000001\n\
                    :END:\n\
                    prose above\n\
                    #+BEGIN_SRC sh\n\
                    echo first\n\
                    #+END_SRC\n\
                    prose below\n";

/// A vault whose config trusts `sh`.
fn fixture(note: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = sh\n#+END_SRC\n",
    )
    .expect("config");
    fs::write(dir.path().join("a-notes.org"), note).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open the note's body and park the cursor on `line`.
fn in_the_buffer(app: &mut ModalApp, shell: &mut Shell, line: usize) {
    app.select(0, shell);
    app.run(shell, "edit-body");
    assert!(app.surface().is_editor(), "the buffer is open");
    app.body_goto_line(line);
}

// === finding the block the cursor is in ===

#[test]
fn a_cursor_inside_the_fences_finds_the_block() {
    let text = "prose\n#+BEGIN_SRC sh :var x=1\necho hi\n#+END_SRC\nafter\n";
    let block = code_block_at(text, 2).expect("the cursor is in the block");
    assert_eq!(block.lang, "sh");
    assert_eq!(block.args.trim(), ":var x=1");
    assert_eq!(block.program, "echo hi\n");
    assert_eq!(block.begin, 1, "the #+BEGIN_SRC line");
    assert_eq!(block.end, 3, "the #+END_SRC line");
}

#[test]
fn the_fence_lines_themselves_count_as_inside() {
    // Org runs the block when point is on the `#+BEGIN_SRC` line, and
    // a reader who has just typed `#+END_SRC` is still "in" it.
    let text = "#+BEGIN_SRC sh\necho hi\n#+END_SRC\n";
    assert!(code_block_at(text, 0).is_some());
    assert!(code_block_at(text, 2).is_some());
}

#[test]
fn prose_is_not_a_block() {
    let text = "prose\n#+BEGIN_SRC sh\necho hi\n#+END_SRC\nafter\n";
    assert!(code_block_at(text, 0).is_none());
    assert!(code_block_at(text, 4).is_none());
    assert!(code_block_at("nothing here\n", 0).is_none());
}

#[test]
fn an_unclosed_fence_is_not_a_block() {
    // Half-typed, mid-edit: running it would run a block whose end the
    // writer has not decided on yet.
    assert!(code_block_at("#+BEGIN_SRC sh\necho hi\n", 1).is_none());
}

// === the results go back into the text ===

#[test]
fn results_land_under_the_block_in_org_s_own_shape() {
    let text = "#+BEGIN_SRC sh\necho hi\n#+END_SRC\nafter\n";
    let out = attach_results(text, 2, "hi\n");
    assert!(out.contains("#+END_SRC\n#+RESULTS:\n: hi\n"), "{out}");
    assert!(
        out.ends_with("after\n"),
        "what followed is still there: {out}"
    );
}

#[test]
fn a_second_run_replaces_the_first_result() {
    let text = "#+BEGIN_SRC sh\necho hi\n#+END_SRC\n#+RESULTS:\n: old\nafter\n";
    let out = attach_results(text, 2, "new\n");
    assert_eq!(out.matches("#+RESULTS:").count(), 1, "{out}");
    assert!(out.contains(": new"), "{out}");
    assert!(!out.contains(": old"), "{out}");
    assert!(out.contains("after"), "{out}");
}

#[test]
fn output_with_nothing_in_it_still_says_it_ran() {
    let out = attach_results("#+BEGIN_SRC sh\ntrue\n#+END_SRC\n", 2, "");
    assert!(out.contains("#+RESULTS:\n:\n"), "{out}");
}

// === the command, where it was pressed ===

#[test]
fn the_buffer_runs_the_block_the_cursor_is_in() {
    let (_d, mut shell, mut app) = fixture(NOTE);
    in_the_buffer(&mut app, &mut shell, 2);
    app.run(&mut shell, "execute-block");
    let buf = app.body_buffer();
    assert!(buf.contains("#+RESULTS:"), "{buf}");
    assert!(buf.contains(": first"), "{buf}");
    assert!(app.status().contains("ran"), "{}", app.status());
}

#[test]
fn running_it_twice_leaves_one_result() {
    let (_d, mut shell, mut app) = fixture(NOTE);
    in_the_buffer(&mut app, &mut shell, 2);
    app.run(&mut shell, "execute-block");
    app.run(&mut shell, "execute-block");
    assert_eq!(app.body_buffer().matches("#+RESULTS:").count(), 1);
}

#[test]
fn a_cursor_in_prose_refuses_rather_than_running_something_else() {
    let (_d, mut shell, mut app) = fixture(NOTE);
    in_the_buffer(&mut app, &mut shell, 0);
    let before = app.body_buffer().to_owned();
    app.run(&mut shell, "execute-block");
    assert_eq!(app.body_buffer(), before, "nothing was written");
    assert!(app.status().contains("no source block"), "{}", app.status());
}

#[test]
fn the_buffer_honours_the_trust_gate_too() {
    // The gate is why opening a file is safe; a second route to eval
    // must not be a way around it.
    let note = "* Note\n#+BEGIN_SRC python\nprint('nope')\n#+END_SRC\n";
    let (_d, mut shell, mut app) = fixture(note);
    in_the_buffer(&mut app, &mut shell, 2);
    let before = app.body_buffer().to_owned();
    app.run(&mut shell, "execute-block");
    assert_eq!(app.body_buffer(), before, "nothing ran");
    assert!(
        app.status().contains("trust") || app.status().contains("blocked"),
        "{}",
        app.status()
    );
}

#[test]
fn the_outline_does_not_run_a_block_it_never_showed_you() {
    // The bug: `self.selected` is an *outline* row there, and it was
    // used as an index into the vault-wide block list.
    let (dir, mut shell, mut app) = fixture(NOTE);
    app.select(0, &shell);
    app.run(&mut shell, "execute-block");
    let src = fs::read_to_string(dir.path().join("a-notes.org")).expect("read");
    assert!(!src.contains("#+RESULTS:"), "nothing was run: {src}");
    assert!(
        app.status().contains("no source block"),
        "and it says so: {}",
        app.status()
    );
}
