//! A clocktable reports the scope it asked for.
//!
//! `clocktable_params` read `:scope file|vault` and the preview bound
//! it to `_params` — the underscore being the compiler told to stop
//! mentioning the one thing the feature needed — and always reported
//! the whole vault. So the default was wrong too: a block that says
//! nothing means the file it sits in, and a weekly note in a vault of
//! fifty files showed fifty files' time under "this week".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const THIS_FILE: &str = "\
* This week
:PROPERTIES:
:ID: 01CLOCKSCOPE000000001
:END:
#+BEGIN: clocktable
#+END:
* Writing
:PROPERTIES:
:ID: 01CLOCKSCOPE000000002
:END:
CLOCK: [2026-08-10 Mon 09:00]--[2026-08-10 Mon 10:30] =>  1:30
";

const OTHER_FILE: &str = "\
* Something else entirely
:PROPERTIES:
:ID: 01CLOCKSCOPE000000003
:END:
CLOCK: [2026-08-10 Mon 14:00]--[2026-08-10 Mon 16:00] =>  2:00
";

fn vault_with(block: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("week.org"),
        THIS_FILE.replace("#+BEGIN: clocktable", block),
    )
    .unwrap();
    std::fs::write(dir.path().join("other.org"), OTHER_FILE).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn body_of(app: &mut ModalApp, shell: &Shell) -> String {
    let rows = app.rows(shell);
    let i = rows
        .iter()
        .position(|r| r.title == "This week")
        .expect("a row");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail").body.clone()
}

#[test]
fn a_block_that_says_nothing_reports_its_own_file() {
    let (_d, shell, mut app) = vault_with("#+BEGIN: clocktable");
    let body = body_of(&mut app, &shell);
    assert!(body.contains("Writing"), "{body}");
    assert!(
        !body.contains("Something else entirely"),
        "the default reported the whole vault: {body}"
    );
    assert!(body.contains("1:30"), "{body}");
}

#[test]
fn scope_file_reports_its_own_file() {
    let (_d, shell, mut app) = vault_with("#+BEGIN: clocktable :scope file");
    let body = body_of(&mut app, &shell);
    assert!(!body.contains("Something else entirely"), "{body}");
}

#[test]
fn scope_vault_reports_the_vault() {
    let (_d, shell, mut app) = vault_with("#+BEGIN: clocktable :scope vault");
    let body = body_of(&mut app, &shell);
    assert!(body.contains("Writing"), "{body}");
    assert!(
        body.contains("Something else entirely"),
        "asked for the vault and got one file: {body}"
    );
}

#[test]
fn the_total_follows_the_scope() {
    let (_d, shell, mut app) = vault_with("#+BEGIN: clocktable :scope vault");
    let body = body_of(&mut app, &shell);
    assert!(body.contains("3:30"), "{body}");
    let (_d, shell, mut app) = vault_with("#+BEGIN: clocktable");
    let body = body_of(&mut app, &shell);
    assert!(body.contains("1:30"), "{body}");
    assert!(!body.contains("3:30"), "{body}");
}
