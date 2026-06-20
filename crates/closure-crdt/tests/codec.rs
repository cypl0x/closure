//! S3: Replica wire encoding. encode -> decode roundtrips exactly;
//! malformed buffers error without panic. Hermetic, no network.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_crdt::Replica;

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

const SRC: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n\
                   * Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";

#[test]
fn encode_decode_roundtrips_exactly() {
    let r = Replica::snapshot(&doc(SRC), 7, "r");
    let bytes = r.encode();
    let back = Replica::decode(&bytes).expect("decode");
    assert_eq!(r, back, "roundtrip preserves the replica exactly");
}

#[test]
fn empty_replica_roundtrips() {
    let r = Replica::default();
    let back = Replica::decode(&r.encode()).expect("decode empty");
    assert_eq!(r, back);
}

#[test]
fn truncated_buffer_errors_without_panic() {
    let r = Replica::snapshot(&doc(SRC), 1, "r");
    let mut bytes = r.encode();
    bytes.truncate(bytes.len() / 2);
    assert!(Replica::decode(&bytes).is_err(), "truncated -> error");
}

#[test]
fn garbage_buffer_errors_without_panic() {
    assert!(Replica::decode(&[0xff, 0xff, 0xff, 0xff, 0x01]).is_err());
}
