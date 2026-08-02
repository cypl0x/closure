//! "mermaid diagrams (plugin)" and "(inline) LaTeX preview (plugin)".
//!
//! Two items, one shape: a block of text that is worth *looking* at
//! rather than reading. org has had this for LaTeX since forever
//! (`org-latex-preview`, `C-c C-x C-l`) and mermaid arrives through
//! babel the same way — a block runs an external program and org shows
//! you the picture that comes back.
//!
//! So neither is a new subsystem here. A diagram is a src block whose
//! output happens to be an image, which means it goes through the
//! eval-trust allowlist like every other block that runs a program,
//! and it is painted by the inline-picture path that already exists.
//!
//! What is new is that rendering is *expensive* and repeatable: the
//! same source must never render twice, so the file is named for the
//! hash of what produced it. An edit renders once; reopening the note
//! renders not at all.
//!
//! closure does not ship `mmdc` or `latex`, and org does not ship
//! `latex` either — you point it at what you have. The one thing that
//! must never happen is silence: a missing tool says so.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{Diagram, RenderError, diagram_for, diagram_path, render_diagram};

/// The dark theme's foreground — the ink every fixture here draws in.
const INK: u32 = 0x00cd_d6f4;

#[test]
fn a_mermaid_block_is_a_diagram() {
    assert_eq!(diagram_for("mermaid"), Some(Diagram::Mermaid));
}

#[test]
fn a_latex_block_is_one_too() {
    // org calls the language `latex`; `tex` is what people type.
    assert_eq!(diagram_for("latex"), Some(Diagram::Latex));
    assert_eq!(diagram_for("tex"), Some(Diagram::Latex));
}

#[test]
fn an_ordinary_src_block_is_not() {
    // A shell block still produces text, and nothing about this may
    // change that: `#+RESULTS:` is the contract for those.
    assert_eq!(diagram_for("shell"), None);
    assert_eq!(diagram_for("python"), None);
}

#[test]
fn the_same_source_always_names_the_same_file() {
    // The whole reason rendering is affordable: a note full of
    // diagrams renders once, ever, and reopening it renders nothing.
    let dir = tempfile::tempdir().unwrap();
    let a = diagram_path(dir.path(), Diagram::Mermaid, "graph TD; A-->B;", INK);
    let b = diagram_path(dir.path(), Diagram::Mermaid, "graph TD; A-->B;", INK);
    assert_eq!(a, b);
}

#[test]
fn editing_the_source_names_a_different_one() {
    let dir = tempfile::tempdir().unwrap();
    let a = diagram_path(dir.path(), Diagram::Mermaid, "graph TD; A-->B;", INK);
    let b = diagram_path(dir.path(), Diagram::Mermaid, "graph TD; A-->C;", INK);
    assert_ne!(a, b, "an edit has to render again");
}

#[test]
fn two_languages_never_collide_on_one_file() {
    // Same bytes, different renderer, different picture.
    let dir = tempfile::tempdir().unwrap();
    let a = diagram_path(dir.path(), Diagram::Mermaid, "x", INK);
    let b = diagram_path(dir.path(), Diagram::Latex, "x", INK);
    assert_ne!(a, b);
}

#[test]
fn a_rendered_file_is_a_png_under_the_cache_directory() {
    let dir = tempfile::tempdir().unwrap();
    let p = diagram_path(dir.path(), Diagram::Mermaid, "graph TD; A-->B;", INK);
    assert!(p.starts_with(dir.path()), "{p:?}");
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
}

#[test]
fn a_missing_tool_says_which_one_and_how_to_get_it() {
    // The failure that must never be silent. closure does not ship
    // these programs any more than org ships `latex`, so "nothing
    // happened" is the one unacceptable answer.
    let dir = tempfile::tempdir().unwrap();
    let err = render_diagram(
        Diagram::Mermaid,
        "graph TD; A-->B;",
        dir.path(),
        "definitely-not-a-real-program-xyzzy",
        INK,
    )
    .expect_err("no such program");
    let RenderError::ToolMissing { tool, hint } = err else {
        panic!("wrong error: {err:?}");
    };
    assert_eq!(tool, "definitely-not-a-real-program-xyzzy");
    assert!(!hint.is_empty(), "a missing tool has to say how to get it");
}

