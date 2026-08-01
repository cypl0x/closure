//! The detail pane shows what is under a headline, not just its prose.
//!
//! Reported 2026-08-02, marked "Do this first!": "the children of a
//! headline should be visible in the right detail view body as well.
//! Just like in the body editor."
//!
//! The pane showed a headline's own body text, so a headline whose
//! content *is* its children — which is most of them in an outline —
//! previewed as blank. You had to open the editor to find out whether
//! there was anything there.
//!
//! One difference from the editor, deliberate: the children come
//! through without their `:PROPERTIES:` drawers. The editor needs them
//! in the text because that is what carries a child's id through the
//! read/write round trip; a read-only preview has no round trip and no
//! reason to spend four lines per child on it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const SRC: &str = "* Parent\n\
                   :PROPERTIES:\n\
                   :ID: 01HQDCH0000000000000001\n\
                   :END:\n\
                   parent prose\n\
                   ** TODO One\n\
                   :PROPERTIES:\n\
                   :ID: 01HQDCH0000000000000002\n\
                   :END:\n\
                   one prose\n\
                   *** Deep\n\
                   :PROPERTIES:\n\
                   :ID: 01HQDCH0000000000000003\n\
                   :END:\n\
                   ** Two\n\
                   :PROPERTIES:\n\
                   :ID: 01HQDCH0000000000000004\n\
                   :END:\n\
                   * Next\n\
                   :PROPERTIES:\n\
                   :ID: 01HQDCH0000000000000005\n\
                   :END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn on(app: &mut ModalApp, sh: &Shell, title: &str) {
    let at = app
        .rows(sh)
        .iter()
        .position(|r| r.title == title)
        .unwrap_or_else(|| panic!("no row {title}"));
    app.select(at, sh);
}

#[test]
fn the_children_are_in_the_detail() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    let d = app.detail(&sh).expect("detail");
    assert!(d.children.contains("** TODO One"), "{:?}", d.children);
    assert!(d.children.contains("** Two"), "{:?}", d.children);
}

#[test]
fn grandchildren_come_too() {
    // "and subsubsubheadings …" — the whole subtree, at every depth.
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    let d = app.detail(&sh).expect("detail");
    assert!(d.children.contains("*** Deep"), "{:?}", d.children);
}

#[test]
fn a_childs_own_prose_comes_with_it() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    assert!(
        app.detail(&sh)
            .expect("detail")
            .children
            .contains("one prose")
    );
}

#[test]
fn the_headlines_own_body_is_still_its_own_body() {
    // Kept separate so a shell can style them differently, and so every
    // existing reader of `body` keeps meaning what it meant.
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.body.trim(), "parent prose");
    assert!(!d.body.contains("One"), "no children in it: {:?}", d.body);
}

#[test]
fn a_sibling_is_not_a_child() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    assert!(!app.detail(&sh).expect("detail").children.contains("Next"));
}

#[test]
fn a_childless_headline_has_no_children_section() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Next");
    assert_eq!(app.detail(&sh).expect("detail").children, "");
}

#[test]
fn the_preview_drops_the_property_drawers() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    let d = app.detail(&sh).expect("detail");
    assert!(!d.children.contains(":PROPERTIES:"), "{:?}", d.children);
    assert!(!d.children.contains(":ID:"), "{:?}", d.children);
    assert!(!d.children.contains(":END:"), "{:?}", d.children);
    assert!(d.children.contains("** Two"), "the headline stays");
}

#[test]
fn it_follows_the_selection() {
    let (_d, sh, mut app) = fixture();
    on(&mut app, &sh, "One");
    let d = app.detail(&sh).expect("detail");
    assert!(
        d.children.contains("*** Deep"),
        "One's child: {:?}",
        d.children
    );
    assert!(!d.children.contains("Two"), "not its sibling's");
}

#[test]
fn it_tracks_an_edit_without_being_reopened() {
    // The detail is memoised against the vault revision, so a child
    // added anywhere has to invalidate it or the pane lies.
    let (_d, mut sh, mut app) = fixture();
    on(&mut app, &sh, "Parent");
    assert!(!app.detail(&sh).expect("detail").children.contains("Three"));
    let id = app.rows(&sh)[0].id.clone();
    let bid = closure_core::BlockId::from_existing(&id);
    sh.add_child(&bid, "", "Three").expect("add");
    assert!(
        app.detail(&sh).expect("detail").children.contains("Three"),
        "{:?}",
        app.detail(&sh).expect("detail").children
    );
}
