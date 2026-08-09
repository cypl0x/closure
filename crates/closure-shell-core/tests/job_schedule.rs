//! "visual UI for scheduled jobs. Highly polished UI and UX. Top notch
//! UI and UX. Highly professional."
//!
//! The pane listed the command, the schedule in words, and the
//! expression. Everything it said was already in the file you wrote —
//! it was a rendering of the block, not a view of the scheduler.
//!
//! What a scheduler view is *for* is the two things the file cannot
//! tell you: when this next runs, and whether it ran. Plus the one
//! thing that silently breaks a job forever — a command name nothing
//! answers to.
//!
//! These pin the arithmetic. The pane is the same facts in columns.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

/// 2026-08-06 12:00:00 UTC. Noon, hence the name.
const NOON: u64 = 1_786_017_600;

fn vault(block: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("jobs.org"),
        format!("* Jobs\n:PROPERTIES:\n:ID: 01JOBSCHED000000000001\n:END:\n#+BEGIN_SRC cron\n{block}\n#+END_SRC\n"),
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

fn rows(shell: &Shell, now: u64) -> Vec<closure_shell_core::JobRow> {
    ModalApp::new(InputMode::Doom).cron_rows_at(shell, now)
}

#[test]
fn a_job_says_when_it_next_runs() {
    let (_d, shell) = vault("*/15 * * * * next-file");
    let r = rows(&shell, NOON);
    assert_eq!(r.len(), 1);
    assert_eq!(
        r[0].next.as_deref(),
        Some("12:15"),
        "a quarter-hourly job at noon next runs at 12:15"
    );
}

#[test]
fn a_daily_job_names_tomorrow_when_today_has_gone() {
    // 09:00 daily, asked at noon: the next one is not today.
    let (_d, shell) = vault("0 9 * * * next-file");
    let r = rows(&shell, NOON);
    assert_eq!(r[0].next.as_deref(), Some("tomorrow 09:00"));
}

#[test]
fn a_job_later_today_is_just_the_clock() {
    let (_d, shell) = vault("30 14 * * * next-file");
    let r = rows(&shell, NOON);
    assert_eq!(r[0].next.as_deref(), Some("14:30"));
}

#[test]
fn a_job_that_cannot_come_round_says_so_rather_than_lying() {
    // 31 February: parses, matches nothing, and a UI that shows a
    // confident next-run time for it is worse than one that admits it.
    let (_d, shell) = vault("0 9 31 2 * next-file");
    let r = rows(&shell, NOON);
    assert_eq!(r[0].next, None);
}

#[test]
fn a_command_nothing_answers_to_is_flagged() {
    // The failure that is invisible in a file: the block is valid cron
    // and the name is a typo, so it fires forever into nothing.
    let (_d, shell) = vault("*/5 * * * * next-fiel");
    let r = rows(&shell, NOON);
    assert!(!r[0].known, "a misspelled command was reported as fine");

    let (_d2, ok) = vault("*/5 * * * * next-file");
    assert!(rows(&ok, NOON)[0].known, "a real command was flagged");
}

#[test]
fn a_job_that_has_run_this_session_says_when() {
    let (_d, mut shell) = vault("0 10 * * * next-file");
    let mut app = ModalApp::new(InputMode::Doom);
    // 10:00 on the 9th — a fire recorded earlier, whose clock time is
    // what the row reports.
    app.cron_tick_at(&mut shell, 0, 10, 9, 8, 0);
    let r = app.cron_rows_at(&shell, NOON);
    assert_eq!(r[0].last.as_deref(), Some("10:00"));
}

#[test]
fn a_job_that_has_not_run_says_nothing_rather_than_never() {
    let (_d, shell) = vault("0 10 * * * next-file");
    assert_eq!(rows(&shell, NOON)[0].last, None);
}

#[test]
fn two_jobs_running_the_same_command_are_two_jobs() {
    // Seen on :1 while checking the pane: a `*/15 next-file` that had
    // just fired and an `0 9 31 2 * next-file` that can never fire
    // both showed "last 10:00", because what ran was recorded against
    // the *command*. Two lines in the block are two jobs even when
    // they say the same words.
    let (_d, mut shell) = vault("*/15 * * * * next-file\n0 9 31 2 * next-file");
    let mut app = ModalApp::new(InputMode::Doom);
    // Noon: the quarter-hourly one is due, the February one is not.
    app.cron_tick_at(&mut shell, 0, 12, 6, 8, 4);
    let r = app.cron_rows_at(&shell, NOON);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].last.as_deref(), Some("12:00"), "the one that ran");
    assert_eq!(
        r[1].last, None,
        "a job that has never run reported someone else's firing"
    );
}

#[test]
fn the_same_command_on_two_schedules_both_fire() {
    // The other half of the same key: the once-a-minute guard is per
    // job, so two jobs due together both run.
    let (_d, mut shell) = vault("0 12 * * * next-file\n*/15 * * * * next-file");
    let mut app = ModalApp::new(InputMode::Doom);
    let ran = app.cron_tick_at(&mut shell, 0, 12, 6, 8, 4);
    assert_eq!(
        ran.len(),
        2,
        "one job's firing suppressed the other: {ran:?}"
    );
}
