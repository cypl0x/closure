//! S1: a CRDT sync session — snapshot local edits, exchange replicas,
//! merge. Pure + hermetic (no transport, no network).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_sync::SyncSession;

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

/// Extract a sorted (id, title, body) view of a session's merged state
/// for order-independent comparison.
fn state(s: &SyncSession) -> Vec<(String, String, String)> {
    let mut v: Vec<_> = s
        .block_ids()
        .map(|id| {
            (
                id.as_str().to_owned(),
                s.title_of(id).unwrap_or("").to_owned(),
                s.body_of(id).unwrap_or_default(),
            )
        })
        .collect();
    v.sort();
    v
}

const A: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n";
const B: &str = "* Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";

#[test]
fn cross_merge_converges_to_the_same_state() {
    let mut pa = SyncSession::new("a");
    let mut pb = SyncSession::new("b");
    pa.record_local(&doc(A)); // peer A knows block Alpha
    pb.record_local(&doc(B)); // peer B knows block Beta
    // Exchange replicas.
    let (oa, ob) = (pa.outgoing().clone(), pb.outgoing().clone());
    pa.receive(&ob);
    pb.receive(&oa);
    assert_eq!(state(&pa), state(&pb), "peers converge");
    // Both know both blocks.
    assert_eq!(state(&pa).len(), 2);
}

// C2b end-to-end: two peers sharing a base paragraph each edit it at a
// *different* position; after sync both edits survive (the LWW body
// register would have kept only one whole body).
#[test]
fn concurrent_paragraph_edits_at_different_spots_both_survive() {
    use closure_core::BlockId;
    const ID: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";
    let base = "* Note\n:PROPERTIES:\n:ID: 01CCCCCCCCCCCCCCCCCCCCCCCC\n:END:\nhello world\n";
    let edit_a = "* Note\n:PROPERTIES:\n:ID: 01CCCCCCCCCCCCCCCCCCCCCCCC\n:END:\nHELLO world\n";
    let edit_b = "* Note\n:PROPERTIES:\n:ID: 01CCCCCCCCCCCCCCCCCCCCCCCC\n:END:\nhello WORLD\n";

    let mut pa = SyncSession::new("a");
    pa.record_local(&doc(base));
    // Peer B starts from the SAME base RGA (shared element ids).
    let mut pb = SyncSession::new("b");
    pb.receive(&pa.outgoing().clone());

    // Concurrent edits to the same paragraph at different positions.
    pa.record_local(&doc(edit_a)); // start: hello -> HELLO
    pb.record_local(&doc(edit_b)); // end:   world -> WORLD

    // Exchange + merge.
    let (oa, ob) = (pa.outgoing().clone(), pb.outgoing().clone());
    pa.receive(&ob);
    pb.receive(&oa);

    assert_eq!(state(&pa), state(&pb), "peers converge");
    let id = BlockId::from_existing(ID);
    assert_eq!(
        pa.body_of(&id).as_deref(),
        Some("HELLO WORLD\n"),
        "both edits survive char-level (LWW would keep only one)"
    );
}

#[test]
fn merge_is_commutative() {
    let mut pa = SyncSession::new("a");
    pa.record_local(&doc(A));
    let mut pb = SyncSession::new("b");
    pb.record_local(&doc(B));
    let (oa, ob) = (pa.outgoing().clone(), pb.outgoing().clone());

    let mut ab = SyncSession::new("x");
    ab.receive(&oa);
    ab.receive(&ob);
    let mut ba = SyncSession::new("x");
    ba.receive(&ob);
    ba.receive(&oa);
    assert_eq!(state(&ab), state(&ba), "A then B == B then A");
}

#[test]
fn merge_is_idempotent() {
    let mut pa = SyncSession::new("a");
    pa.record_local(&doc(A));
    let once = {
        let mut s = SyncSession::new("x");
        s.receive(pa.outgoing());
        state(&s)
    };
    let twice = {
        let mut s = SyncSession::new("x");
        s.receive(pa.outgoing());
        s.receive(pa.outgoing());
        state(&s)
    };
    assert_eq!(once, twice, "merging twice == once");
}

#[test]
fn apply_to_reconciles_a_document() {
    // A edits Alpha's title; sync to a peer holding the old Alpha doc.
    let mut pa = SyncSession::new("a");
    pa.record_local(&doc(
        "* Alpha v2\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
    ));
    let mut target = doc(A); // old title "Alpha"
    let n = pa.apply_to(&mut target).expect("apply");
    assert!(n >= 1, "at least one edit applied");
    assert!(target.source().contains("Alpha v2"), "title reconciled");
}
