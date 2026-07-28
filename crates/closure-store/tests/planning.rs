//! Planning a headline keeps its id (I2).
//!
//! `Vault::set_planning` writes the planning line where org puts it —
//! between the header and the property drawer. Until the parser read a
//! drawer through that line, scheduling a note orphaned its `:ID:`: the
//! next open found none, minted a fresh ULID, and every `id:` link into
//! the note, the remembered `last_place` and any CRDT merge addressing
//! that block pointed at something that no longer existed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;

const SRC: &str = "* TODO Ship it\n:PROPERTIES:\n:ID: 01HQSTOREPLAN000000000001\n:END:\nbody\n";

#[test]
fn scheduling_a_headline_keeps_its_id_across_a_reopen() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let mut vault = Vault::open(dir.path()).expect("open");
    let id = BlockId::from_existing("01HQSTOREPLAN000000000001");

    vault
        .set_planning(&id, Some("<2026-07-29 Wed>"), None, None)
        .expect("schedule");

    let reopened = Vault::open(dir.path()).expect("reopen");
    let (headline, _) = reopened.find_by_id(&id).expect("the id still resolves");
    assert_eq!(headline.title(), "Ship it");
    assert_eq!(headline.scheduled(), Some("<2026-07-29 Wed>"));
}

#[test]
fn a_deadline_leaves_the_schedule_alone_when_both_are_passed() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let mut vault = Vault::open(dir.path()).expect("open");
    let id = BlockId::from_existing("01HQSTOREPLAN000000000001");

    vault
        .set_planning(&id, Some("<2026-07-29 Wed>"), None, None)
        .expect("schedule");
    vault
        .set_planning(
            &id,
            Some("<2026-07-29 Wed>"),
            Some("<2026-08-01 Sat>"),
            None,
        )
        .expect("deadline");

    let (headline, _) = vault.find_by_id(&id).expect("found");
    assert_eq!(headline.scheduled(), Some("<2026-07-29 Wed>"));
    assert_eq!(headline.deadline(), Some("<2026-08-01 Sat>"));
}

#[test]
fn clearing_planning_leaves_the_headline_and_its_id() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let mut vault = Vault::open(dir.path()).expect("open");
    let id = BlockId::from_existing("01HQSTOREPLAN000000000001");

    vault
        .set_planning(&id, Some("<2026-07-29 Wed>"), None, None)
        .expect("schedule");
    vault.set_planning(&id, None, None, None).expect("clear");

    let text = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!text.contains("SCHEDULED:"), "{text}");
    assert!(text.contains(":ID: 01HQSTOREPLAN000000000001"), "{text}");
    assert!(vault.find_by_id(&id).is_some());
}
