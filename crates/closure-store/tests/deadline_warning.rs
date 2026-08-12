//! A deadline is mentioned before it lands.
//!
//! `agenda_until(today)` kept entries whose date was `<= today`, so a
//! DEADLINE surfaced on the day it was due and not a moment earlier —
//! the one moment a warning has stopped being useful. Org warns for
//! `org-deadline-warning-days` beforehand, 14 by default, and a `-2d`
//! cooldown on the timestamp narrows that for one task.
//!
//! So the cooldown was not merely unread: it had nothing to narrow.
//! Both halves are here because either alone is untestable — a warning
//! period with no way to shorten it is org's default and nothing more,
//! and a cooldown with no period to cut is arithmetic nobody runs.
//!
//! SCHEDULED is deliberately untouched. "I planned to start this on
//! Tuesday" is not a thing to be warned about in advance; it appears on
//! its day, which is what a plan is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

fn vault(src: &str) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

const PLAIN: &str = "\
* Tax return
DEADLINE: <2026-08-20 Thu>
:PROPERTIES:
:ID: 01DEADLINEWARN000001
:END:
";

const NARROWED: &str = "\
* Bin day
DEADLINE: <2026-08-20 Thu -2d>
:PROPERTIES:
:ID: 01DEADLINEWARN000002
:END:
";

fn titles(v: &Vault, today: &str) -> Vec<String> {
    v.agenda_until(today).into_iter().map(|e| e.title).collect()
}

#[test]
fn a_deadline_is_mentioned_before_the_day() {
    let (_d, v) = vault(PLAIN);
    // Eleven days out, inside org's fourteen.
    assert!(titles(&v, "2026-08-09").contains(&"Tax return".to_owned()));
}

#[test]
fn but_not_before_the_warning_period_opens() {
    let (_d, v) = vault(PLAIN);
    // Twenty days out. A to-do list that shows everything shows nothing.
    assert!(titles(&v, "2026-07-31").is_empty(), "warned far too early");
}

#[test]
fn a_cooldown_narrows_the_window() {
    let (_d, v) = vault(NARROWED);
    // Five days out: inside the default fourteen, outside this task's two.
    assert!(
        titles(&v, "2026-08-15").is_empty(),
        "`-2d` asked for two days' notice and got the default"
    );
    // Two days out: exactly what it asked for.
    assert!(titles(&v, "2026-08-18").contains(&"Bin day".to_owned()));
}

#[test]
fn an_overdue_deadline_stays() {
    // Past its date is the case that matters most; a warning period
    // must not become a way for a missed deadline to disappear.
    let (_d, v) = vault(PLAIN);
    assert!(titles(&v, "2026-09-01").contains(&"Tax return".to_owned()));
}

#[test]
fn a_scheduled_date_appears_on_its_day_and_not_before() {
    // Not a deadline. "I planned to start this Tuesday" is not
    // something to be warned about a fortnight out.
    let (_d, v) = vault(
        "* Start the thing\nSCHEDULED: <2026-08-20 Thu>\n\
         :PROPERTIES:\n:ID: 01DEADLINEWARN000003\n:END:\n",
    );
    assert!(titles(&v, "2026-08-15").is_empty(), "warned about a plan");
    assert!(titles(&v, "2026-08-20").contains(&"Start the thing".to_owned()));
}
