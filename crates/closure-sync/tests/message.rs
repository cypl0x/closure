//! S4a: the framed sync message a transport ships — a versioned header
//! wrapping an encoded Replica. Fully hermetic (no network/feature).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_sync::{SyncMessage, SyncSession};

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

const A: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n";
const B: &str = "* Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";

#[test]
fn message_round_trip_drives_convergence_over_bytes() {
    let mut a = SyncSession::new("a");
    let mut b = SyncSession::new("b");
    a.record_local(&doc(A));
    b.record_local(&doc(B));
    // A frames its state to bytes; B parses + merges (the full wire path).
    let wire: Vec<u8> = SyncMessage::from_session(&a).to_bytes();
    let msg = SyncMessage::from_bytes(&wire).expect("parse");
    b.apply_message(&msg);
    // And back the other way.
    let wire_b = SyncMessage::from_session(&b).to_bytes();
    a.apply_message(&SyncMessage::from_bytes(&wire_b).expect("parse"));
    assert_eq!(a.block_ids().count(), 2);
    assert_eq!(b.block_ids().count(), 2);
    assert_eq!(b.title_of_str("01AAAAAAAAAAAAAAAAAAAAAAAA"), Some("Alpha".to_owned()));
}

#[test]
fn bad_magic_errors() {
    assert!(SyncMessage::from_bytes(b"XXXX\x01").is_err());
}

#[test]
fn unsupported_version_errors() {
    // Right magic, absurd version byte.
    let mut bad = b"CLSY".to_vec();
    bad.push(0xff);
    assert!(SyncMessage::from_bytes(&bad).is_err());
}

#[test]
fn empty_buffer_errors_without_panic() {
    assert!(SyncMessage::from_bytes(&[]).is_err());
}
