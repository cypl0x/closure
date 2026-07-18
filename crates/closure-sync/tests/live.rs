//! Q11: live collaboration — a persistent session loop exchanging ops
//! per edit over ONE connection (C1), plus ephemeral presence frames
//! that never touch document state (C2). Loopback-hermetic.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::Document;
use closure_sync::{Presence, SyncSession, TcpSyncTransport};

fn doc(title: &str, id: &str) -> Document {
    Document::load_str(&format!("* {title}\n:PROPERTIES:\n:ID: {id}\n:END:\n")).expect("parse")
}

const A1: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B1: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const C1: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";

#[test]
fn stream_rounds_converge_interleaved_edits_live() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = std::thread::spawn(move || {
        let mut sa = SyncSession::new("a");
        let (mut stream, _) = listener.accept().expect("accept");
        // Round 1: a knows A1. Round 2: a has ALSO edited C1 meanwhile.
        sa.record_local(&doc("Alpha", A1));
        TcpSyncTransport::stream_round_server(&mut stream, &mut sa).expect("round 1");
        sa.record_local(&doc("Gamma", C1));
        TcpSyncTransport::stream_round_server(&mut stream, &mut sa).expect("round 2");
        sa
    });
    let mut sb = SyncSession::new("b");
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    sb.record_local(&doc("Beta", B1));
    TcpSyncTransport::stream_round_client(&mut stream, &mut sb).expect("round 1");
    assert_eq!(sb.block_ids().count(), 2, "after round 1: A1+B1");
    TcpSyncTransport::stream_round_client(&mut stream, &mut sb).expect("round 2");
    assert_eq!(sb.block_ids().count(), 3, "round 2 carried the live edit");
    let sa = server.join().expect("join");
    assert_eq!(sa.block_ids().count(), 3, "server converged too");
}

#[test]
fn presence_frames_are_ephemeral_and_typed() {
    let p = Presence {
        peer: "wap".into(),
        block: B1.into(),
        line: 4,
    };
    let bytes = p.encode();
    let back = Presence::decode(&bytes).expect("decode");
    assert_eq!(back, p);
    // A presence frame is NOT a sync message: the replica decoder
    // rejects it instead of merging cursor chatter into state.
    assert!(closure_sync::SyncMessage::from_bytes(&bytes).is_err());
}

#[test]
fn presence_decode_rejects_garbage() {
    assert!(Presence::decode(b"junk").is_err());
    assert!(Presence::decode(&[]).is_err());
}
