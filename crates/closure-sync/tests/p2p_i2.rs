//! D3: two peers with *divergent* vaults converge over a real
//! `127.0.0.1` loopback socket using authenticated (ed25519-signed) AND
//! encrypted (Noise) frames — and the merge preserves every block id
//! verbatim (I2: no regeneration). Also proves authenticity is enforced on
//! the wire: an untrusted peer is rejected. Hermetic (loopback, no
//! external network, no heavy deps); iroh/QUIC is the external drop-in
//! behind the same `SyncMessage` protocol (`just sync-net`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::{SocketAddr, TcpListener};
use std::thread;

use closure_core::Document;
use closure_sync::{SigningKey, SyncSession, TcpSyncTransport, VerifyingKey};

const A_ID: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B_ID: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn session(name: &str, title: &str, id: &str) -> SyncSession {
    let src = format!("* {title}\n:PROPERTIES:\n:ID: {id}\n:END:\nbody\n");
    let mut s = SyncSession::new(name);
    s.record_local(&Document::load_str(&src).expect("doc"));
    s
}

#[test]
fn divergent_vaults_converge_over_secure_socket_preserving_ids() {
    let server_key = SigningKey::from_bytes(&[7; 32]);
    let client_key = SigningKey::from_bytes(&[8; 32]);
    let server_trusts = vec![client_key.verifying_key()];
    let client_trusts = vec![server_key.verifying_key()];

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");

    let server = thread::spawn(move || {
        let mut s = session("server", "Beta", B_ID);
        TcpSyncTransport::serve_once_secure(&listener, &mut s, &server_key, &server_trusts)
            .expect("serve");
        s
    });

    let mut client = session("client", "Alpha", A_ID);
    TcpSyncTransport::connect_and_sync_secure(addr, &mut client, &client_key, &client_trusts)
        .expect("connect");
    let server = server.join().expect("join");

    // Each end converged on both blocks...
    for (who, s) in [("client", &client), ("server", &server)] {
        let ids: Vec<String> = s.block_ids().map(|b| b.as_str().to_owned()).collect();
        assert_eq!(ids.len(), 2, "{who} converged on both blocks");
        // ...and every id survives the network merge *verbatim* (I2 — the
        // CRDT addresses by BlockId, so no merge ever regenerates one).
        assert!(
            ids.contains(&A_ID.to_owned()),
            "{who} kept A id (I2): {ids:?}"
        );
        assert!(
            ids.contains(&B_ID.to_owned()),
            "{who} kept B id (I2): {ids:?}"
        );
    }
    assert_eq!(client.title_of_str(B_ID), Some("Beta".to_owned()));
    assert_eq!(server.title_of_str(A_ID), Some("Alpha".to_owned()));
}

#[test]
fn untrusted_peer_is_rejected_over_the_socket() {
    let server_key = SigningKey::from_bytes(&[7; 32]);
    let client_key = SigningKey::from_bytes(&[8; 32]);
    let stranger_key = SigningKey::from_bytes(&[9; 32]);
    // The server trusts only `stranger`, NOT the connecting client — so the
    // client's valid-but-unrecognised signature must be refused, even
    // though the Noise channel itself establishes fine.
    let server_trusts: Vec<VerifyingKey> = vec![stranger_key.verifying_key()];
    let client_trusts = vec![server_key.verifying_key()];

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");

    let server = thread::spawn(move || {
        let mut s = session("server", "Beta", B_ID);
        let res =
            TcpSyncTransport::serve_once_secure(&listener, &mut s, &server_key, &server_trusts);
        (res.is_err(), s)
    });

    let mut client = session("client", "Alpha", A_ID);
    // The client may also error (the server closes the stream on reject).
    let _ =
        TcpSyncTransport::connect_and_sync_secure(addr, &mut client, &client_key, &client_trusts);
    let (server_rejected, server_session) = server.join().expect("join");

    assert!(
        server_rejected,
        "untrusted signature must be rejected on the wire"
    );
    // The server never applied the unverified frame.
    assert_eq!(
        server_session.block_ids().count(),
        1,
        "server kept only its own block; the spoofed frame was not merged"
    );
}
