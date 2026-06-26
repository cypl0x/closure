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
fn cid_is_blake3_256bit() {
    // D2: the Cid is a real cryptographic hash — BLAKE3, 256-bit, prefixed
    // `b3`. Pinned two ways: (1) against the lib's own digest so the
    // algorithm is exactly BLAKE3, and (2) against the published empty-input
    // test vector so it is the *canonical* BLAKE3, not a look-alike.
    let cid = Cid::of(b"closure");
    assert_eq!(
        cid.as_str(),
        format!("b3{}", blake3::hash(b"closure").to_hex()),
        "Cid must be `b3` + canonical BLAKE3 hex"
    );
    assert_eq!(cid.as_str().len(), 2 + 64, "b3 prefix + 256-bit (64 hex)");
    assert_eq!(
        Cid::of(b"").as_str(),
        "b3af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        "published BLAKE3 empty-input vector"
    );
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
