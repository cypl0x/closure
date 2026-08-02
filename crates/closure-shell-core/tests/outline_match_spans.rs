//! "search in outline tree view doesn't highlight the words like it
//! does in the command palette for the same filter. Can you apply the
//! same rules and behavior from the command palette? Consistency is
//! key."
//!
//! The palette has told you *why* a row is in the list since it became
//! a picker: the characters your filter matched are painted in the
//! accent colour, which is what makes a list of near-identical
//! candidates readable. The outline scored its rows with the same
//! fuzzy matcher and then threw the positions away — so filtering the
//! tree gave you a shorter list and no reason for it.
//!
//! Same matcher, same spans, same painter. The one place the two
//! genuinely differ is the empty filter: a palette with no query is a
//! menu, and the outline with no query is the whole document, so
//! neither highlights anything.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha project
:PROPERTIES:
:ID: 01HQSPAN00000000000001
:END:
* Beta project
:PROPERTIES:
:ID: 01HQSPAN00000000000002
:END:
* Gamma notes
:PROPERTIES:
:ID: 01HQSPAN00000000000003
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Filter the outline by typing `q` into the search prompt.
fn search(app: &mut ModalApp, shell: &mut Shell, q: &str) {
    app.run(shell, "search");
    for c in q.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn a_filtered_row_says_which_characters_matched() {
    let (_d, mut shell, mut app) = fixture();
    search(&mut app, &mut shell, "proj");
    let rows = app.rows(&shell);
    assert!(!rows.is_empty(), "something matched");
    for r in &rows {
        assert!(
            !r.matches.is_empty(),
            "{:?} is in the list with no reason: {:?}",
            r.title,
            r.matches
        );
    }
}

#[test]
fn the_spans_land_on_the_characters_they_claim() {
    // A span that points past the end, or at the wrong letters, paints
    // the highlight somewhere the eye cannot use it.
    let (_d, mut shell, mut app) = fixture();
    search(&mut app, &mut shell, "proj");
    for r in app.rows(&shell) {
        let mut hit = String::new();
        for &(start, end) in &r.matches {
            assert!(
                end <= r.title.len(),
                "{:?} past the end of {:?}",
                (start, end),
                r.title
            );
            hit.push_str(&r.title[start..end]);
        }
        assert!(
            hit.to_lowercase().contains("proj"),
            "{hit:?} is not what was searched for, in {:?}",
            r.title
        );
    }
}

#[test]
fn an_unfiltered_outline_highlights_nothing() {
    // The whole document is not a list of candidates.
    let (_d, shell, app) = fixture();
    for r in app.rows(&shell) {
        assert!(r.matches.is_empty(), "{:?}", r.title);
    }
}

#[test]
fn the_outline_and_the_palette_agree_about_one_filter() {
    // "Can you apply the same rules and behavior from the command
    // palette?" — the same matcher, so the same answer for the same
    // text. Checked against the matcher itself rather than against a
    // second copy of it.
    let (_d, mut shell, mut app) = fixture();
    search(&mut app, &mut shell, "alpha");
    let row = app
        .rows(&shell)
        .into_iter()
        .find(|r| r.title.starts_with("Alpha"))
        .expect("a match");
    assert_eq!(row.matches, closure_query::match_spans("alpha", &row.title));
}

#[test]
fn a_filter_that_matches_nothing_leaves_no_stray_spans() {
    let (_d, mut shell, mut app) = fixture();
    search(&mut app, &mut shell, "zzzznothing");
    assert!(app.rows(&shell).is_empty());
}
