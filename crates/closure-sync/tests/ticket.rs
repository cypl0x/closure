//! Q10: sync tickets — the plain-text pairing artifact. A ticket
//! names where to connect and WHO must sign (addr + ed25519 verifying
//! key), so `join` gets authenticity for free from the C3a trusted
//! set. Loopback-hermetic.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::Document;
use closure_sync::{SyncSession, SyncTicket, TcpSyncTransport};

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

#[test]
fn ticket_round_trips_as_plain_text() {
    let key = closure_sync::SigningKey::from_bytes(&[7u8; 32]);
    let t = SyncTicket {
        addr: "127.0.0.1:4711".parse().expect("addr"),
        pubkey: key.verifying_key(),
    };
    let s = t.encode();
    assert!(s.starts_with("closure-sync:"), "greppable prefix: {s}");
    assert!(!s.contains('\n'), "single line, storable in a vault file");
    let back = SyncTicket::decode(&s).expect("decode");
    assert_eq!(back.addr, t.addr);
    assert_eq!(back.pubkey, t.pubkey);
}

#[test]
fn ticket_decode_rejects_garbage() {
    assert!(SyncTicket::decode("nonsense").is_err());
    assert!(SyncTicket::decode("closure-sync:not@addr#key").is_err());
    assert!(SyncTicket::decode("closure-sync:127.0.0.1:1|zzzz").is_err());
}

#[test]
fn pairing_via_ticket_syncs_and_pins_the_signer() {
    let a_key = closure_sync::SigningKey::from_bytes(&[1u8; 32]);
    let b_key = closure_sync::SigningKey::from_bytes(&[2u8; 32]);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    // The server hands out its ticket (addr + ITS verifying key).
    let ticket = SyncTicket {
        addr,
        pubkey: a_key.verifying_key(),
    }
    .encode();

    let b_pub = b_key.verifying_key();
    let server = std::thread::spawn(move || {
        let mut sa = SyncSession::new("a");
        sa.record_local(&doc(
            "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
        ));
        TcpSyncTransport::serve_once_secure(&listener, &mut sa, &a_key, &[b_pub])
            .expect("serve");
        sa
    });

    let mut sb = SyncSession::new("b");
    sb.record_local(&doc(
        "* Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\n",
    ));
    let t = SyncTicket::decode(&ticket).expect("decode");
    // join = connect to the ticket's addr trusting exactly its key.
    TcpSyncTransport::connect_and_sync_secure(t.addr, &mut sb, &b_key, &[t.pubkey])
        .expect("join");
    let sa = server.join().expect("thread");
    let ids_b: Vec<String> = sb.block_ids().map(ToString::to_string).collect();
    assert_eq!(ids_b.len(), 2, "b holds both blocks: {ids_b:?}");
    assert_eq!(
        sa.block_ids().count(),
        2,
        "a converged too (I2 ids preserved)"
    );
}
