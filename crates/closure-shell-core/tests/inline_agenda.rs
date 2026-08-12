//! An inline task shows up where tasks are read.
//!
//! `inline_tasks()` finds them and nothing asked. An inline task is a
//! task attached to the paragraph it sits in — that is the whole reason
//! to write one instead of a headline — and the agenda is where tasks
//! are read. One that is invisible there is one nobody writes twice.
//!
//! It is deliberately not in the outline: that was the bug the parser
//! fix corrected, and putting it back into the tree here would undo it.
//! Not outline structure and not invisible are compatible, and the
//! agenda is exactly where the difference shows.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Kitchen
:PROPERTIES:
:ID: 01INLAG0000000000000001
:END:
the tap has been dripping since Tuesday
*************** TODO ring the plumber
*************** END
and the light needs a bulb
* Done thing
:PROPERTIES:
:ID: 01INLAG0000000000000002
:END:
*************** DONE already sorted
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn an_inline_task_is_offered_as_a_task() {
    let (_d, shell, app) = app();
    let rows = app.inline_task_rows(&shell);
    let titles: Vec<&str> = rows.iter().map(|(_, t, _)| t.as_str()).collect();
    assert!(
        titles.contains(&"ring the plumber"),
        "the inline task is nowhere a reader would look: {titles:?}"
    );
}

#[test]
fn it_says_which_headline_it_hangs_off() {
    // An inline task with no context is a fragment: "ring the plumber"
    // is only actionable next to "Kitchen".
    let (_d, shell, app) = app();
    let rows = app.inline_task_rows(&shell);
    let (context, _, _) = rows
        .iter()
        .find(|(_, t, _)| t == "ring the plumber")
        .expect("the task");
    assert_eq!(context, "Kitchen");
}

#[test]
fn it_says_which_file_it_is_in() {
    let (_d, shell, app) = app();
    let rows = app.inline_task_rows(&shell);
    assert!(rows.iter().all(|(_, _, path)| path.ends_with("notes.org")));
}

#[test]
fn a_finished_one_is_still_listed_with_its_keyword() {
    // Filtering DONE out is the reader's decision, not this function's:
    // an agenda that hides them cannot show what was finished today.
    let (_d, shell, app) = app();
    let rows = app.inline_task_rows(&shell);
    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[test]
fn it_does_not_put_them_back_in_the_outline() {
    // The parser fix took them out of the tree on purpose. Making them
    // visible must not undo that.
    let (_d, shell, app) = app();
    let rows = app.rows(&shell);
    assert_eq!(
        rows.len(),
        2,
        "{:?}",
        rows.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
}

#[test]
fn a_vault_without_any_offers_none() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* Plain\n:PROPERTIES:\n:ID: 01INLAG0000000000000003\n:END:\nbody\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let app = ModalApp::new(InputMode::Doom);
    assert!(app.inline_task_rows(&shell).is_empty());
}

#[test]
fn the_context_is_the_headline_it_is_under_at_any_depth() {
    // The first version of this looked for lines starting with `*` but
    // not `***`, which is right for a one- or two-level file and wrong
    // for every other one: a task under a level-four headline got the
    // level-two headline above it, or nothing.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* House\n:PROPERTIES:\n:ID: 01INLAG0000000000000004\n:END:\n\
         **** Bathroom\n:PROPERTIES:\n:ID: 01INLAG0000000000000005\n:END:\n\
         the extractor rattles\n*************** TODO call someone\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let app = ModalApp::new(InputMode::Doom);
    let rows = app.inline_task_rows(&shell);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].0, "Bathroom",
        "it took the wrong headline: {rows:?}"
    );
}
