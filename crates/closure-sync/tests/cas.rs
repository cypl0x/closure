//! V5a: content-addressed block store. Every blob is addressable by a
//! stable content id (CID); identical content dedups to one entry;
//! reads verify the stored bytes still hash to their key.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_sync::{BlockStore, Cid};

#[test]
fn same_content_yields_same_cid() {
    assert_eq!(Cid::of(b"hello"), Cid::of(b"hello"));
    assert_ne!(Cid::of(b"hello"), Cid::of(b"world"));
}

#[test]
fn cid_is_a_stable_string() {
    // Deterministic textual form (I6), usable as a key / on the wire.
    let cid = Cid::of(b"closure");
    assert_eq!(cid.as_str(), Cid::of(b"closure").as_str());
    assert!(!cid.as_str().is_empty());
}

#[test]
fn put_returns_cid_and_get_round_trips() {
    let mut store = BlockStore::new();
    let cid = store.put(b"some block bytes");
    assert_eq!(cid, Cid::of(b"some block bytes"));
    assert_eq!(store.get(&cid).unwrap(), b"some block bytes");
    assert!(store.has(&cid));
}

#[test]
fn identical_content_dedups() {
    let mut store = BlockStore::new();
    let a = store.put(b"dup");
    let b = store.put(b"dup");
    assert_eq!(a, b, "same content → same cid");
    assert_eq!(store.len(), 1, "stored once");
}

#[test]
fn get_missing_is_none() {
    let store = BlockStore::new();
    assert!(store.get(&Cid::of(b"absent")).is_none());
}

#[test]
fn verify_detects_a_tampered_blob() {
    let mut store = BlockStore::new();
    let cid = store.put(b"trusted");
    assert!(store.verify(&cid), "intact blob verifies");
    // Inject a blob under the wrong cid → verify must reject it.
    store.insert_raw(cid.clone(), b"tampered".to_vec());
    assert!(!store.verify(&cid), "hash mismatch detected on read");
}
