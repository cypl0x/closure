//! "implement the job scheduler ('cron') stack".
//!
//! Jobs were parsed, listed in a pane, and never run. `Scheduler`
//! exists in `closure-cron` and nothing outside that crate ever
//! referred to it — so a `#+BEGIN_SRC cron` block was a list of
//! intentions.
//!
//! A scheduler is three decisions and this pins all three: which jobs
//! are due at a given wall-clock minute, that each fires *once* for
//! that minute however often the frame timer asks, and that firing
//! means running the registry command (I8) rather than a shell.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Scheduled
:PROPERTIES:
:ID: 01CRONRUNAAAAAAAAAAAAAAAAA
:END:
#+BEGIN_SRC cron
0 9 * * * next-file
30 9 * * * prev-file
#+END_SRC
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn a_job_fires_at_its_minute() {
    let (_d, mut shell, mut app) = app();
    // Wednesday 2026-08-05, 09:00.
    let fired = app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3);
    assert_eq!(fired, vec!["next-file".to_owned()]);
}

#[test]
fn a_job_does_not_fire_at_someone_elses_minute() {
    let (_d, mut shell, mut app) = app();
    assert!(app.cron_tick_at(&mut shell, 15, 9, 5, 8, 3).is_empty());
    assert_eq!(
        app.cron_tick_at(&mut shell, 30, 9, 5, 8, 3),
        vec!["prev-file".to_owned()]
    );
}

#[test]
fn the_frame_timer_asking_twice_does_not_run_it_twice() {
    // The tick fires many times a second and a minute is sixty of
    // them. A job that ran on every frame for a minute would run
    // thousands of times — this is the whole reason a scheduler needs
    // memory rather than only a predicate.
    let (_d, mut shell, mut app) = app();
    assert_eq!(app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3).len(), 1);
    for _ in 0..50 {
        assert!(
            app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3).is_empty(),
            "it fired twice in the same minute"
        );
    }
}

#[test]
fn the_same_job_fires_again_the_next_day() {
    // Memory that never forgets is a job that runs once, ever.
    let (_d, mut shell, mut app) = app();
    assert_eq!(app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3).len(), 1);
    assert_eq!(app.cron_tick_at(&mut shell, 0, 9, 6, 8, 4).len(), 1);
}

#[test]
fn a_vault_with_no_jobs_does_nothing_at_all() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3).is_empty());
}

#[test]
fn firing_says_so_where_a_person_can_see_it() {
    // A command running by itself, with no word about why, reads as
    // the shell doing something on its own.
    let (_d, mut shell, mut app) = app();
    app.cron_tick_at(&mut shell, 0, 9, 5, 8, 3);
    let said = app.status();
    assert!(
        said.contains("next-file") && said.to_lowercase().contains("cron"),
        "nothing said a job ran: {said}"
    );
}
