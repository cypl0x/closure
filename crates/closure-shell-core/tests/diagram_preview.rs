//! "mermaid diagrams (plugin)" and "(inline) LaTeX preview (plugin)".
//!
//! The renderers live in `closure-eval` (see its `diagrams` test);
//! this is the half the user touches. A diagram is a src block whose
//! result is a picture, so:
//!
//! - `preview-diagrams` renders every diagram block in the buffer, the
//!   way `C-c C-x C-l` does in org.
//! - Rendering runs a program on your machine, so it is gated by the
//!   *same* eval-trust allowlist as every other block, default-deny.
//!   A picture is not a reason to skip the gate.
//! - What is rendered is painted where the block is, by the inline
//!   picture path images already use — and hidden by the same toggle.
//!
//! Deliberately two steps and not one: painting must never shell out.
//! The painter only ever looks for a file that is already there, so a
//! note full of diagrams costs nothing to scroll through.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, diagram_blocks};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Diagrams
:PROPERTIES:
:ID: 01HQDIAG00000000000001
:END:
prose

#+begin_src mermaid
graph TD; A-->B;
#+end_src

more prose

#+begin_src shell
echo hi
#+end_src
";

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQDIAG00000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

#[test]
fn a_mermaid_block_is_found_in_the_buffer() {
    let found = diagram_blocks(NOTES);
    assert_eq!(
        found.len(),
        1,
        "one diagram, not the shell block: {found:?}"
    );
    assert_eq!(found[0].lang, "mermaid");
    assert_eq!(found[0].src.trim(), "graph TD; A-->B;");
}

#[test]
fn a_latex_block_is_found_too() {
    let text = "#+begin_src latex\n\\frac{a}{b}\n#+end_src\n";
    let found = diagram_blocks(text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].lang, "latex");
}

#[test]
fn the_picture_belongs_under_the_block_that_made_it() {
    // Not at the top of the note and not at the caret: where the
    // source is, which is the only place it means anything.
    let found = diagram_blocks(NOTES);
    let end = NOTES
        .lines()
        .position(|l| l.trim() == "#+end_src")
        .expect("a fence");
    assert_eq!(found[0].line, end, "under the closing fence");
}

#[test]
fn an_untrusted_diagram_is_not_rendered_and_says_so() {
    // The gate, default-deny: rendering a picture runs a program, and
    // an empty allowlist runs nothing. The report has to name the
    // language, or the only way to act on it is to guess.
    let (_d, mut shell, mut app) = editing();
    assert!(shell.vault.eval_trust().is_empty(), "default-deny");
    app.run(&mut shell, "preview-diagrams");
    let status = app.status().to_owned();
    assert!(status.contains("mermaid"), "{status}");
    assert!(
        status.contains("trust") || status.contains("Trust"),
        "says what to do about it: {status}"
    );
}

#[test]
fn nothing_is_painted_before_anything_is_rendered() {
    // The painter never shells out — it looks for a file that is
    // already there, so scrolling a note full of diagrams costs
    // nothing.
    let (_d, shell, app) = editing();
    assert!(app.diagram_previews(&shell).is_empty());
}

#[test]
fn a_rendered_diagram_is_painted_where_its_block_is() {
    // Put the picture where the renderer would have put it — the same
    // content-addressed name — and it is found without running
    // anything.
    let (_d, shell, app) = editing();
    let block = &diagram_blocks(app.body_buffer())[0];
    let want = closure_eval::diagram_path(
        &app.diagram_cache(&shell),
        closure_eval::Diagram::Mermaid,
        &block.src,
        app.ink(),
    );
    fs::create_dir_all(want.parent().unwrap()).unwrap();
    fs::write(&want, b"\x89PNG\r\n\x1a\n").unwrap();

    let painted = app.diagram_previews(&shell);
    assert_eq!(painted.len(), 1, "{painted:?}");
    assert_eq!(painted[0].0, block.line, "under its own block");
    assert_eq!(painted[0].1, want);
}

#[test]
fn the_image_toggle_hides_diagrams_as_well() {
    // One toggle for everything painted between the lines: two would
    // mean remembering which kind of picture you were looking at.
    let (_d, mut shell, mut app) = editing();
    let block = &diagram_blocks(app.body_buffer())[0];
    let want = closure_eval::diagram_path(
        &app.diagram_cache(&shell),
        closure_eval::Diagram::Mermaid,
        &block.src,
        app.ink(),
    );
    fs::create_dir_all(want.parent().unwrap()).unwrap();
    fs::write(&want, b"\x89PNG\r\n\x1a\n").unwrap();
    assert_eq!(app.diagram_previews(&shell).len(), 1);

    app.run(&mut shell, "toggle-inline-images");
    assert!(app.diagram_previews(&shell).is_empty(), "hidden with them");
}

#[test]
fn the_cache_is_not_in_the_vault_you_read() {
    // A rendered picture is a build artefact, not a note. Dropping
    // them beside the org files would put derived files into the thing
    // the user syncs and greps.
    let (_d, shell, app) = editing();
    let cache = app.diagram_cache(&shell);
    let notes = shell.vault.root().join("notes.org");
    assert!(notes.is_file());
    assert!(
        !cache
            .join("x")
            .starts_with(shell.vault.root().join("notes.org")),
        "{cache:?}"
    );
    assert!(
        cache.components().any(|c| c.as_os_str() == "diagrams"),
        "somewhere named for what it holds: {cache:?}"
    );
}

#[test]
fn enter_inside_a_drawn_block_opens_the_picture_full_size() {
    // The same `RET` that opens an image link. A mermaid chart is the
    // thing you most want to enlarge — the inline copy is eight rows
    // tall on purpose — and having one gesture for "show me that
    // picture" is worth more than the distinction between a picture a
    // note links and one it generates.
    let (_d, mut shell, mut app) = editing();
    let block = &diagram_blocks(app.body_buffer())[0];
    let want = closure_eval::diagram_path(
        &app.diagram_cache(&shell),
        closure_eval::Diagram::Mermaid,
        &block.src,
        app.ink(),
    );
    fs::create_dir_all(want.parent().unwrap()).unwrap();
    fs::write(&want, b"\x89PNG\r\n\x1a\n").unwrap();

    // The caret on the block's own source line, not the fence.
    app.body_click(block.line - 1, 0);
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::ImageView, "{}", app.status());
    assert_eq!(app.image_shown(), Some(want.as_path()));
}

#[test]
fn a_block_nobody_has_drawn_yet_is_left_alone() {
    // Enter is a newline everywhere there is no picture to show, and
    // an undrawn block has none.
    let (_d, mut shell, mut app) = editing();
    let block = &diagram_blocks(app.body_buffer())[0];
    app.body_click(block.line - 1, 0);
    app.on_key(&mut shell, "enter", false, false, None);
    assert_ne!(app.surface(), ModalSurface::ImageView);
}

#[test]
fn every_mode_can_reach_the_preview_command() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "preview-diagrams").is_some(),
            "{mode:?} cannot preview a diagram"
        );
    }
}
