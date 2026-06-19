//! C3a: authenticated sync frames. A `SyncMessage` is signed by the
//! sending peer's ed25519 key; a tampered or forged frame is rejected
//! before it can be merged into a session. Hermetic.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_sync::{SigningKey, SyncMessage, SyncSession, VerifyingKey};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn session_with(name: &str, title: &str) -> SyncSession {
    let doc = Document::load_str(&format!("* {title}\n")).expect("doc");
    let mut s = SyncSession::new(name);
    s.record_local(&doc);
    s
}

#[test]
fn signed_frame_verifies_and_converges() {
    let alice = key(1);
    let trusted: Vec<VerifyingKey> = vec![alice.verifying_key()];
    let a = session_with("a", "Alpha");
    let bytes = SyncMessage::from_session(&a).to_signed_bytes(&alice);

    let msg = SyncMessage::from_signed_bytes(&bytes, &trusted).expect("trusted + intact");
    let mut b = SyncSession::new("b");
    b.apply_message(&msg);
    assert!(
        b.block_ids().next().is_some(),
        "verified frame merged into peer"
    );
}

#[test]
fn tampered_frame_is_rejected() {
    let alice = key(1);
    let trusted = vec![alice.verifying_key()];
    let a = session_with("a", "Alpha");
    let mut bytes = SyncMessage::from_session(&a).to_signed_bytes(&alice);

    // Flip a byte in the signed payload (past magic+version+pubkey+sig).
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    assert!(
        SyncMessage::from_signed_bytes(&bytes, &trusted).is_err(),
        "tampered payload must fail signature verification"
    );
}

#[test]
fn forged_frame_from_untrusted_key_is_rejected() {
    let mallory = key(9);
    let alice_trusted = vec![key(1).verifying_key()];
    let a = session_with("a", "Alpha");
    // Mallory signs a perfectly valid frame — but is not trusted.
    let bytes = SyncMessage::from_session(&a).to_signed_bytes(&mallory);
    assert!(
        SyncMessage::from_signed_bytes(&bytes, &alice_trusted).is_err(),
        "untrusted signer rejected even with a valid signature"
    );
}

#[test]
fn empty_trust_set_accepts_any_valid_signature() {
    // Integrity-only mode: no pinned peers, but the signature must still
    // be self-consistent (tamper still caught by the previous test).
    let bob = key(2);
    let a = session_with("a", "Alpha");
    let bytes = SyncMessage::from_session(&a).to_signed_bytes(&bob);
    assert!(SyncMessage::from_signed_bytes(&bytes, &[]).is_ok());
}
