//! C3b: transport encryption. A Noise channel encrypts `SyncMessage`
//! bytes so the replica never crosses the wire in plaintext, while the
//! same framing decrypts + converges on the far side. Hermetic
//! (in-process handshake; no sockets).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;
use closure_sync::{NoiseChannel, SyncMessage, SyncSession};

fn session_with(name: &str, title: &str) -> SyncSession {
    let doc = Document::load_str(&format!("* {title}\n")).expect("doc");
    let mut s = SyncSession::new(name);
    s.record_local(&doc);
    s
}

#[test]
fn wire_bytes_are_not_the_plaintext_replica() {
    let (mut initiator, mut _responder) = NoiseChannel::pair().expect("handshake");
    let a = session_with("a", "TopSecret");
    let plaintext = SyncMessage::from_session(&a).to_bytes();

    let wire = initiator.encrypt(&plaintext).expect("encrypt");
    assert_ne!(wire, plaintext, "ciphertext must differ from plaintext");
    assert!(
        !wire.windows(4).any(|w| w == b"CLSY"),
        "the SyncMessage magic must not be visible on the wire"
    );
    // The headline title must not leak in the clear either.
    assert!(
        !wire.windows(9).any(|w| w == b"TopSecret"),
        "replica content must be encrypted"
    );
}

#[test]
fn encrypted_round_trip_decrypts_and_converges() {
    let (mut initiator, mut responder) = NoiseChannel::pair().expect("handshake");
    let a = session_with("a", "Alpha");
    let plaintext = SyncMessage::from_session(&a).to_bytes();

    let wire = initiator.encrypt(&plaintext).expect("encrypt");
    let recovered = responder.decrypt(&wire).expect("decrypt");
    assert_eq!(recovered, plaintext, "decrypt restores the exact frame");

    let msg = SyncMessage::from_bytes(&recovered).expect("parse");
    let mut b = SyncSession::new("b");
    b.apply_message(&msg);
    assert!(b.block_ids().next().is_some(), "decrypted frame converged");
}

#[test]
fn secure_tcp_round_trip_converges_over_127_0_0_1() {
    use closure_sync::{SigningKey, TcpSyncTransport, VerifyingKey};
    use std::net::{TcpListener, SocketAddr};

    let server_key = SigningKey::from_bytes(&[7; 32]);
    let client_key = SigningKey::from_bytes(&[8; 32]);
    let server_trusts: Vec<VerifyingKey> = vec![client_key.verifying_key()];
    let client_trusts: Vec<VerifyingKey> = vec![server_key.verifying_key()];

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");

    let server = std::thread::spawn(move || {
        let mut s = session_with("server", "ServerNote");
        TcpSyncTransport::serve_once_secure(&listener, &mut s, &server_key, &server_trusts)
            .expect("serve");
        s
    });

    let mut client = session_with("client", "ClientNote");
    TcpSyncTransport::connect_and_sync_secure(addr, &mut client, &client_key, &client_trusts)
        .expect("connect");
    let server = server.join().expect("join");

    // Both ends now hold both blocks (converged over the encrypted link).
    assert_eq!(client.block_ids().count(), 2);
    assert_eq!(server.block_ids().count(), 2);
}

#[test]
fn tampered_ciphertext_fails_to_decrypt() {
    let (mut initiator, mut responder) = NoiseChannel::pair().expect("handshake");
    let mut wire = initiator.encrypt(b"hello").expect("encrypt");
    let last = wire.len() - 1;
    wire[last] ^= 0xFF;
    assert!(
        responder.decrypt(&wire).is_err(),
        "AEAD tag must reject a tampered ciphertext"
    );
}
