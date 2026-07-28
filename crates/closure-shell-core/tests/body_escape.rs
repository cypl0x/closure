//! A body line that looks like a headline must stay a body line.
//!
//! The body editor writes whatever is in its buffer straight back into
//! the org file, so `* Foo` typed into a note used to split the
//! outline: a new level-1 headline appeared and every following
//! sibling was reparented under it. Org's comma escape
//! ([`closure_org::escape_body`]) is applied at the editor seam — on
//! commit — and undone on the way back out, so what the user typed is
//! what the user sees.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{App, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Foo\n** Bar\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Open the selected headline's body and get into INSERT.
///
/// Two `i`s: the first is the outline's edit-body chord, which lands in
/// NORMAL in a modal input mode (a buffer opens the way Doom opens one);
/// the second is vim's own insert.
fn typing_body(app: &mut ModalApp, sh: &mut Shell) {
    app.on_key(sh, "i", false, false, Some('i'));
    assert_eq!(app.surface(), ModalSurface::EditBody);
    app.on_key(sh, "i", false, false, Some('i'));
}

fn typ(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            app.on_key(sh, "enter", false, false, None);
        } else {
            app.on_key(sh, &c.to_string(), false, false, Some(c));
        }
    }
}

// Contract revised 2026-07-28, on the user's report: a *headline* typed
// into a body is not prose that needs escaping — it is an outline entry,
// and what people mean by it is "this belongs under this". So a `* Foo`
// line becomes a real child (see `tests/body_children.rs` in
// closure-org for the rebasing rule). The comma escape stays for what
// it was always for: reading files that already contain one, and prose
// that merely *looks* like a headline.

#[test]
fn a_body_star_line_becomes_a_child_of_the_note() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    typing_body(&mut app, &mut sh);
    typ(&mut app, &mut sh, "* Foo body\n");
    app.on_key(&mut sh, "enter", true, false, None); // C-Enter commit
    let titles: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert_eq!(
        titles,
        vec!["Foo", "Foo body", "Bar"],
        "filed under the note it was typed in"
    );
    let level = app
        .rows(&sh)
        .iter()
        .find(|r| r.title == "Foo body")
        .expect("there")
        .level;
    assert_eq!(level, 2, "one level under `Foo`");
}

#[test]
fn the_prose_around_it_stays_in_the_body() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    typing_body(&mut app, &mut sh);
    typ(&mut app, &mut sh, "some prose\n* Foo body\n");
    app.on_key(&mut sh, "enter", true, false, None);
    assert_eq!(
        app.detail(&sh).expect("detail").body.trim_end(),
        "some prose",
        "the body kept the prose and lost only the headline"
    );
}

#[test]
fn text_that_merely_looks_like_a_headline_is_still_escaped() {
    // `*bold*` has no space after its star, so it is markup, not an
    // outline entry — it stays in the body, escaped on disk exactly as
    // before.
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    typing_body(&mut app, &mut sh);
    typ(&mut app, &mut sh, "*bold* opening\n");
    app.on_key(&mut sh, "enter", true, false, None);
    assert_eq!(app.rows(&sh).len(), 2, "no headline was invented");
    assert_eq!(
        app.detail(&sh).expect("detail").body.trim_end(),
        "*bold* opening"
    );
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("*bold* opening"), "{on_disk}");
}

#[test]
fn committing_an_unchanged_body_twice_changes_nothing() {
    // The old worry was commas accumulating; the new one is a child
    // being filed twice. Neither may happen.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    typing_body(&mut app, &mut sh);
    typ(&mut app, &mut sh, "* one\n");
    app.on_key(&mut sh, "enter", true, false, None);
    let after_first: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();

    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.on_key(&mut sh, "enter", true, false, None);
    let after_second: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert_eq!(after_first, after_second, "a second commit is a no-op");
}

#[test]
fn a_hand_written_escape_reads_back_as_the_author_meant() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Foo\n,* not a headline\n").expect("write");
    let sh = Shell::new(Vault::open(dir.path()).expect("open"));
    let app = ModalApp::new(InputMode::Vim);
    assert_eq!(app.rows(&sh).len(), 1, "the escape hides the star");
    assert_eq!(
        app.detail(&sh).expect("detail").body.trim_end(),
        "* not a headline"
    );
}

#[test]
fn the_launcher_editor_escapes_too() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.begin_edit_body(&sh);
    "* Foo body\n".clone_into(app.body_buffer_mut());
    app.commit_edit_body(&mut sh);
    assert_eq!(app.rows(&sh).len(), 2, "still two headlines");
    app.begin_edit_body(&sh);
    assert_eq!(app.body_buffer().trim_end(), "* Foo body");
}

// === What the escape must stay invisible to ===
//
// The escape is an on-disk spelling, so every surface that shows body
// text has to undo it. The detail pane and the editor did; the body
// search hit list read `body_text()` raw and printed the comma.

/// A vault whose one note has an escaped body line and two hits.
fn searchable() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Note\n,* needle one\nmiddle\nneedle two\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The body-search surface with `query` typed into it, exactly as the
/// shells reach it.
fn searching(sh: &mut Shell, query: &str) -> ModalApp {
    let mut app = ModalApp::new(InputMode::Vim);
    app.run(sh, "body-search");
    assert_eq!(app.surface(), ModalSurface::BodySearch);
    typ(&mut app, sh, query);
    app
}

#[test]
fn a_body_search_hit_shows_the_text_the_author_typed() {
    let (_d, mut sh) = searchable();
    let app = searching(&mut sh, "needle one");
    let rows = app.body_search_rows(&sh);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].1, "Note — * needle one",
        "the on-disk comma must not reach the hit list"
    );
}

#[test]
fn a_body_search_reports_every_matching_line() {
    // One hit per headline hid the rest of the matches in the note you
    // were looking through — the case the search exists for.
    let (_d, mut sh) = searchable();
    let app = searching(&mut sh, "needle");
    let rows = app.body_search_rows(&sh);
    assert_eq!(rows.len(), 2, "both lines, not just the first: {rows:?}");
    assert!(rows[0].1.ends_with("* needle one"));
    assert!(rows[1].1.ends_with("needle two"));
}

#[test]
fn a_body_search_still_matches_nothing_on_an_empty_query() {
    let (_d, mut sh) = searchable();
    let mut app = ModalApp::new(InputMode::Vim);
    app.run(&mut sh, "body-search");
    assert!(app.body_search_rows(&sh).is_empty());
}
