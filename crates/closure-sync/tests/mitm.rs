//! "`Noise_NN` is unauthenticated: an active MITM reads everything."
//!
//! NN agrees an ephemeral key with whoever answers. That is
//! confidentiality against a listener and nothing at all against
//! someone in the path: the attacker runs one handshake with each side
//! and holds two channels it can read and rewrite.
//!
//! The frames inside are signed, so it cannot forge a *replica* — but
//! it can read every one, drop the ones it dislikes, and replay old
//! ones. Signing the payload proves who wrote the bytes; it does not
//! prove who you are talking to.
//!
//! What proves that is binding the identity to the channel: each side
//! signs the Noise handshake hash and sends the signature *through*
//! the channel. An attacker in the middle has two channels with two
//! different hashes, and cannot produce the peer's signature over the
//! one you are on.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_sync::{NoiseChannel, SigningKey};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// The wire's chunk framing, hand-rolled — an attacker does not link
/// against your crate's private helpers, and neither does this.
fn put(stream: &mut std::net::TcpStream, frames: &[Vec<u8>]) {
    use std::io::Write as _;
    let count = u32::try_from(frames.len()).unwrap();
    stream.write_all(&count.to_le_bytes()).unwrap();
    for f in frames {
        let len = u32::try_from(f.len()).unwrap();
        stream.write_all(&len.to_le_bytes()).unwrap();
        stream.write_all(f).unwrap();
    }
}

fn get(stream: &mut std::net::TcpStream) -> Option<Vec<Vec<u8>>> {
    use std::io::Read as _;
    let mut n = [0u8; 4];
    stream.read_exact(&mut n).ok()?;
    let count = u32::from_le_bytes(n);
    let mut out = Vec::new();
    for _ in 0..count {
        let mut len = [0u8; 4];
        stream.read_exact(&mut len).ok()?;
        let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
        stream.read_exact(&mut buf).ok()?;
        out.push(buf);
    }
    Some(out)
}

#[test]
fn two_honest_peers_prove_who_they_are() {
    let alice = key(1);
    let bob = key(2);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let bob_key = bob.clone();
    let alice_pub = alice.verifying_key();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut chan = NoiseChannel::handshake_responder(&mut stream).unwrap();
        chan.authenticate(&mut stream, &bob_key, &alice_pub, false)
    });

    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let mut chan = NoiseChannel::handshake_initiator(&mut stream).unwrap();
    let client = chan.authenticate(&mut stream, &alice, &bob.verifying_key(), true);

    assert!(
        client.is_ok(),
        "the client refused an honest peer: {client:?}"
    );
    assert!(
        server.join().unwrap().is_ok(),
        "the server refused an honest peer"
    );
}

#[test]
fn somebody_in_the_middle_is_caught() {
    let alice = key(1);
    let bob = key(2);

    // Bob's real listener, and the attacker's, which Alice will reach.
    let bobs = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bobs_addr = bobs.local_addr().unwrap();
    let attacker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let attacker_addr = attacker.local_addr().unwrap();

    let bob_key = bob.clone();
    let alice_pub = alice.verifying_key();
    let honest_bob = std::thread::spawn(move || {
        let (mut stream, _) = bobs.accept().unwrap();
        let mut chan = NoiseChannel::handshake_responder(&mut stream).unwrap();
        let _ = chan.authenticate(&mut stream, &bob_key, &alice_pub, false);
    });

    // The attacker, doing the thing NN allows: one handshake with each
    // side, so it holds two channels and can read and rewrite
    // everything that crosses. A transparent byte relay is not this —
    // that is just a wire, and it should and does succeed.
    let middle = std::thread::spawn(move || {
        let (mut from_alice, _) = attacker.accept().unwrap();
        let mut to_bob = std::net::TcpStream::connect(bobs_addr).unwrap();
        let mut with_alice = NoiseChannel::handshake_responder(&mut from_alice).unwrap();
        let mut with_bob = NoiseChannel::handshake_initiator(&mut to_bob).unwrap();
        // Alice speaks first: her proof, in the clear to the attacker,
        // re-encrypted onwards to Bob.
        let Some(ct) = get(&mut from_alice) else {
            return Vec::new();
        };
        let Ok(seen) = with_alice.decrypt_chunks(&ct) else {
            return Vec::new();
        };
        put(&mut to_bob, &with_bob.encrypt_chunks(&seen).unwrap());
        // Bob's proof back the other way.
        if let Some(ct) = get(&mut to_bob)
            && let Ok(bobs) = with_bob.decrypt_chunks(&ct)
        {
            put(&mut from_alice, &with_alice.encrypt_chunks(&bobs).unwrap());
        }
        seen
    });

    let mut stream = std::net::TcpStream::connect(attacker_addr).unwrap();
    let mut chan = NoiseChannel::handshake_initiator(&mut stream).unwrap();
    // Alice believes she is talking to Bob, and checks it.
    let got = chan.authenticate(&mut stream, &alice, &bob.verifying_key(), true);

    let seen = middle.join().unwrap_or_default();
    let _ = honest_bob.join();
    assert!(
        got.is_err(),
        "an attacker holding both halves of the conversation was accepted as Bob"
    );
    // And this is why it matters: everything Alice sent, the attacker
    // read. What stops it is being caught before the vault goes over,
    // not the channel being private from it.
    assert!(
        !seen.is_empty(),
        "the attacker saw nothing, so this test is not testing the attack"
    );
}

#[test]
fn the_wrong_peer_is_caught_even_when_it_is_honest() {
    // Not an attack: the address in the ticket now answers with
    // somebody else's closure. Same refusal, and it has to be — a
    // ticket names one peer.
    let alice = key(1);
    let bob = key(2);
    let carol = key(3);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let alice_pub = alice.verifying_key();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut chan = NoiseChannel::handshake_responder(&mut stream).unwrap();
        let _ = chan.authenticate(&mut stream, &carol, &alice_pub, false);
    });

    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let mut chan = NoiseChannel::handshake_initiator(&mut stream).unwrap();
    let got = chan.authenticate(&mut stream, &alice, &bob.verifying_key(), true);
    assert!(got.is_err(), "Carol was accepted as Bob");
}

#[test]
fn a_signature_from_another_channel_does_not_transfer() {
    // The property underneath: the proof is over *this* handshake, so
    // one taken from another conversation is not a proof here.
    let (one, _) = NoiseChannel::pair().unwrap();
    let (two, _) = NoiseChannel::pair().unwrap();
    assert_ne!(
        one.handshake_hash(),
        two.handshake_hash(),
        "two handshakes produced the same hash"
    );
}
