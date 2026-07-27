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

// === Q5-O3: vault-wide clock report (clocked minutes per headline). ===

#[test]
fn clock_minutes_aggregates_per_headline() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(
        dir.path().join("t.org"),
        "* Deep work\n:LOGBOOK:\nCLOCK: [2024-01-01 Mon 09:00]--[2024-01-01 Mon 10:30] =>  1:30\nCLOCK: [2024-01-02 Tue 09:00]--[2024-01-02 Tue 09:45] =>  0:45\n:END:\n* Idle\n",
    )
    .expect("write");
    let v = closure_store::Vault::open(dir.path()).expect("open");
    let report = v.clock_minutes();
    assert_eq!(report.len(), 1, "only clocked headlines appear: {report:?}");
    assert_eq!(report[0].0, "Deep work");
    assert_eq!(report[0].1, 135, "1:30 + 0:45");
}

#[test]
fn clock_minutes_ignores_open_clocks() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(
        dir.path().join("t.org"),
        "* Running\n:LOGBOOK:\nCLOCK: [2024-01-01 Mon 09:00]\n:END:\n",
    )
    .expect("write");
    let v = closure_store::Vault::open(dir.path()).expect("open");
    assert!(
        v.clock_minutes().is_empty(),
        "open clock has no minutes yet"
    );
}
