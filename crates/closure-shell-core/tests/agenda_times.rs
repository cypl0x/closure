//! The agenda says what time a thing is, and puts the morning first.
//!
//! `span_of` reads `<2026-08-12 Wed 10:00-11:00>` and the agenda threw
//! the time away: every entry on a day was "2026-08-12", sorted by
//! title, so a nine o'clock stand-up could sit below a four o'clock
//! review because "review" sorts before "stand-up". An agenda that
//! cannot order a day is a list of things, not a plan for one.
//!
//! Sorting is the load-bearing half and the display is the visible one,
//! which is why both are here. A day with no times keeps sorting by
//! title — that was right before and stays right, because two untimed
//! items have no order except the one you can read.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const DAY: &str = "\
* Review
SCHEDULED: <2026-08-12 Wed 16:00-17:00>
:PROPERTIES:
:ID: 01AGENDATIME0000000001
:END:
* Stand-up
SCHEDULED: <2026-08-12 Wed 09:00-09:15>
:PROPERTIES:
:ID: 01AGENDATIME0000000002
:END:
* Errand
SCHEDULED: <2026-08-12 Wed>
:PROPERTIES:
:ID: 01AGENDATIME0000000003
:END:
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_morning_comes_before_the_afternoon() {
    let (_d, shell, app) = app(DAY);
    let titles: Vec<String> = app
        .agenda_rows(&shell)
        .into_iter()
        .map(|(_, title, _)| title)
        .collect();
    let standup = titles
        .iter()
        .position(|t| t == "Stand-up")
        .expect("stand-up");
    let review = titles.iter().position(|t| t == "Review").expect("review");
    assert!(
        standup < review,
        "nine o'clock sorted below four o'clock: {titles:?}"
    );
}

#[test]
fn the_time_is_shown_beside_the_day() {
    let (_d, shell, app) = app(DAY);
    let rows = app.agenda_rows(&shell);
    let (when, _, _) = rows
        .iter()
        .find(|(_, t, _)| t == "Stand-up")
        .expect("stand-up");
    assert!(
        when.contains("09:00"),
        "the agenda did not say what time it is: {when}"
    );
}

#[test]
fn an_untimed_entry_still_says_its_day_and_nothing_more() {
    // A date with no time is not "00:00". Showing one would be the
    // agenda inventing a commitment the file never made.
    let (_d, shell, app) = app(DAY);
    let rows = app.agenda_rows(&shell);
    let (when, _, _) = rows.iter().find(|(_, t, _)| t == "Errand").expect("errand");
    assert_eq!(when.trim(), "2026-08-12", "{when}");
}

#[test]
fn a_day_with_no_times_at_all_still_sorts_by_title() {
    // Unchanged behaviour, asserted so the new sort cannot quietly
    // reorder the case it was not written for.
    let src = "\
* Beta
SCHEDULED: <2026-08-12 Wed>
:PROPERTIES:
:ID: 01AGENDATIME0000000004
:END:
* Alpha
SCHEDULED: <2026-08-12 Wed>
:PROPERTIES:
:ID: 01AGENDATIME0000000005
:END:
";
    let (_d, shell, app) = app(src);
    let titles: Vec<String> = app
        .agenda_rows(&shell)
        .into_iter()
        .map(|(_, t, _)| t)
        .collect();
    assert_eq!(titles, vec!["Alpha".to_owned(), "Beta".to_owned()]);
}

#[test]
fn a_timed_entry_comes_before_an_untimed_one_on_the_same_day() {
    // Something at nine is a thing you have to be somewhere for;
    // "sometime Wednesday" is not. The one with a claim on the clock
    // goes first.
    let (_d, shell, app) = app(DAY);
    let titles: Vec<String> = app
        .agenda_rows(&shell)
        .into_iter()
        .map(|(_, t, _)| t)
        .collect();
    let errand = titles.iter().position(|t| t == "Errand").expect("errand");
    let review = titles.iter().position(|t| t == "Review").expect("review");
    assert!(review < errand, "{titles:?}");
}

// `agenda_rows` is not the list the gpui window paints — it calls
// `agenda_context`, which is a second path to the same entries. The
// first version of this work taught only the first one, the window's
// order came out right, and the times were still missing from the
// pane: a gap a core test cannot see and a screenshot can. So the path
// the window actually uses is asserted here too.

#[test]
fn the_context_rows_the_window_paints_carry_the_time() {
    let (_d, shell, app) = app(DAY);
    let rows = app.agenda_context(&shell, "2026-08-12");
    let standup = rows
        .iter()
        .find(|r| r.title == "Stand-up")
        .expect("stand-up");
    assert_eq!(standup.time.as_deref(), Some("09:00"));
}

#[test]
fn an_untimed_context_row_is_given_no_time() {
    let (_d, shell, app) = app(DAY);
    let rows = app.agenda_context(&shell, "2026-08-12");
    let errand = rows.iter().find(|r| r.title == "Errand").expect("errand");
    assert_eq!(errand.time, None);
}

#[test]
fn the_context_rows_are_in_the_same_order_as_the_agenda() {
    // Two paths to one list is the shape that lets them disagree. If
    // they ever do, the window and every other shell are showing
    // different days.
    let (_d, shell, app) = app(DAY);
    let a: Vec<String> = app
        .agenda_rows(&shell)
        .into_iter()
        .map(|(_, t, _)| t)
        .collect();
    let b: Vec<String> = app
        .agenda_context(&shell, "2026-08-12")
        .into_iter()
        .map(|r| r.title)
        .collect();
    assert_eq!(a, b);
}
