//! org-edit-special: editing a source block on its own.
//!
//! `C-c '` in Emacs opens the block under point in a buffer of its own
//! language, and writes it back in place on exit. The GUI could list
//! blocks and run them, but the only way to *edit* one was to edit the
//! surrounding body as org text — with the fences and the indentation
//! in the way.
//!
//! Two entry points, and they write back differently, because they
//! have different things to preserve:
//!
//!  * from the Blocks list, the block belongs to a file, and the
//!    writeback goes through the kernel's span-preserving
//!    `set_block_content` (I1: the rest of the file is untouched
//!    bytes);
//!  * from the body editor, the block lives inside the buffer being
//!    edited, so it is spliced back into that buffer and the ordinary
//!    body commit carries it to disk. Going through the file path
//!    there would mean writing the body twice, from two states.
//!
//! Finding the block under the cursor is pure, and that is what most
//! of this pins.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, enclosing_src_block};
use closure_store::Vault;

// === the pure seam: which block is the cursor in? ===

const BODY: &str = "intro\n\
                    #+BEGIN_SRC sh\n\
                    echo one\n\
                    echo two\n\
                    #+END_SRC\n\
                    between\n\
                    #+BEGIN_SRC python\n\
                    print(1)\n\
                    #+END_SRC\n\
                    tail\n";

#[test]
fn a_cursor_in_prose_is_in_no_block() {
    assert_eq!(enclosing_src_block(BODY, 0), None, "first line");
    let between = BODY.find("between").expect("between");
    assert_eq!(enclosing_src_block(BODY, between), None);
    let tail = BODY.find("tail").expect("tail");
    assert_eq!(enclosing_src_block(BODY, tail), None);
}

#[test]
fn a_cursor_inside_a_block_finds_its_content_and_language() {
    let at = BODY.find("echo two").expect("echo two");
    let (range, lang) = enclosing_src_block(BODY, at).expect("in the sh block");
    assert_eq!(lang, "sh");
    assert_eq!(
        &BODY[range], "echo one\necho two\n",
        "the content, without the fences"
    );
}

#[test]
fn the_second_block_is_found_too() {
    let at = BODY.find("print(1)").expect("print");
    let (range, lang) = enclosing_src_block(BODY, at).expect("in the python block");
    assert_eq!(lang, "python");
    assert_eq!(&BODY[range], "print(1)\n");
}

#[test]
fn the_fence_lines_themselves_count_as_inside() {
    // Point on `#+BEGIN_SRC` is how you usually reach for `C-c '`.
    let begin = BODY.find("#+BEGIN_SRC sh").expect("begin");
    let (range, lang) = enclosing_src_block(BODY, begin).expect("on the opener");
    assert_eq!(lang, "sh");
    assert_eq!(&BODY[range], "echo one\necho two\n");
    let end = BODY.find("#+END_SRC").expect("end");
    assert!(enclosing_src_block(BODY, end).is_some(), "on the closer");
}

#[test]
fn an_empty_block_has_an_empty_range() {
    let text = "#+BEGIN_SRC sh\n#+END_SRC\n";
    let (range, lang) = enclosing_src_block(text, 0).expect("found");
    assert_eq!(lang, "sh");
    assert!(range.is_empty(), "nothing between the fences: {range:?}");
}

#[test]
fn a_block_with_no_language_still_resolves() {
    let text = "#+BEGIN_SRC\nbody\n#+END_SRC\n";
    let (range, lang) = enclosing_src_block(text, 15).expect("found");
    assert_eq!(lang, "", "no language is empty, not a guess");
    assert_eq!(&text[range], "body\n");
}

#[test]
fn an_unterminated_block_is_not_a_block() {
    // A half-typed fence must not swallow the rest of the file.
    let text = "#+BEGIN_SRC sh\necho hi\n";
    assert_eq!(enclosing_src_block(text, 18), None);
}

#[test]
fn fences_are_matched_case_insensitively() {
    let text = "#+begin_src sh\nbody\n#+end_src\n";
    let (range, lang) = enclosing_src_block(text, 16).expect("lowercase fences are org too");
    assert_eq!(lang, "sh");
    assert_eq!(&text[range], "body\n");
}

