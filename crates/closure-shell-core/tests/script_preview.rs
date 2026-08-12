//! `x^2` reads as `x²` in the pane.
//!
//! Sixth construct through the preview seam (I12): the editor shows the
//! file, the preview shows what the file means. A subscript is markup
//! whose whole purpose is to be *seen* — leaving it as three characters
//! in the one place a reader reads is the same as not having parsed it.
//!
//! Unicode has real sub- and superscript glyphs for digits and a few
//! letters and not for everything, and where it has none the source
//! stays. That is the same rule the entity preview follows: a wrong
//! rendering is worse than an honest un-rendered one, because the
//! reader cannot tell the first from the truth.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Maths
:PROPERTIES:
:ID: 01SCRIPTPREV000000001
:END:
a^2 + b^2 = c^2 and H_{2}O
* Code
:PROPERTIES:
:ID: 01SCRIPTPREV000000002
:END:
let some_value = other_value;
* Unrenderable
:PROPERTIES:
:ID: 01SCRIPTPREV000000003
:END:
x^{qq}
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn body_of(app: &mut ModalApp, shell: &Shell, title: &str) -> String {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail").body.clone()
}

#[test]
fn a_superscript_reads_as_one() {
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Maths");
    assert!(body.contains('²'), "{body}");
    assert!(!body.contains("a^2"), "{body}");
}

#[test]
fn a_subscript_reads_as_one() {
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Maths");
    assert!(body.contains("H₂O"), "{body}");
}

#[test]
fn snake_case_is_left_alone() {
    // The case the whole feature lives or dies on, asserted where a
    // reader would actually notice it.
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Code");
    assert!(body.contains("some_value"), "{body}");
    assert!(body.contains("other_value"), "{body}");
}

#[test]
fn something_unicode_cannot_render_keeps_its_source() {
    // No superscript `q` exists. Dropping it, or rendering half of it,
    // would be a quieter kind of wrong than showing what was written.
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Unrenderable");
    assert!(body.contains("x^{qq}"), "{body}");
}

#[test]
fn the_file_is_not_rewritten() {
    let (dir, shell, mut app) = app(VAULT);
    let _ = body_of(&mut app, &shell, "Maths");
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(on_disk.contains("a^2 + b^2 = c^2"), "{on_disk}");
    assert!(!on_disk.contains('²'), "the preview reached the file");
}

#[test]
fn maths_keeps_its_own_carets() {
    // `$x^2$` is a LaTeX fragment: something to render whole, not a
    // string to substitute inside. The first version of this pass
    // turned it into `$x²$`, which is neither the source nor maths —
    // caught by `entity_preview`'s shipped assertion that maths is left
    // as written, and stated here so the interaction is a rule rather
    // than a thing that happens to hold.
    let src = "\
* Formula
:PROPERTIES:
:ID: 01SCRIPTPREV000000004
:END:
inline a^2 but maths stays: $x^2$
";
    let (_d, shell, mut app) = app(src);
    let body = body_of(&mut app, &shell, "Formula");
    assert!(body.contains("$x^2$"), "maths was rendered into: {body}");
    assert!(body.contains('²'), "the prose superscript was not: {body}");
}
