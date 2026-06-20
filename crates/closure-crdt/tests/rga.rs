//! C2b: the body RGA converges char-level. Concurrent inserts at the
//! same position both survive; merge is order-independent and
//! idempotent; LCS-based `edit_to` preserves untouched characters so a
//! peer's concurrent edit elsewhere is not clobbered.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_crdt::{BodyCrdt, ElemId};

fn base() -> BodyCrdt {
    // "AC" authored by replica "base" (counters 0,1).
    let mut b = BodyCrdt::new();
    let a = ElemId::new(0, "base");
    b.insert_after(None, 'A', a.clone());
    b.insert_after(Some(a), 'C', ElemId::new(1, "base"));
    b
}

#[test]
fn concurrent_inserts_at_same_position_both_survive_and_converge() {
    let a = ElemId::new(0, "base");
    let mut r1 = base();
    r1.insert_after(Some(a.clone()), 'X', ElemId::new(5, "r1"));
    let mut r2 = base();
    r2.insert_after(Some(a), 'Y', ElemId::new(5, "r2"));

    let mut m1 = r1.clone();
    m1.merge(&r2);
    let mut m2 = r2.clone();
    m2.merge(&r1);

    // Convergent + deterministic: both merge orders give the same text.
    assert_eq!(m1.materialize(), m2.materialize());
    let s = m1.materialize();
    assert!(
        s.contains('X') && s.contains('Y'),
        "both edits survive: {s}"
    );
    assert_eq!(s.chars().filter(|&c| c == 'A' || c == 'C').count(), 2);
    // Deterministic tiebreak: equal counter ⇒ higher replica first (desc).
    assert_eq!(s, "AYXC");
}

#[test]
fn merge_is_idempotent() {
    let a = ElemId::new(0, "base");
    let mut r1 = base();
    r1.insert_after(Some(a.clone()), 'X', ElemId::new(5, "r1"));
    let mut r2 = base();
    r2.insert_after(Some(a), 'Y', ElemId::new(5, "r2"));

    let mut once = r1.clone();
    once.merge(&r2);
    let mut twice = once.clone();
    twice.merge(&r2);
    twice.merge(&r1);
    assert_eq!(once.materialize(), twice.materialize());
    assert_eq!(once, twice, "merging the same state again is a no-op");
}

#[test]
fn concurrent_edits_at_different_positions_both_apply() {
    // Shared base "hello world"; one peer edits the start, the other the
    // end. After merge both edits must be present (the LWW register would
    // have dropped one).
    let mut counter = 0u64;
    let shared = BodyCrdt::from_text("hello world", "base", &mut counter);

    let mut p1 = shared.clone();
    let mut c1 = 100u64;
    p1.edit_to("HELLO world", "p1", &mut c1);

    let mut p2 = shared;
    let mut c2 = 200u64;
    p2.edit_to("hello WORLD", "p2", &mut c2);

    let mut merged = p1.clone();
    merged.merge(&p2);
    let mut merged_other = p2.clone();
    merged_other.merge(&p1);

    assert_eq!(merged.materialize(), merged_other.materialize());
    let s = merged.materialize();
    assert_eq!(s, "HELLO WORLD", "both ends' edits survived: {s}");
}

#[test]
fn edit_to_round_trips_plain_text() {
    let mut counter = 0u64;
    let b = BodyCrdt::from_text("line one\nline two\n", "x", &mut counter);
    assert_eq!(b.materialize(), "line one\nline two\n");
}

#[test]
fn edit_to_deletion_tombstones() {
    let mut counter = 0u64;
    let mut b = BodyCrdt::from_text("abcdef", "x", &mut counter);
    b.edit_to("acef", "x", &mut counter); // drop 'b' and 'd'
    assert_eq!(b.materialize(), "acef");
}
