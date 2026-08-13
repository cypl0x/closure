//! Two peers find each other from a shared name, with no address typed.
//!
//! The wire protocol was always real — framing, sessions, merge, ed25519
//! signing, and `TcpSyncTransport` moving bytes over a real socket to
//! any `SocketAddr`. The gap was that somebody had to *know* that
//! address. On two laptops on one wifi, which is what P2P sync is for in
//! practice, there is nothing to type.
//!
//! What is tested here is the protocol: the line a peer broadcasts, and
//! what a listener does with one. The socket that carries it is not,
//! and that is stated rather than hidden — this machine's loopback
//! interface has no MULTICAST flag (`ip -o link` shows `lo` without it),
//! so nothing addressed to a group comes back and a test asserting
//! otherwise would only pass somewhere else. The format, the name
//! matching and the round-trip are where the decisions are.
//!
//! Deliberately LAN-only. Reaching a peer behind a different router
//! needs a relay or a STUN server — a service closure would have to run
//! — which is recorded in `docs/spec.md` rather than smuggled in behind
//! a discovery API.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_sync::{SyncTicket, announcement_line, parse_announcement};

fn ticket(port: u16, seed: u8) -> SyncTicket {
    SyncTicket {
        addr: format!("127.0.0.1:{port}").parse().unwrap(),
        pubkey: closure_sync::SigningKey::from_bytes(&[seed; 32]).verifying_key(),
    }
}

#[test]
fn an_announcement_round_trips() {
    let t = ticket(7431, 1);
    let heard = parse_announcement(&announcement_line("vault-alpha", &t), "vault-alpha")
        .expect("an announcement for this name");
    assert_eq!(heard.addr, t.addr);
    assert_eq!(heard.pubkey, t.pubkey);
}

#[test]
fn a_different_name_does_not_match() {
    // Two people on one office wifi must not pair by accident.
    let t = ticket(7432, 2);
    let line = announcement_line("vault-beta", &t);
    assert!(parse_announcement(&line, "vault-gamma").is_none());
}

#[test]
fn a_future_version_is_ignored_rather_than_misread() {
    // The version is first for this reason: a peer running a newer
    // closure should be invisible to an older one, not half-understood.
    let t = ticket(7433, 3);
    let line = announcement_line("vault-delta", &t).replace("closure-sync/1", "closure-sync/2");
    assert!(parse_announcement(&line, "vault-delta").is_none());
}

#[test]
fn noise_on_the_port_is_not_a_peer() {
    // A multicast group is shared. Anything that is not ours has to be
    // dropped without a panic (I5) rather than parsed hopefully.
    for line in [
        "",
        "hello",
        "closure-sync/1",
        "closure-sync/1 vault-alpha",
        "closure-sync/1 vault-alpha not-a-ticket",
        "closure-sync/1 vault-alpha closure-sync:notanaddr|zz",
    ] {
        assert!(
            parse_announcement(line, "vault-alpha").is_none(),
            "{line:?}"
        );
    }
}

#[test]
fn a_name_with_a_space_in_it_cannot_forge_another() {
    // The line is space-separated, so a vault called "a b" would
    // otherwise let its announcement read as a vault called "a". The
    // name is the whole authorisation here, so this is the one parsing
    // detail that matters for safety rather than tidiness.
    let t = ticket(7434, 4);
    let line = announcement_line("a b", &t);
    assert!(
        parse_announcement(&line, "a").is_none(),
        "a name with a space forged a shorter one: {line}"
    );
}

#[test]
fn what_comes_back_is_the_ticket_pairing_already_takes() {
    // The point of a `SyncTicket` rather than an address: discovery
    // fills in what somebody would otherwise paste, so the pairing path
    // is unchanged and there is no second way to pair.
    let t = ticket(7435, 5);
    let heard = parse_announcement(&announcement_line("vault-eps", &t), "vault-eps").unwrap();
    let decoded = SyncTicket::decode(&heard.encode()).expect("a valid ticket");
    assert_eq!(decoded.addr, t.addr);
    assert_eq!(decoded.pubkey, t.pubkey);
}
