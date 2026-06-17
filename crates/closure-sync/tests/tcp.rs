//! S4b: a real network sync transport over std TCP. Tested hermetically
//! over `127.0.0.1` loopback (no external network, no heavy deps) — two
//! peers exchange `SyncMessage` frames and converge. iroh/QUIC is a
//! future drop-in behind the same `SyncMessage` protocol.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpListener;
use std::thread;

use closure_core::Document;
use closure_sync::{SyncSession, TcpSyncTransport};

fn doc(src: &str) -> Document {
    Document::load_str(src).expect("parse")
}

const A: &str = "* Alpha\n:PROPERTIES:\n:ID: 01AAAAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody a\n";
const B: &str = "* Beta\n:PROPERTIES:\n:ID: 01BBBBBBBBBBBBBBBBBBBBBBBB\n:END:\nbody b\n";

#[test]
fn two_peers_converge_over_tcp_loopback() {
    let mut a = SyncSession::new("a");
    let mut b = SyncSession::new("b");
    a.record_local(&doc(A));
    b.record_local(&doc(B));

    // Peer B serves on an ephemeral loopback port.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = thread::spawn(move || {
        TcpSyncTransport::serve_once(&listener, &mut b).expect("serve");
        b // hand the merged session back
    });

    // Peer A connects and syncs.
    TcpSyncTransport::connect_and_sync(addr, &mut a).expect("connect");
    let b = server.join().expect("join");

    // Both learned both blocks.
    assert_eq!(a.block_ids().count(), 2, "A converged");
    assert_eq!(b.block_ids().count(), 2, "B converged");
    assert_eq!(a.title_of_str("01BBBBBBBBBBBBBBBBBBBBBBBB"), Some("Beta".to_owned()));
    assert_eq!(b.title_of_str("01AAAAAAAAAAAAAAAAAAAAAAAA"), Some("Alpha".to_owned()));
}
