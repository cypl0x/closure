//! "[#C] top notch UI for job scheduler (cron like syntax?)".
//!
//! The pane listed `(format!("{:?}", job.spec), job.command)` — a Rust
//! Debug dump paired with a command, in an untyped tuple. Three things
//! wrong with one line: it shows the parser's idea of the schedule
//! rather than the user's, it cannot say when the job next runs, and
//! nothing in the type says which string is which.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Jobs
:PROPERTIES:
:ID: 01JOBSAAAAAAAAAAAAAAAAAAAA
:END:
#+BEGIN_SRC cron
0 9 * * * agenda
*/15 * * * * sync-now
#+END_SRC
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn a_job_row_says_which_field_is_which() {
    let (_d, shell, app) = app();
    let rows = app.cron_rows(&shell);
    let first = rows.first().expect("a job");
    assert_eq!(first.command, "agenda");
    assert_eq!(first.schedule, "0 9 * * *");
}

#[test]
fn the_schedule_is_the_one_that_was_written() {
    // Not `CronSpec { minute: Exact(0), … }`.
    let (_d, shell, app) = app();
    let rows = app.cron_rows(&shell);
    for row in &rows {
        assert!(
            !row.schedule.contains("CronSpec") && !row.schedule.contains("Exact"),
            "the pane is showing the parser's idea of it: {}",
            row.schedule
        );
    }
    assert!(rows.iter().any(|r| r.schedule == "*/15 * * * *"));
}

#[test]
fn every_job_says_when_it_runs_in_words() {
    let (_d, shell, app) = app();
    let rows = app.cron_rows(&shell);
    let agenda = rows.iter().find(|r| r.command == "agenda").expect("agenda");
    assert_eq!(agenda.when, "every day at 09:00");
    let sync = rows.iter().find(|r| r.command == "sync-now").expect("sync");
    assert_eq!(sync.when, "every 15 minutes");
}

#[test]
fn a_vault_with_no_jobs_lists_none() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let app = ModalApp::new(InputMode::Doom);
    assert!(app.cron_rows(&shell).is_empty());
}
