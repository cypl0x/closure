//! A deadline that is coming says how far off it is.
//!
//! `agenda_context` filtered nothing and marked only today and overdue,
//! so a deadline eleven days out looked exactly like one due in an
//! hour: same row, same colour, same silence about which. Now that
//! deadlines are mentioned before they land, the row has to say how
//! long there is — otherwise the warning period just makes the agenda
//! longer without making it more useful.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Tax return
DEADLINE: <2026-08-20 Thu>
:PROPERTIES:
:ID: 01DEADNOTICE00000001
:END:
* Bin day
DEADLINE: <2026-08-13 Thu -2d>
:PROPERTIES:
:ID: 01DEADNOTICE00000002
:END:
* A plan
SCHEDULED: <2026-08-12 Wed>
:PROPERTIES:
:ID: 01DEADNOTICE00000003
:END:
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn row(app: &ModalApp, shell: &Shell, title: &str) -> closure_shell_core::AgendaRow {
    app.agenda_context(shell, "2026-08-12")
        .into_iter()
        .find(|r| r.title == title)
        .unwrap_or_else(|| panic!("no row for {title}"))
}

#[test]
fn a_deadline_days_away_says_how_many() {
    let (_d, shell, app) = app();
    assert_eq!(row(&app, &shell, "Tax return").days_left, Some(8));
}

#[test]
fn tomorrow_is_one_day() {
    let (_d, shell, app) = app();
    assert_eq!(row(&app, &shell, "Bin day").days_left, Some(1));
}

#[test]
fn a_plan_has_no_days_left_because_it_is_not_a_deadline() {
    // A scheduled date is when you meant to start, not a countdown.
    let (_d, shell, app) = app();
    assert_eq!(row(&app, &shell, "A plan").days_left, None);
}

#[test]
fn an_overdue_deadline_is_still_overdue_not_negative() {
    // Past the date, `is_overdue` is the thing to say. A negative
    // countdown would read as "minus three days left", which is not
    // how anybody thinks about a missed deadline.
    let (_d, shell, app) = app();
    let r = app
        .agenda_context(&shell, "2026-08-23")
        .into_iter()
        .find(|r| r.title == "Tax return")
        .expect("still listed");
    assert!(r.is_overdue);
    assert_eq!(r.days_left, None);
}

#[test]
fn today_is_zero_days_and_not_overdue() {
    let (_d, shell, app) = app();
    let r = app
        .agenda_context(&shell, "2026-08-20")
        .into_iter()
        .find(|r| r.title == "Tax return")
        .expect("listed");
    assert!(r.is_today);
    assert!(!r.is_overdue);
    assert_eq!(r.days_left, Some(0));
}
