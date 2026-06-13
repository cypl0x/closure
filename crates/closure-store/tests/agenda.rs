//! Vault agenda: collect SCHEDULED/DEADLINE headlines, sorted by date.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::{AgendaKind, Vault};
use tempfile::TempDir;

fn vault_with(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (n, b) in files {
        fs::write(dir.path().join(n), b).expect("write");
    }
    dir
}

#[test]
fn agenda_collects_scheduled_and_deadline_sorted_by_date() {
    let td = vault_with(&[(
        "a.org",
        "* TODO Later\nDEADLINE: <2026-06-20 Sat>\n\
         * TODO Soon\nSCHEDULED: <2026-06-13 Fri>\n\
         * No planning\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let ag = v.agenda();
    assert_eq!(ag.len(), 2, "only planned headlines");
    assert_eq!(ag[0].title, "Soon", "earliest first");
    assert_eq!(ag[0].kind, AgendaKind::Scheduled);
    assert_eq!(ag[0].date, "2026-06-13");
    assert_eq!(ag[1].title, "Later");
    assert_eq!(ag[1].kind, AgendaKind::Deadline);
}

#[test]
fn agenda_filters_to_on_or_before() {
    let td = vault_with(&[(
        "a.org",
        "* A\nSCHEDULED: <2026-06-13 Fri>\n* B\nSCHEDULED: <2026-06-30 Tue>\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let upto = v.agenda_until("2026-06-20");
    assert_eq!(upto.len(), 1);
    assert_eq!(upto[0].title, "A");
}

#[test]
fn agenda_empty_without_planning() {
    let td = vault_with(&[("a.org", "* Plain\n* Another\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.agenda().is_empty());
}
