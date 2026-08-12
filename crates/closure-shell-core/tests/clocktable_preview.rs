//! A `#+BEGIN: clocktable` block reads as a table, where it sits.
//!
//! Fifth construct through the seam the composition work opened, and
//! the same rule as the other four (I12): the editor shows the file,
//! the preview shows what the file means. Org fills a clocktable in by
//! rewriting the document, which makes its bytes depend on when it was
//! last refreshed; closure renders it instead, and the block keeps the
//! two lines the author typed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* This week
:PROPERTIES:
:ID: 01CLOCKTABLE000000001
:END:
#+BEGIN: clocktable
#+END:
* Writing
:PROPERTIES:
:ID: 01CLOCKTABLE000000002
:END:
CLOCK: [2026-08-10 Mon 09:00]--[2026-08-10 Mon 10:30] =>  1:30
* Reading
:PROPERTIES:
:ID: 01CLOCKTABLE000000003
:END:
CLOCK: [2026-08-10 Mon 11:00]--[2026-08-10 Mon 11:30] =>  0:30
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn detail_of(
    app: &mut ModalApp,
    shell: &Shell,
    title: &str,
) -> std::sync::Arc<closure_shell_core::Detail> {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail")
}

#[test]
fn the_block_reads_as_the_time_that_was_clocked() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "This week");
    assert!(
        d.body.contains("1:30"),
        "no clocked time in the preview: {}",
        d.body
    );
    assert!(d.body.contains("Writing"), "{}", d.body);
    assert!(d.body.contains("2:00"), "no total: {}", d.body);
}

#[test]
fn the_file_still_says_what_the_author_typed() {
    // I1 and I12 together: rendering a clocktable must not write one.
    let (dir, shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "This week");
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(on_disk.contains("#+BEGIN: clocktable\n#+END:"), "{on_disk}");
    assert!(
        !on_disk.contains("Total time"),
        "the preview reached the file"
    );
}

#[test]
fn a_headline_with_no_clocktable_is_untouched() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Writing");
    assert!(d.body.contains("CLOCK:"), "{}", d.body);
    assert!(!d.body.contains("Total time"), "{}", d.body);
}

#[test]
fn a_vault_with_nothing_clocked_says_so() {
    let src = "\
* Week
:PROPERTIES:
:ID: 01CLOCKTABLE000000004
:END:
#+BEGIN: clocktable
#+END:
";
    let (_d, shell, mut app) = app(src);
    let d = detail_of(&mut app, &shell, "Week");
    assert!(d.body.contains("no clocked time"), "{}", d.body);
}
