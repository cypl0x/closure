//! S2: in-memory loopback transport — two peers exchange replicas over
//! a shared in-process channel and converge. Hermetic stand-in for a
//! network link (what the iroh transport mirrors).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_sync::{LoopbackPair, SyncSession};

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

// Shared base: two blocks. Peer A edits Alpha, peer B edits Beta.
const BASE: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n\
                    * Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";
const A_EDIT: &str = "* Alpha v2\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n\
                      * Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";
const B_EDIT: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n\
                      * Beta v2\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";

#[test]
fn one_sync_round_converges_both_documents() {
    let mut a = SyncSession::new("a");
    let mut b = SyncSession::new("b");
    // Both start from the shared base, then each edits a different block.
    a.record_local(&doc(BASE));
    b.record_local(&doc(BASE));
    a.record_local(&doc(A_EDIT)); // Alpha -> "Alpha v2"
    b.record_local(&doc(B_EDIT)); // Beta  -> "Beta v2"

    let mut link = LoopbackPair::new();
    link.sync_round(&mut a, &mut b);

    // Apply each converged session to its own Document copy.
    let mut da = doc(A_EDIT);
    let mut db = doc(B_EDIT);
    a.apply_to(&mut da).expect("apply a");
    b.apply_to(&mut db).expect("apply b");

    assert_eq!(da.source(), db.source(), "both documents converge");
    assert!(da.source().contains("Alpha v2") && da.source().contains("Beta v2"));
}

#[test]
fn empty_channel_pull_is_a_noop() {
    let mut a = SyncSession::new("a");
    a.record_local(&doc(BASE));
    let before = a.block_ids().count();
    let mut link = LoopbackPair::new();
    link.pull_a(&mut a); // nothing pushed by B yet
    let after = a.block_ids().count();
    assert_eq!(before, after, "pull with empty channel changes nothing");
}
