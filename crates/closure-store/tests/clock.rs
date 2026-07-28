//! Q3-V3 — clocking in and out.
//!
//! `clock_minutes()` has been able to *read* `CLOCK:` lines since Q5-O3
//! and nothing could write one, so the only way to track time in a
//! closure vault was to type the logbook by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* TODO Write the parser
:PROPERTIES:
:ID: 01HQCLOCK00000000000000001
:END:
notes about it
* TODO Something else
:PROPERTIES:
:ID: 01HQCLOCK00000000000000002
:END:
";

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, vault)
}

fn source(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("notes.org")).expect("read")
}

#[test]
fn v3_clocking_in_opens_a_logbook_entry() {
    let (dir, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");

    vault.clock_in(&id, "2026-07-28 09:15").expect("clock in");

    let src = source(&dir);
    assert!(src.contains(":LOGBOOK:"), "{src}");
    assert!(src.contains("CLOCK: [2026-07-28 Tue 09:15]"), "{src}");
    assert!(src.contains("notes about it"), "the body survives: {src}");
}

#[test]
fn v3_clocking_out_closes_it_with_the_duration() {
    let (dir, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");

    vault.clock_in(&id, "2026-07-28 09:15").expect("in");
    vault.clock_out(&id, "2026-07-28 10:45").expect("out");

    let src = source(&dir);
    assert!(
        src.contains("CLOCK: [2026-07-28 Tue 09:15]--[2026-07-28 Tue 10:45] =>  1:30"),
        "{src}"
    );
}

#[test]
fn v3_the_open_clock_is_findable() {
    let (_d, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");
    assert_eq!(vault.running_clock(), None);

    vault.clock_in(&id, "2026-07-28 09:15").expect("in");
    let (running, started) = vault.running_clock().expect("a clock is running");
    assert_eq!(running, id.as_str());
    assert_eq!(started, "2026-07-28 Tue 09:15");

    vault.clock_out(&id, "2026-07-28 10:45").expect("out");
    assert_eq!(vault.running_clock(), None);
}

#[test]
fn v3_clocking_in_elsewhere_closes_the_clock_that_was_running() {
    let (dir, mut vault) = vault();
    let first = BlockId::from_existing("01HQCLOCK00000000000000001");
    let second = BlockId::from_existing("01HQCLOCK00000000000000002");

    vault.clock_in(&first, "2026-07-28 09:15").expect("in");
    vault
        .clock_in(&second, "2026-07-28 10:00")
        .expect("in again");

    let src = source(&dir);
    assert!(
        src.contains("CLOCK: [2026-07-28 Tue 09:15]--[2026-07-28 Tue 10:00] =>  0:45"),
        "the first one was closed: {src}"
    );
    let (running, _) = vault.running_clock().expect("the second is running");
    assert_eq!(running, second.as_str());
}

#[test]
fn v3_cancelling_removes_the_open_entry() {
    let (dir, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");

    vault.clock_in(&id, "2026-07-28 09:15").expect("in");
    vault.clock_cancel(&id).expect("cancel");

    let src = source(&dir);
    assert!(!src.contains("CLOCK:"), "no entry left: {src}");
    assert_eq!(vault.running_clock(), None);
}

#[test]
fn v3_clocking_out_with_nothing_running_is_an_error_not_a_mess() {
    let (dir, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");
    assert!(vault.clock_out(&id, "2026-07-28 10:45").is_err());
    assert!(!source(&dir).contains("CLOCK:"));
}

#[test]
fn v3_clocked_time_adds_up_per_headline() {
    let (_d, mut vault) = vault();
    let id = BlockId::from_existing("01HQCLOCK00000000000000001");

    vault.clock_in(&id, "2026-07-28 09:15").expect("in");
    vault.clock_out(&id, "2026-07-28 10:45").expect("out");
    vault.clock_in(&id, "2026-07-28 13:00").expect("in");
    vault.clock_out(&id, "2026-07-28 13:30").expect("out");

    let totals = vault.clock_minutes();
    let total = totals
        .iter()
        .find(|(t, _)| t == "Write the parser")
        .map(|(_, m)| *m);
    assert_eq!(total, Some(120), "90 + 30: {totals:?}");
}
