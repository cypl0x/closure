//! A file that asks to open folded opens folded.
//!
//! `#+STARTUP: overview` is how a document says which way it wants to
//! be met, and every file opened the same way regardless. The queue
//! file driving this whole session carries `#+STARTUP: overview` at the
//! top and closure ignored it every time I opened it.
//!
//! It applies once, on the way in. `:VISIBILITY:` is what the reader
//! last left a headline as and it wins: a file's opinion about how it
//! opens must not overrule somebody who has since folded something
//! themselves, or the fold you set would come back undone.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const OVERVIEW: &str = "\
#+STARTUP: overview
* Top
:PROPERTIES:
:ID: 01STARTUP00000000000001
:END:
** Under it
:PROPERTIES:
:ID: 01STARTUP00000000000002
:END:
";

const NOTHING: &str = "\
* Top
:PROPERTIES:
:ID: 01STARTUP00000000000003
:END:
** Under it
:PROPERTIES:
:ID: 01STARTUP00000000000004
:END:
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let shell = Shell::new(vault);
    // Deliberately nothing between building the app and asking it for
    // rows. The first version of this called `apply_startup` here, and
    // that call was the entire feature: the gpui window did not make
    // it, so a file asking to open folded opened open in the only shell
    // anyone was looking at. Opening a vault has to be enough.
    let app = ModalApp::new(InputMode::Doom);
    (dir, shell, app)
}

#[test]
fn overview_hides_what_is_under_a_headline() {
    let (_d, shell, app) = app(OVERVIEW);
    let titles: Vec<String> = app.rows(&shell).iter().map(|r| r.title.clone()).collect();
    assert_eq!(titles, vec!["Top".to_owned()], "{titles:?}");
}

#[test]
fn a_file_that_asks_for_nothing_opens_as_it_did() {
    let (_d, shell, app) = app(NOTHING);
    assert_eq!(app.rows(&shell).len(), 2);
}

#[test]
fn showall_leaves_everything_open() {
    let (_d, shell, app) = app(&OVERVIEW.replace("overview", "showall"));
    assert_eq!(app.rows(&shell).len(), 2);
}

#[test]
fn it_does_not_write_to_the_file() {
    // A fold applied on the way in is a view, not an edit. Writing
    // `:VISIBILITY:` for every headline because a file said `overview`
    // would turn opening a document into a commit.
    let (dir, shell, app) = app(OVERVIEW);
    let _ = app.rows(&shell);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.org")).unwrap(),
        OVERVIEW
    );
}
