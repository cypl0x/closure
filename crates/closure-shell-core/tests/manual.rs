//! "Emacs like manual directly within closure (self documented).
//! Currently we just have … Generate on the fly (or JIT) via LLM from
//! the source repository? This would be cool (but can get expensive
//! and/or slow. And is not available offline if not using a local
//! LLM)."
//!
//! The doubts in that sentence are the argument against it. What makes
//! Emacs' manual trustworthy is not that it is well written — it is
//! that `C-h k` and `C-h f` answer from the running program, so the
//! documentation cannot drift from the binary. An LLM reading the
//! source is a second thing that can be wrong, costs money, and is not
//! there on a train.
//!
//! So the manual is generated from the same registry the palette and
//! which-key read. It cannot go stale, it costs nothing, and it works
//! offline — which is the whole point of a local-first tool.
//!
//! `tutorial.org` already existed and is a different document: a
//! tutorial teaches one path through, a manual is complete.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::manual_org;

#[test]
fn every_command_in_the_registry_is_in_the_manual() {
    // Complete is the difference between a manual and a tutorial. A
    // command you can run and cannot look up is the gap this closes.
    let manual = manual_org(InputMode::Doom);
    for name in closure_shell_core::palette_command_names() {
        assert!(
            manual.contains(name),
            "`{name}` can be run and cannot be looked up"
        );
    }
}

#[test]
fn every_description_is_there_too() {
    let manual = manual_org(InputMode::Doom);
    for (_, desc) in closure_shell_core::palette_descriptions() {
        assert!(manual.contains(desc), "missing: {desc}");
    }
}

#[test]
fn it_names_the_chords_of_the_mode_it_was_asked_about() {
    // The same command has different keys in different modes, so a
    // manual that named one mode's would be wrong for four users out
    // of five.
    let doom = manual_org(InputMode::Doom);
    assert!(doom.contains("SPC h k"), "Doom's leader chord is missing");
    let vim = manual_org(InputMode::Vim);
    assert!(vim.contains("g ?"), "the modal chord is missing");
    assert!(
        !vim.contains("SPC h k"),
        "vim's manual is showing Doom's leader"
    );
}

#[test]
fn it_carries_the_invariants() {
    // `closure spec` prints these from code as enforced invariants;
    // the manual is where a reader would look for them.
    let manual = manual_org(InputMode::Doom);
    for want in ["I1", "I4", "I8"] {
        assert!(manual.contains(want), "{want} is not in the manual");
    }
}

#[test]
fn it_is_org_that_parses() {
    // It opens *in closure*, so it has to be a document closure can
    // read — and round-trip, since that is I1.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let src = manual_org(mode);
        let doc = closure_org::parse(&src).expect("the manual is org");
        assert_eq!(
            closure_org::print(&doc),
            src,
            "{mode:?}: does not round-trip"
        );
        // Three top-level sections, each with children — counting
        // roots alone said "5" was too many and it is not.
        assert!(
            doc.roots().len() >= 3,
            "{mode:?}: a manual with no sections"
        );
        let deep: usize = doc.roots().iter().map(|r| r.children().len()).sum();
        assert!(deep >= 5, "{mode:?}: the Keys section has no subsections");
    }
}

#[test]
fn it_says_it_is_generated_and_from_what() {
    // A file that looks hand-written invites hand-editing, and an edit
    // to it is lost the next time it is asked for.
    let manual = manual_org(InputMode::Doom);
    let head = manual.lines().take(12).collect::<Vec<_>>().join("\n");
    assert!(
        head.to_lowercase().contains("generated"),
        "it does not say where it came from:\n{head}"
    );
}

#[test]
fn a_command_with_no_chord_is_still_listed() {
    // Palette-only is a real state and "no key" is an answer.
    let manual = manual_org(InputMode::Doom);
    assert!(manual.contains("trust-language"));
}

#[test]
fn the_manual_opens_inside_closure() {
    // "directly within closure": a manual you have to leave the app to
    // read is a README.
    use closure_shell_core::{ModalApp, ModalSurface, Shell};
    use closure_store::Vault;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01MANUALAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "manual");
    assert_eq!(app.surface(), ModalSurface::Manual);
    let rows = app.manual_rows();
    assert!(rows.len() > 40, "only {} lines of manual", rows.len());
    assert!(
        rows.iter().any(|r| r.contains("describe-key")),
        "the manual is missing its own commands"
    );
}

#[test]
fn it_reads_the_mode_you_are_actually_in() {
    // A manual showing Doom's keys to a vim user is worse than none.
    use closure_shell_core::{ModalApp, Shell};
    use closure_store::Vault;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Vim);
    app.run(&mut shell, "manual");
    let text = app.manual_rows().join("\n");
    assert!(text.contains("g ?"), "not vim's keys");
    assert!(
        !text.contains("SPC h k"),
        "showing Doom's leader to a vim user"
    );
}

#[test]
fn the_pane_view_is_not_the_file_view() {
    // The manual is org, and the CLI prints it as org. In a pane every
    // line costs a row, so the `#+TITLE:` machinery and the double
    // blank lines between sections turn a dense reference into
    // scrolling.
    use closure_shell_core::{ModalApp, Shell};
    use closure_store::Vault;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "manual");
    let rows = app.manual_rows();

    assert!(
        !rows.iter().any(|r| r.starts_with("#+")),
        "the org preamble is furniture in a pane"
    );
    assert!(
        !rows.windows(2).any(|w| w[0].is_empty() && w[1].is_empty()),
        "two blank rows in a row"
    );
    // And it is still the manual.
    assert!(rows.iter().any(|r| r.contains("Invariants")));
    assert!(rows.iter().any(|r| r.contains("describe-key")));
}
