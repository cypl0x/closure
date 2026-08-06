//! The last subsystems that had no way into the GUI.
//!
//! Each of these has been real in the kernel for a while and reachable
//! only from `closure <subcommand>`: the link graph (orphans, hubs,
//! dead links), the recorded command journal, scheduled jobs, and the
//! installed plugin set. A feature you can only reach by quitting the
//! app and typing a subcommand is, from the app's point of view, not
//! there.
//!
//! They are read-only lists, so what matters is that each one answers
//! with the right rows, says something useful when it is empty, and
//! costs nothing to open.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const A: &str = "01HQGRAPH0000000000001";
const B: &str = "01HQGRAPH0000000000002";
const C: &str = "01HQGRAPH0000000000003";

/// A → B twice, A → missing. C is linked by nobody.
fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!(
            "* Hub\n:PROPERTIES:\n:ID: {A}\n:END:\n\
             links [[id:{B}]] and [[id:01HQMISSING000000000000]]\n\
             * Target\n:PROPERTIES:\n:ID: {B}\n:END:\n\
             also [[id:{B}]]\n\
             * Lonely\n:PROPERTIES:\n:ID: {C}\n:END:\n"
        ),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

// === the link graph ===

#[test]
fn the_graph_command_opens_the_surface() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "graph");
    assert_eq!(app.surface(), ModalSurface::Graph);
}

#[test]
fn hubs_are_ranked_by_how_many_things_point_at_them() {
    let (_d, shell, app) = fixture();
    let hubs = app.hub_rows(&shell);
    assert!(!hubs.is_empty(), "something is linked");
    assert_eq!(hubs[0].0, B, "B is linked twice: {hubs:?}");
    assert_eq!(hubs[0].2, 2, "and the count says so");
    assert_eq!(hubs[0].1, "Target", "with its title, not just an id");
}

#[test]
fn orphans_are_the_headlines_nothing_points_at() {
    let (_d, shell, app) = fixture();
    let orphans = app.orphan_rows(&shell);
    let ids: Vec<&str> = orphans.iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(&C), "Lonely is an orphan: {orphans:?}");
    assert!(!ids.contains(&B), "B is linked, so it is not");
}

#[test]
fn dead_links_are_the_targets_that_do_not_exist() {
    let (_d, shell, app) = fixture();
    let dead = app.dead_link_rows(&shell);
    assert_eq!(dead.len(), 1, "{dead:?}");
    assert!(dead[0].contains("01HQMISSING"), "{dead:?}");
}

#[test]
fn an_empty_vault_has_an_empty_graph_not_a_panic() {
    let dir = tempfile::tempdir().expect("tmp");
    let vault = Vault::open(dir.path()).expect("open");
    let (shell, app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    assert!(app.hub_rows(&shell).is_empty());
    assert!(app.orphan_rows(&shell).is_empty());
    assert!(app.dead_link_rows(&shell).is_empty());
}

// === the journal ===

#[test]
fn the_journal_surface_reads_the_recorded_history() {
    let (dir, mut shell, mut app) = fixture();
    let journal = closure_record::Journal::new(dir.path(), true);
    journal
        .record(0, "rename", "Hub -> Hubbed")
        .expect("record");
    app.run(&mut shell, "journal");
    assert_eq!(app.surface(), ModalSurface::Journal);
    let rows = app.journal_rows(&shell);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].contains("rename"), "{rows:?}");
}

#[test]
fn an_unrecorded_vault_has_an_empty_journal() {
    let (_d, shell, app) = fixture();
    assert!(app.journal_rows(&shell).is_empty());
}

// === scheduled jobs ===

#[test]
fn the_cron_surface_lists_the_jobs_in_the_vault() {
    let (dir, mut shell, mut app) = fixture();
    fs::write(
        dir.path().join("jobs.org"),
        "#+BEGIN_SRC closure-cron\n0 9 * * * capture Daily review\n#+END_SRC\n",
    )
    .expect("write");
    shell.vault.reload().expect("reload");
    app.run(&mut shell, "cron");
    assert_eq!(app.surface(), ModalSurface::Cron);
    let jobs = app.cron_rows(&shell);
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert!(jobs[0].command.contains("Daily review"), "{jobs:?}");
    assert_eq!(
        jobs[0].schedule, "0 9 * * *",
        "the spec as written: {jobs:?}"
    );
    assert_eq!(jobs[0].when, "every day at 09:00", "and in words: {jobs:?}");
}

#[test]
fn a_vault_with_no_jobs_lists_none() {
    let (_d, shell, app) = fixture();
    assert!(app.cron_rows(&shell).is_empty());
}

#[test]
fn a_malformed_cron_block_does_not_break_the_surface() {
    // A typo in a job spec must not make the pane unopenable.
    let (dir, mut shell, app) = fixture();
    fs::write(
        dir.path().join("jobs.org"),
        "#+BEGIN_SRC closure-cron\nnot a cron line at all\n#+END_SRC\n",
    )
    .expect("write");
    shell.vault.reload().expect("reload");
    let _ = app.cron_rows(&shell);
}

// === every surface is reachable and leaves cleanly ===

#[test]
fn each_surface_opens_and_escape_returns() {
    for (command, surface) in [
        ("graph", ModalSurface::Graph),
        ("journal", ModalSurface::Journal),
        ("cron", ModalSurface::Cron),
    ] {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        assert_eq!(app.surface(), surface, "{command} opens its surface");
        app.on_key(&mut shell, "escape", false, false, None);
        assert_eq!(
            app.surface(),
            ModalSurface::Browse,
            "{command}: Escape returns"
        );
    }
}
