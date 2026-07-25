//! Pairing two peers from the GUI.
//!
//! closure-sync has had the whole stack for a while — an ed25519
//! identity, signed frames, a Noise-encrypted TCP transport, CRDT
//! merge with conflict detection, presence. All of it was reachable
//! only from tests and `cargo`. There was no way, from any shell, to
//! actually connect to another person: `closure sync` is git push/pull
//! and nothing more.
//!
//! This is the state behind that: an identity, a ticket to hand over,
//! a peer list, and the round that merges. The socket work belongs to
//! the shell (it blocks); everything that decides *what* happens is
//! here, and testable without a network.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, PeerState, Shell, SyncApp};
use closure_store::Vault;

fn vault_with(name: &str, title: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* {title}\n:PROPERTIES:\n:ID: 01HQSYNC00000000000000{name}\n:END:\n"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

// === identity and tickets ===

#[test]
fn a_fresh_app_has_an_identity_and_a_ticket() {
    let sync = SyncApp::new("laptop", "127.0.0.1:7777".parse().expect("addr"));
    let ticket = sync.ticket();
    assert!(
        ticket.starts_with("closure-sync:"),
        "a ticket is one pasteable line: {ticket}"
    );
    assert!(ticket.contains("127.0.0.1:7777"), "{ticket}");
    assert_eq!(sync.name(), "laptop");
}

#[test]
fn the_ticket_round_trips_through_the_wire_format() {
    // What one peer prints, the other must be able to paste.
    let sync = SyncApp::new("a", "192.168.1.5:9000".parse().expect("addr"));
    let decoded = closure_sync::SyncTicket::decode(&sync.ticket()).expect("decodes");
    assert_eq!(decoded.addr.to_string(), "192.168.1.5:9000");
    assert_eq!(decoded.pubkey, sync.public_key());
}

#[test]
fn two_apps_get_different_identities() {
    let a = SyncApp::new("a", "127.0.0.1:1".parse().expect("addr"));
    let b = SyncApp::new("b", "127.0.0.1:2".parse().expect("addr"));
    assert_ne!(
        a.public_key(),
        b.public_key(),
        "each peer signs with its own key"
    );
}

// === adding peers ===

#[test]
fn pasting_a_ticket_adds_a_peer() {
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    assert!(a.peers().is_empty());
    a.add_peer(&b.ticket()).expect("valid ticket");
    let peers = a.peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].addr.to_string(), "127.0.0.1:7002");
    assert_eq!(
        peers[0].state,
        PeerState::Known,
        "known, not connected — nothing has been sent yet"
    );
}

#[test]
fn a_malformed_ticket_is_refused_with_a_reason() {
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let err = a.add_peer("not a ticket").expect_err("refused");
    assert!(!err.is_empty(), "the error must say something");
    assert!(a.peers().is_empty(), "and nothing was added");
}

#[test]
fn the_same_peer_is_not_added_twice() {
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.add_peer(&b.ticket()).expect("first");
    a.add_peer(&b.ticket()).expect("second");
    assert_eq!(a.peers().len(), 1, "pasting twice is not two peers");
}

#[test]
fn our_own_ticket_is_refused() {
    // Pointing a vault at itself would merge it with itself forever.
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let own = a.ticket();
    let err = a.add_peer(&own).expect_err("refused");
    assert!(err.contains("own"), "say why: {err}");
    assert!(a.peers().is_empty());
}

#[test]
fn a_peers_outcome_is_recorded_against_it() {
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.add_peer(&b.ticket()).expect("added");
    let addr = a.peers()[0].addr;

    a.record_outcome(addr, Ok(3));
    assert_eq!(a.peers()[0].state, PeerState::Synced { blocks: 3 });

    a.record_outcome(addr, Err("connection refused".to_owned()));
    assert_eq!(
        a.peers()[0].state,
        PeerState::Failed("connection refused".to_owned()),
        "a failure is shown, not swallowed"
    );
}

// === merging ===

#[test]
fn a_round_merges_a_peers_replica_into_ours() {
    let (_da, mut shell_a) = vault_with("1", "Mine");
    let (_db, shell_b) = vault_with("2", "Theirs");
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.snapshot(&shell_a);
    b.snapshot(&shell_b);

    let conflicts = a.merge_session(b.session());
    assert!(conflicts.is_empty(), "different blocks do not conflict");
    // Our replica now knows about both blocks.
    let ids: Vec<String> = a.session().block_ids().map(ToString::to_string).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    let _ = &mut shell_a;
}

#[test]
fn a_divergent_title_comes_back_as_a_conflict() {
    // The whole reason the Conflicts surface exists: LWW would pick a
    // side silently, so both are surfaced instead.
    let (_da, shell_a) = vault_with("1", "Mine");
    let (_db, shell_b) = vault_with("1", "Theirs"); // same id, different title
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.snapshot(&shell_a);
    b.snapshot(&shell_b);
    let conflicts = a.merge_session(b.session());
    assert_eq!(conflicts.len(), 1, "one divergent title: {conflicts:?}");
    assert_eq!(conflicts[0].ours, "Mine");
    assert_eq!(conflicts[0].theirs, "Theirs");
}

// === the surface ===

fn app_fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let (dir, shell) = vault_with("1", "Note");
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn the_sync_command_opens_the_surface() {
    let (_d, mut shell, mut app) = app_fixture();
    app.run(&mut shell, "sync");
    assert_eq!(app.surface(), ModalSurface::Sync);
    assert!(
        app.sync()
            .expect("sync open")
            .ticket()
            .starts_with("closure-sync:"),
        "and it has a ticket to show"
    );
}

#[test]
fn typing_a_ticket_into_the_surface_adds_the_peer() {
    let (_d, mut shell, mut app) = app_fixture();
    let peer = SyncApp::new("them", "127.0.0.1:7099".parse().expect("addr"));
    app.run(&mut shell, "sync");
    for c in peer.ticket().chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.sync().expect("sync open").peers().len(),
        1,
        "{}",
        app.status()
    );
    assert_eq!(app.sync_buffer(), "", "the field clears after adding");
}

#[test]
fn a_bad_ticket_in_the_surface_reports_and_keeps_the_text() {
    let (_d, mut shell, mut app) = app_fixture();
    app.run(&mut shell, "sync");
    for c in "rubbish".chars() {
        app.on_key(&mut shell, "x", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.sync().expect("sync open").peers().is_empty());
    assert_eq!(app.sync_buffer(), "rubbish", "so it can be corrected");
    assert!(!app.status().is_empty());
}

#[test]
fn escape_leaves_the_sync_surface() {
    let (_d, mut shell, mut app) = app_fixture();
    app.run(&mut shell, "sync");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}
