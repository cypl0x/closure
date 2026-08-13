//! A sync frame comes from another machine.
//!
//! Which makes this crate's decoders the one place in closure where the
//! input is not a file the user wrote — it is bytes off a socket, from a
//! peer that may be running a different version, a corrupted link, or
//! somebody having a go. 93 of its 760 lines were unexecuted and they
//! are the arms that handle exactly that.
//!
//! The claim is that no sequence of bytes panics a decoder and that an
//! unsigned or wrongly-signed frame is refused. A peer must not be able
//! to crash the notebook by sending it something, and must not be able
//! to have its edits applied without the key.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_sync::{SigningKey, SyncMessage, SyncSession, SyncTicket};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

#[test]
fn no_byte_sequence_panics_the_decoder() {
    let hostile: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0xff; 4],
        vec![0xff; 1024],
        b"hello".to_vec(),
        b"{\"jsonrpc\":\"2.0\"}".to_vec(),
        // A length prefix claiming far more than follows: the classic
        // way to make a reader allocate or wait forever.
        vec![0xff, 0xff, 0xff, 0xff, 1, 2, 3],
        vec![0, 0, 0, 0],
        // Valid UTF-8 that is not a frame.
        "🎉 not a frame".as_bytes().to_vec(),
        // Invalid UTF-8.
        vec![0xf0, 0x28, 0x8c, 0x28],
    ];
    for bytes in &hostile {
        let _ = SyncMessage::from_bytes(bytes);
        let _ = SyncMessage::from_signed_bytes(bytes, &[]);
    }
}

#[test]
fn a_frame_signed_by_nobody_we_trust_is_refused() {
    // The whole point of signing. A peer we have not paired with must
    // not get its edits applied.
    let mine = key(1);
    let theirs = key(2);
    let session = SyncSession::new("a");
    let signed = SyncMessage::from_session(&session).to_signed_bytes(&mine);
    // Signed by mine, offered to a peer who trusts only theirs.
    assert!(
        SyncMessage::from_signed_bytes(&signed, &[theirs.verifying_key()]).is_err(),
        "a frame signed by an unpaired key was accepted"
    );
    // …and the same frame verifies for a peer who does trust it, or the
    // test above would pass on a decoder that refuses everything.
    assert!(
        SyncMessage::from_signed_bytes(&signed, &[mine.verifying_key()]).is_ok(),
        "a frame signed by a trusted key was refused"
    );
}

#[test]
fn an_empty_trust_list_trusts_nobody() {
    // Failing open here would mean a fresh vault accepts edits from
    // anything that can reach the socket.
    let session = SyncSession::new("a");
    let signed = SyncMessage::from_session(&session).to_signed_bytes(&key(1));
    assert!(
        SyncMessage::from_signed_bytes(&signed, &[]).is_err(),
        "a frame verified against an empty trust list"
    );
}

#[test]
fn a_ticket_that_is_not_one_is_refused() {
    for bad in [
        "",
        "hello",
        "closure-sync:",
        "closure-sync:notanaddr|deadbeef",
        "closure-sync:127.0.0.1:7000",
        "closure-sync:127.0.0.1:7000|",
        "closure-sync:127.0.0.1:7000|zz",
        // Right shape, hex of the wrong length — a key that is 63 or 65
        // characters is a typo, and taking it would pair with nothing.
        "closure-sync:127.0.0.1:7000|00112233",
        "wrong-scheme:127.0.0.1:7000|00",
    ] {
        assert!(
            SyncTicket::decode(bad).is_err(),
            "`{bad}` was accepted as a ticket"
        );
    }
}

#[test]
fn a_real_ticket_still_decodes() {
    // The control: a decoder that refused everything would satisfy the
    // test above.
    let t = SyncTicket {
        addr: "127.0.0.1:7000".parse().unwrap(),
        pubkey: key(3).verifying_key(),
    };
    let back = SyncTicket::decode(&t.encode()).expect("a valid ticket");
    assert_eq!(back.addr, t.addr);
    assert_eq!(back.pubkey, t.pubkey);
}

#[test]
fn a_ticket_round_trips_through_its_own_text() {
    // It is pasted between machines by hand, so the text is the
    // interface and has to survive being one.
    for port in [1_u16, 7000, 65535] {
        let t = SyncTicket {
            addr: format!("127.0.0.1:{port}").parse().unwrap(),
            pubkey: key(4).verifying_key(),
        };
        let text = t.encode();
        assert!(!text.contains(' '), "a ticket with a space in it: {text}");
        assert_eq!(SyncTicket::decode(&text).expect("decodes").addr, t.addr);
    }
}
