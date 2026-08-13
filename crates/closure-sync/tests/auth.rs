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
fn an_empty_trust_set_trusts_nobody() {
    // Rewritten 2026-08-13. This asserted the opposite — that an empty
    // list accepts any valid signature — and called it "integrity-only
    // mode: no pinned peers, but the signature must still be
    // self-consistent".
    //
    // The intent is coherent and the default is wrong. A signature
    // proves the sender holds *some* key, and an attacker generates
    // their own, so integrity-only stops corruption in transit and
    // nothing else. More to the point it was not chosen: `trusted` is
    // the peer list, so a vault with nobody paired was in that mode
    // without anyone deciding to be — and that is exactly the vault
    // that has just been bound to a socket and not yet paired.
    //
    // Failing open on an empty allow-list is the classic shape. If a
    // real integrity-only mode is wanted it should be a named argument
    // somebody passes on purpose, not the emergent meaning of an empty
    // slice.
    let bob = key(2);
    let a = session_with("a", "Alpha");
    let bytes = SyncMessage::from_session(&a).to_signed_bytes(&bob);
    assert!(
        SyncMessage::from_signed_bytes(&bytes, &[]).is_err(),
        "a frame from an unpaired peer was accepted because no peers were paired"
    );
    // …and the same frame is still accepted once bob is paired, so this
    // is a membership check and not a refusal of everything.
    assert!(SyncMessage::from_signed_bytes(&bytes, &[bob.verifying_key()]).is_ok());
}
