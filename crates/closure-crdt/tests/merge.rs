#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Document};
use closure_crdt::Replica;

#[test]
fn snapshot_captures_every_headline() {
    let doc = Document::load_str("* A\n** B\n* C\n").expect("load");
    let r = Replica::snapshot(&doc, 1);
    let ids = doc.all_block_ids();
    for id in ids {
        assert!(r.title_of(&id).is_some());
    }
}

#[test]
fn merge_picks_higher_timestamp() {
    let doc_a = Document::load_str("* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 1);

    let doc_b = Document::load_str("* New\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let r_b = Replica::snapshot(&doc_b, 2);

    r_a.merge(&r_b);
    let id = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(r_a.title_of(&id), Some("New"));
}

#[test]
fn merge_keeps_local_when_other_older() {
    let doc_a =
        Document::load_str("* Local\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 10);

    let doc_b =
        Document::load_str("* Remote\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
            .expect("load");
    let r_b = Replica::snapshot(&doc_b, 5);

    r_a.merge(&r_b);
    let id = BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(r_a.title_of(&id), Some("Local"));
}

#[test]
fn merge_adds_new_blocks_from_other() {
    let doc_a = Document::load_str("* A\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n")
        .expect("load");
    let mut r_a = Replica::snapshot(&doc_a, 1);

    let doc_b = Document::load_str("* B\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n")
        .expect("load");
    let r_b = Replica::snapshot(&doc_b, 1);

    r_a.merge(&r_b);
    assert_eq!(
        r_a.title_of(&BlockId::from_existing("01HXAAAAAAAAAAAAAAAAAAAAAA")),
        Some("A")
    );
    assert_eq!(
        r_a.title_of(&BlockId::from_existing("01HXBBBBBBBBBBBBBBBBBBBBBB")),
        Some("B")
    );
}