#[test]
fn an_already_rendered_diagram_is_not_rendered_again() {
    // Put the file where the renderer would have put it, then ask for
    // it with a tool that does not exist: a cache hit never runs
    // anything, so this has to succeed.
    let dir = tempfile::tempdir().unwrap();
    let src = "graph TD; A-->B;";
    let want = diagram_path(dir.path(), Diagram::Mermaid, src, INK);
    std::fs::create_dir_all(want.parent().unwrap()).unwrap();
    std::fs::write(&want, b"\x89PNG\r\n\x1a\n").unwrap();
    let got = render_diagram(Diagram::Mermaid, src, dir.path(), "xyzzy-no-such-tool", INK)
        .expect("a cache hit runs nothing");
    assert_eq!(got, want);
}

#[test]
fn the_default_tool_is_the_one_org_users_already_have() {
    // `mmdc` is mermaid-cli's binary and what every org mermaid setup
    // shells out to; `latex` is what `org-preview-latex-process-alist`
    // starts from.
    assert_eq!(Diagram::Mermaid.tool(), "mmdc");
    assert_eq!(Diagram::Latex.tool(), "latex");
}

#[test]
fn the_ink_is_part_of_what_names_the_file() {
    // Found on screen: `dvipng` draws in black by default, so the
    // first working LaTeX preview was black maths on a dark editor —
    // rendered correctly and unreadable. The ink is the theme's
    // foreground now, which means it is also part of what identifies
    // the picture: a picture drawn for a dark theme is the wrong
    // picture for a light one, and a cache that ignored the colour
    // would hand it over anyway.
    let dir = tempfile::tempdir().unwrap();
    let light = diagram_path(dir.path(), Diagram::Latex, "x", 0x0011_1111);
    let dark = diagram_path(dir.path(), Diagram::Latex, "x", 0x00cd_d6f4);
    assert_ne!(light, dark, "the colour has to change the name");
}

#[test]
fn the_ink_reaches_dvipng_as_a_colour_it_understands() {
    // dvipng wants `rgb r g b` with components in 0..1, not a hex
    // triple — a wrong spelling here is not an error, it is silently
    // black again.
    assert_eq!(
        closure_eval::dvipng_fg(0x00ff_ffff),
        "rgb 1.000 1.000 1.000"
    );
    assert_eq!(
        closure_eval::dvipng_fg(0x0000_0000),
        "rgb 0.000 0.000 0.000"
    );
}

#[test]
fn a_latex_fragment_is_wrapped_in_a_class_every_tex_has() {
    // Found on screen with a real `texliveSmall`: the wrapper asked
    // for `standalone`, which is a nicer class and is in no small TeX
    // distribution, so the run stopped to ask where `standalone.cls`
    // was and the note showed "No pages of output". `article` plus
    // `dvipng -T tight` is what org's own dvipng entry does, and it
    // works on the TeX people actually have installed.
    let doc = closure_eval::latex_document("$E = mc^2$");
    assert!(
        doc.contains("\\documentclass{article}"),
        "a preview that needs a full TeX is a preview most people cannot have: {doc}"
    );
    assert!(!doc.contains("standalone"), "{doc}");
    assert!(doc.contains("$E = mc^2$"), "the fragment survives: {doc}");
}

#[test]
fn a_diagram_language_still_answers_to_the_trust_allowlist() {
    // A diagram runs a program on your machine, so it is gated by the
    // same allowlist as a shell block and by default-deny like one:
    // rendering a picture is not a reason to skip the gate.
    assert!(!closure_eval::eval_allowed(&[], "mermaid"));
    assert!(closure_eval::eval_allowed(
        &["mermaid".to_owned()],
        "mermaid"
    ));
    assert!(
        closure_eval::eval_allowed(&["tex".to_owned()], "latex"),
        "the alias trusts the language"
    );
}
