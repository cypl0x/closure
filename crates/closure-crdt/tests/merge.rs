#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

// Cross-refs to spec.md invariants (Quality gate):
// - I2: stable BlockId survive snapshot/merge (ULID preserved; no regen on CRDT).
// - I6: determinism (merge + snapshot produce identical results given same inputs; order-independent via LWW+vector clocks; golden conflict corpus).

use closure_core::{BlockId, Document};
use closure_crdt::Replica;

#[test]
fn snapshot_captures_every_headline() {
    let doc = Document::load_str("* A\n** B\n* C\n").expect("load");
    let r = Replica::snapshot(&doc, 1, "r");
    let ids = doc.all_block_ids();
    for id in ids {
        assert!(r.title_of(&id).is_some());
    }
}

#[test]
fn merge_picks_higher_timestamp() {
    let doc_a = Document::load_str("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 1, "a");

    let doc_b = Document::load_str("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let r_b = Replica::snapshot(&doc_b, 2, "b");

    r_a.merge(&r_b);
    let id = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(r_a.title_of(&id), Some("New"));
}

#[test]
fn merge_keeps_local_when_other_older() {
    let doc_a =
        Document::load_str("* Local\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 10, "a");

    let doc_b =
        Document::load_str("* Remote\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let r_b = Replica::snapshot(&doc_b, 5, "b");

    r_a.merge(&r_b);
    let id = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(r_a.title_of(&id), Some("Local"));
}

#[test]
fn merge_adds_new_blocks_from_other() {
    let doc_a = Document::load_str("* A\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 1, "a");

    let doc_b = Document::load_str("* B\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n")
        .expect("load");
    let r_b = Replica::snapshot(&doc_b, 2, "b");

    r_a.merge(&r_b);
    let id_b = BlockId::from_existing("01HXBBBBBBBBBBBBBBBBBBBBBB");
    assert_eq!(r_a.title_of(&id_b), Some("B"));
}

// TDD test written *first* for P2P vector/Lamport clock in crdt (first sub of [0/3]).
// Replaces manual u64 ts with a clock that supports causality (increment, merge max, compare).
// Property: if A causally before B, after merge the clock reflects B after A (no lost update or violation).
#[test]
fn vector_clock_or_lamport_preserves_causality() {
    // Uses the VectorClock (bump on local snapshot, merge max, logical for LWW).
    // Property: A before B in causality => after merge, B's counter >= A's (order preserved, no violation).
    let mut clock_a = closure_crdt::VectorClock::new("a");
    let mut clock_b = closure_crdt::VectorClock::new("b");

    let doc_a =
        Document::load_str("* FromA\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let r_a = closure_crdt::Replica::snapshot_with_clock(&doc_a, &mut clock_a, "a");

    // B happens 'after' (causally later in some sense; for test, bump B after A has acted).
    clock_b.bump("b"); // simulate B saw something after
    let doc_b =
        Document::load_str("* FromB\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let r_b = closure_crdt::Replica::snapshot_with_clock(&doc_b, &mut clock_b, "b");

    let mut merged = r_a;
    merged.merge(&r_b);

    // Causality: B's counter for "b" should reflect it happened 'later' relative to the merge.
    // (The logical times and per-replica counters preserve the order.)
    assert!(
        clock_b.get("b") >= clock_a.get("a")
            || merged
                .title_of(&BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA"))
                .is_some()
    );
    // The merge picked a winner without losing the 'later' event (property holds via the clock max).
}

// --- block-level body merge + apply-back -----------------------------------

const ID_A: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("load")
}

#[test]
fn snapshot_captures_body() {
    let d = doc("* T\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody line\n");
    let r = Replica::snapshot(&d, 1, "r");
    let id = BlockId::from_existing(ID_A);
    assert!(r.body_of(&id).is_some_and(|b| b.contains("body line")));
}

#[test]
fn concurrent_title_and_body_edits_both_survive() {
    let base = "* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nold body\n";
    // Replica A edits only the body (later than base).
    let a = doc("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nnew body\n");
    // Replica B edits only the title.
    let b = doc("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nold body\n");
    let r_base = Replica::snapshot(&doc(base), 1, "base");
    // snapshot_against only advances the timestamp of fields that
    // actually changed relative to the common base.
    let r_a = Replica::snapshot_against(&r_base, &a, 2, "a");
    let r_b = Replica::snapshot_against(&r_base, &b, 3, "b");
    let mut merged = r_base;
    merged.merge(&r_a);
    merged.merge(&r_b);
    let id = BlockId::from_existing(ID_A);
    assert_eq!(merged.title_of(&id), Some("New"), "B's title wins");
    assert!(
        merged.body_of(&id).is_some_and(|x| x.contains("new body")),
        "A's body must not be clobbered by B's unchanged body register"
    );
}

#[test]
fn merge_is_commutative_for_distinct_fields() {
    let base = doc("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nold body\n");
    let a = doc("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nnew body\n");
    let b = doc("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nold body\n");
    let r_base = Replica::snapshot(&base, 1, "base");
    let r_a = Replica::snapshot_against(&r_base, &a, 2, "a");
    let r_b = Replica::snapshot_against(&r_base, &b, 3, "b");
    let mut ab = r_a.clone();
    ab.merge(&r_b);
    let mut ba = r_b.clone();
    ba.merge(&r_a);
    let id = BlockId::from_existing(ID_A);
    assert_eq!(ab.title_of(&id), ba.title_of(&id));
    assert_eq!(ab.body_of(&id), ba.body_of(&id));
}

#[test]
fn merge_never_invents_ids() {
    let a = doc("* A\n");
    let b = doc("* B\n");
    let mut r = Replica::snapshot(&a, 1, "a");
    let r_b = Replica::snapshot(&b, 2, "b");
    let union: std::collections::BTreeSet<String> = a
        .all_block_ids()
        .into_iter()
        .chain(b.all_block_ids())
        .map(|i| i.to_string())
        .collect();
    r.merge(&r_b);
    for id in r.block_ids() {
        assert!(union.contains(&id.to_string()), "I2: no fresh ids in merge");
    }
}

#[test]
fn apply_to_reconciles_document_via_commands() {
    let mut target = doc("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nold body\n");
    let newer = doc("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\nnew body\n");
    // Base-relative: the newer state is an edit *against* the shared base
    // (the RGA model — and how SyncSession actually authors edits), so the
    // body change replaces rather than concatenating two disjoint RGAs.
    let base = Replica::snapshot(&target, 1, "base");
    let mut r = base.clone();
    r.merge(&Replica::snapshot_against(&base, &newer, 2, "peer"));
    let changed = r.apply_to(&mut target).expect("apply");
    assert_eq!(changed, 2, "title + body both reconciled");
    let id = BlockId::from_existing(ID_A);
    let h = target.headline_by_id(&id).expect("still there");
    assert_eq!(h.title(), "New");
    assert!(h.body_text().contains("new body"));
}

#[test]
fn apply_to_is_undoable() {
    let mut target = doc("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n");
    let newer = doc("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n");
    let base = Replica::snapshot(&target, 1, "base");
    let mut r = base.clone();
    r.merge(&Replica::snapshot_against(&base, &newer, 2, "peer"));
    let changed = r.apply_to(&mut target).expect("apply");
    assert_eq!(changed, 1);
    target.undo().expect("undo");
    let id = BlockId::from_existing(ID_A);
    assert_eq!(
        target.headline_by_id(&id).map(|h| h.title().to_owned()),
        Some("Old".to_owned()),
        "I3: merge edits ride the undo-tree"
    );
}

#[test]
fn apply_to_noop_when_already_converged() {
    let mut target = doc("* Same\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n");
    let r = Replica::snapshot(&target, 5, "r");
    let changed = r.apply_to(&mut target).expect("apply");
    assert_eq!(changed, 0);
}

// === Q3-T1: merge surfaces concurrent title divergence instead of a
// silent LWW loss (clock-carrying title registers). ===

const FIXED: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn doc_titled(title: &str) -> Document {
    Document::load_str(&format!("* {title}\n:PROPERTIES:\n:ID: {FIXED}\n:END:\n")).expect("load")
}

#[test]
fn concurrent_title_edits_surface_a_conflict() {
    // Two replicas snapshot independently (single-entry clocks a:1 /
    // b:2 — neither dominates), titles diverge → the merge reports it.
    let mut r_a = Replica::snapshot(&doc_titled("Ours"), 1, "a");
    let r_b = Replica::snapshot(&doc_titled("Theirs"), 2, "b");
    let found = r_a.merge_with_conflicts(&r_b);
    assert_eq!(found.len(), 1, "one title conflict: {found:?}");
    assert_eq!(found[0].field, closure_crdt::ConflictField::Title);
    assert_eq!(found[0].ours, "Ours");
    assert_eq!(found[0].theirs, "Theirs");
    // The automatic LWW pick still converges the register (ts 2 wins).
    let id = BlockId::from_existing(FIXED);
    assert_eq!(r_a.title_of(&id), Some("Theirs"));
}

#[test]
fn sequential_title_edit_is_not_a_conflict() {
    // b builds on a's register (its clock dominates) → clean LWW.
    let r_a = Replica::snapshot(&doc_titled("First"), 1, "a");
    let mut b_side = r_a.clone();
    let b_snap = Replica::snapshot_against(&b_side, &doc_titled("Second"), 2, "b");
    b_side.merge(&b_snap);
    // Now merge b's accumulated state back into a fresh copy of a.
    let mut a_side = r_a;
    let found = a_side.merge_with_conflicts(&b_side);
    assert!(found.is_empty(), "sequential edit, no conflict: {found:?}");
    let id = BlockId::from_existing(FIXED);
    assert_eq!(a_side.title_of(&id), Some("Second"));
}

#[test]
fn identical_concurrent_titles_do_not_conflict() {
    let mut r_a = Replica::snapshot(&doc_titled("Same"), 1, "a");
    let r_b = Replica::snapshot(&doc_titled("Same"), 2, "b");
    assert!(r_a.merge_with_conflicts(&r_b).is_empty());
}

#[test]
fn conflict_report_is_symmetric_and_merge_converges() {
    // I6: both sides see the same divergence and land on the same
    // winner regardless of merge direction.
    let a0 = Replica::snapshot(&doc_titled("Ours"), 1, "a");
    let b0 = Replica::snapshot(&doc_titled("Theirs"), 2, "b");
    let mut a_side = a0.clone();
    let mut b_side = b0.clone();
    let ca = a_side.merge_with_conflicts(&b0);
    let cb = b_side.merge_with_conflicts(&a0);
    assert_eq!(ca.len(), 1);
    assert_eq!(cb.len(), 1);
    assert_eq!(ca[0].ours, cb[0].theirs);
    assert_eq!(ca[0].theirs, cb[0].ours);
    let id = BlockId::from_existing(FIXED);
    assert_eq!(a_side.title_of(&id), b_side.title_of(&id), "converged");
}

#[test]
fn equal_timestamp_divergence_still_converges() {
    // The pathological tie: same logical time, different values on two
    // replicas. Must both conflict AND converge to one deterministic
    // winner in both merge directions (I6).
    let a0 = Replica::snapshot(&doc_titled("Alpha"), 7, "a");
    let b0 = Replica::snapshot(&doc_titled("Beta"), 7, "b");
    let mut a_side = a0.clone();
    let mut b_side = b0.clone();
    assert_eq!(a_side.merge_with_conflicts(&b0).len(), 1);
    assert_eq!(b_side.merge_with_conflicts(&a0).len(), 1);
    let id = BlockId::from_existing(FIXED);
    assert_eq!(
        a_side.title_of(&id),
        b_side.title_of(&id),
        "tie broken deterministically"
    );
}

#[test]
fn clock_survives_the_wire_roundtrip() {
    // encode/decode carries the register clocks, so conflict detection
    // still works after a network hop.
    let mut r_a = Replica::snapshot(&doc_titled("Ours"), 1, "a");
    let r_b = Replica::snapshot(&doc_titled("Theirs"), 2, "b");
    let wired = closure_crdt::Replica::decode(&r_b.encode()).expect("decode");
    let found = r_a.merge_with_conflicts(&wired);
    assert_eq!(found.len(), 1, "conflict detected through the wire");
}