#[test]
fn an_offset_past_the_end_is_clamped_not_panicking() {
    assert_eq!(enclosing_src_block(BODY, usize::MAX), None);
}

// === the surface ===

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n:PROPERTIES:\n:ID: 01HQSPECIAL000000000001\n:END:\n\
         prose before\n\
         #+BEGIN_SRC sh\necho hi\n#+END_SRC\n\
         prose after\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn edit_special_from_the_blocks_list_opens_the_block_alone() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "edit-special");
    assert_eq!(app.surface(), ModalSurface::EditBlock);
    assert_eq!(
        app.body_buffer(),
        "echo hi\n",
        "the block's content, no fences"
    );
    assert_eq!(
        app.special_language(),
        "shell",
        "the normalised name, whichever door you came through"
    );
}

#[test]
fn committing_writes_the_block_back_and_leaves_the_file_alone() {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "edit-special");
    // The block buffer opens in NORMAL (Doom opens buffers that way),
    // so typing starts with vim's own insert.
    app.on_key(&mut shell, "i", false, false, Some('i'));
    for c in "echo edited".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", true, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::Blocks,
        "back where we came from"
    );

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("echo edited"), "the edit landed: {src}");
    assert!(src.contains("prose before"), "surroundings intact: {src}");
    assert!(src.contains("prose after"), "surroundings intact: {src}");
    assert!(src.contains("#+BEGIN_SRC sh"), "fences intact: {src}");
    // And the whole file still round-trips as org (I1).
    let doc = closure_core::Document::load_str(&src).expect("parses");
    assert_eq!(doc.source(), src, "byte-exact");
}

#[test]
fn escaping_edit_special_discards_the_edit() {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    app.run(&mut shell, "edit-special");
    // The block buffer opens in NORMAL (Doom opens buffers that way),
    // so typing starts with vim's own insert.
    app.on_key(&mut shell, "i", false, false, Some('i'));
    for c in "garbage".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    // Esc leaves INSERT, Esc again abandons the session.
    app.on_key(&mut shell, "escape", false, false, None);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Blocks);
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("echo hi"), "original kept: {src}");
    assert!(!src.contains("garbage"), "edit discarded: {src}");
}

#[test]
fn edit_special_from_the_body_editor_splices_back_into_the_buffer() {
    // Here the block lives in the buffer being edited, so the write
    // goes back into that buffer — the body commit carries it to disk.
    let (dir, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    // Put the cursor inside the block.
    let at = app.body_buffer().find("echo hi").expect("block content");
    let line = app.body_buffer()[..at].matches('\n').count();
    app.body_click(line, 2);
    app.run(&mut shell, "edit-special");
    assert_eq!(app.surface(), ModalSurface::EditBlock);
    assert_eq!(app.body_buffer(), "echo hi\n");

    // The block buffer opens in NORMAL (Doom opens buffers that way),
    // so typing starts with vim's own insert.
    app.on_key(&mut shell, "i", false, false, Some('i'));
    for c in "echo spliced".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", true, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "back in the body");
    let body = app.body_buffer();
    assert!(body.contains("echo spliced"), "spliced: {body:?}");
    assert!(body.contains("#+BEGIN_SRC sh"), "fences kept: {body:?}");
    assert!(body.contains("prose before"), "prose kept: {body:?}");

    // Nothing on disk until the body itself is committed.
    let before = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!before.contains("echo spliced"), "not written yet");
    app.on_key(&mut shell, "enter", true, false, None);
    let after = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(after.contains("echo spliced"), "written on body commit");
}

#[test]
fn edit_special_outside_a_block_says_so() {
    let (_d, mut shell, mut app) = fixture();
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    app.body_click(0, 0); // "prose before"
    app.run(&mut shell, "edit-special");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "no block under the cursor, so no session"
    );
    assert!(
        app.status().contains("block"),
        "and it says why: {}",
        app.status()
    );
}

#[test]
fn edit_special_from_browse_says_so_too() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-special");
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!app.status().is_empty(), "silence would look like a hang");
}
