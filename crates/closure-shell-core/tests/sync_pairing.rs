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
fn a_headline_made_any_of_the_three_ways_crosses_to_a_peer() {
    // "How do I create a new headline/sibling/subtree that is still
    // compatible with the P2P sync?" — the answer has to be "any of
    // them", so this is all three against a real replica. What makes
    // one syncable is its `:ID:`; a headline whose id lives only in
    // memory is a different block on every read, and no peer can agree
    // with us about it.
    let (_da, mut shell_a) = vault_with("1", "Mine");
    let mut a = SyncApp::new("a", "127.0.0.1:7003".parse().expect("addr"));
    let root = closure_core::BlockId::from_existing("01HQSYNC000000000000001");
    let root = shell_a
        .vault
        .find_by_title("Mine")
        .map_or(root, |(h, _)| h.id().clone());
    shell_a
        .vault
        .capture_under(&root, "", "Captured")
        .expect("capture");
    shell_a
        .vault
        .add_sibling(&root, "Sibling")
        .expect("sibling");
    shell_a
        .vault
        .set_body_with_children(&root, "", "* Typed\n")
        .expect("typed");
    a.snapshot(&shell_a);

    let ours: Vec<String> = a.session().block_ids().map(ToString::to_string).collect();
    assert_eq!(
        ours.len(),
        4,
        "all four blocks are in the replica: {ours:?}"
    );

    // …and the peer that merges it sees the same four ids, which is
    // the only thing "compatible with the sync" can mean.
    let (_db, shell_b) = vault_with("2", "Theirs");
    let mut b = SyncApp::new("b", "127.0.0.1:7004".parse().expect("addr"));
    b.snapshot(&shell_b);
    assert!(b.merge_session(a.session()).is_empty(), "no conflicts");
    let theirs: Vec<String> = b.session().block_ids().map(ToString::to_string).collect();
    for id in &ours {
        assert!(theirs.contains(id), "{id} did not cross: {theirs:?}");
    }
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

// === writing a merge back to the files ===
//
// The replica converging is only half of a sync. Until the merged
// state reaches the org files, nothing the user can see has changed —
// and the vault, not the replica, is the thing that gets committed to
// git and opened in Emacs.

#[test]
fn applying_a_merge_writes_the_new_title_to_disk() {
    let (dir_a, mut shell_a) = vault_with("1", "Mine");
    let (_db, shell_b) = vault_with("1", "Theirs");
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.snapshot(&shell_a);
    b.snapshot(&shell_b);
    // B's edit is newer, so the merge resolves to its title.
    let _ = a.merge_session(b.session());

    let applied = a.apply_to_vault(&mut shell_a);
    assert!(applied > 0, "something was written");
    let src = fs::read_to_string(dir_a.path().join("notes.org")).expect("read");
    assert!(
        src.contains(
            a.session()
                .title_of_str("01HQSYNC000000000000001")
                .unwrap_or_default()
                .as_str()
        ),
        "the converged title reached the file: {src}"
    );
    // …and the file is still org (I1).
    let doc = closure_core::Document::load_str(&src).expect("parses");
    assert_eq!(doc.source(), src, "byte-exact");
}

#[test]
fn applying_a_merge_that_changed_nothing_writes_nothing() {
    // Re-running a round must be a no-op, not a rewrite of every file
    // — a vault whose mtimes churn on every sync is a vault git cannot
    // tell you anything useful about.
    let (_da, mut shell_a) = vault_with("1", "Same");
    let (_db, shell_b) = vault_with("2", "Other");
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.snapshot(&shell_a);
    b.snapshot(&shell_b);
    let _ = a.merge_session(b.session());
    let _ = a.apply_to_vault(&mut shell_a);
    let revision = shell_a.vault.revision();
    let again = a.apply_to_vault(&mut shell_a);
    assert_eq!(again, 0, "nothing left to apply");
    assert_eq!(
        shell_a.vault.revision(),
        revision,
        "and the vault was not touched"
    );
}

#[test]
fn a_block_the_peer_has_and_we_do_not_arrives_in_the_capture_file() {
    // This asserted the opposite until 2026-08-03, on the reasoning
    // that "creating a headline out of a replica entry would need a
    // file to put it in and a place in the tree; guessing either is
    // worse than reporting it."
    //
    // The user filed the consequence as a defect: "`apply_sync_to_vault`
    // skips ids `find_by_id` misses, so a peer's new headline never
    // arrives." They are right — convergence of *known* blocks only is
    // not a sync anybody would describe as working, and the failure was
    // silent while the status line reported the edits it *had* applied.
    //
    // The old objection is answered rather than ignored: the file to
    // put it in is the capture file, which is already where a thought
    // with no home goes. That is an existing rule of the vault, not a
    // guess invented for this.
    let (_da, mut shell_a) = vault_with("1", "Mine");
    let (_db, shell_b) = vault_with("2", "Theirs");
    let mut a = SyncApp::new("a", "127.0.0.1:7001".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7002".parse().expect("addr"));
    a.snapshot(&shell_a);
    b.snapshot(&shell_b);
    let _ = a.merge_session(b.session());
    let _ = a.apply_to_vault(&mut shell_a);
    assert!(
        shell_a
            .vault
            .find_by_id(&closure_core::BlockId::from_existing(
                "01HQSYNC000000000000002"
            ))
            .is_some(),
        "their block converged in the replica and never reached the files"
    );
}

// === accepting a connection ===

#[test]
fn a_listener_can_be_bound_and_reports_its_address() {
    // Pairing needs both directions: handing over a ticket is useless
    // if nothing is listening at the address it names.
    let mut app = SyncApp::new("a", "127.0.0.1:0".parse().expect("addr"));
    let addr = app.listen().expect("bound");
    assert_ne!(addr.port(), 0, "the OS assigned a real port");
    assert_eq!(
        app.ticket_addr(),
        addr,
        "and the ticket now names where we actually listen"
    );
    assert!(app.ticket().contains(&addr.to_string()), "{}", app.ticket());
}

#[test]
fn listening_twice_keeps_the_first_socket() {
    let mut app = SyncApp::new("a", "127.0.0.1:0".parse().expect("addr"));
    let first = app.listen().expect("bound");
    let second = app.listen().expect("still bound");
    assert_eq!(first, second, "one listener, one address");
}

// === reaching another machine ===
//
// Everything above pairs two replicas that can already see each other.
// Nothing could: the socket bound `127.0.0.1:7420` and the ticket
// named it, so a ticket carried to a second machine told that machine
// to dial *itself*. Which socket to open and which address to hand out
// are two different questions, and the second one has no single right
// answer — a host on Tailscale and on a LAN has two reachable
// addresses and only the operator knows which one the peer is on.

#[test]
fn a_wide_bind_never_advertises_the_unspecified_address() {
    // `0.0.0.0` means "every interface" to bind(2) and nothing at all
    // to connect(2). A ticket naming it is worse than no ticket: it
    // fails at the peer, after the paste, with a confusing error.
    let mut app = SyncApp::with_bind("a", "0.0.0.0:0".parse().expect("addr"), None);
    let bound = app.listen().expect("bound");
    assert!(bound.ip().is_unspecified(), "we did bind every interface");
    let advertised = app.ticket_addr();
    assert!(
        !advertised.ip().is_unspecified(),
        "but the ticket names something dialable, got {advertised}"
    );
    assert_eq!(
        advertised.port(),
        bound.port(),
        "on the port we actually bound"
    );
    assert!(!app.ticket().contains("0.0.0.0"), "{}", app.ticket());
}

#[test]
fn an_explicit_advertise_address_wins_over_detection() {
    // The Tailscale case: bind everything, hand out the one address
    // the peer can route to.
    let ip: std::net::IpAddr = "100.101.102.103".parse().expect("ip");
    let mut app = SyncApp::with_bind("a", "0.0.0.0:0".parse().expect("addr"), Some(ip));
    let bound = app.listen().expect("bound");
    assert_eq!(
        app.ticket_addr(),
        std::net::SocketAddr::new(ip, bound.port())
    );
    assert!(app.ticket().contains("100.101.102.103"), "{}", app.ticket());
}

#[test]
fn a_loopback_bind_still_advertises_loopback() {
    // Two shells on one machine is a real configuration — and the one
    // every test above uses. Detection must not "improve" it into a
    // LAN address the local peer would then dial the long way round.
    let mut app = SyncApp::with_bind("a", "127.0.0.1:0".parse().expect("addr"), None);
    let bound = app.listen().expect("bound");
    assert_eq!(
        app.ticket_addr(),
        bound,
        "what we bound is what we hand out"
    );
}

#[test]
fn the_bind_address_is_readable_before_anything_is_bound() {
    // The pairing surface shows where it will listen; it must not have
    // to open a socket to find out.
    let bind: std::net::SocketAddr = "0.0.0.0:7420".parse().expect("addr");
    let app = SyncApp::with_bind("a", bind, None);
    assert_eq!(app.bind_addr(), bind);
}

#[test]
fn a_public_listener_refuses_inbound_until_a_peer_is_trusted() {
    // An empty trusted set is integrity-only mode in the transport:
    // any self-consistent signature is accepted, which on a loopback
    // socket is nobody and on `0.0.0.0` is anyone on the network. The
    // widened bind must not quietly widen who may write to the vault.
    let mut app = SyncApp::with_bind("a", "0.0.0.0:0".parse().expect("addr"), None);
    let refusal = app.inbound_ready().expect_err("refused");
    assert!(
        refusal.contains("ticket"),
        "and says what to do about it: {refusal}"
    );

    let peer = SyncApp::new("b", "10.0.0.9:7420".parse().expect("addr"));
    app.add_peer(&peer.ticket()).expect("pasted");
    app.inbound_ready()
        .expect("a trusted peer is what the listener was waiting for");
}

#[test]
fn a_loopback_listener_accepts_without_a_pasted_ticket() {
    // Nobody but this machine can reach it, and refusing here would
    // break the two-windows-on-one-box flow that already works.
    let app = SyncApp::with_bind("a", "127.0.0.1:0".parse().expect("addr"), None);
    app.inbound_ready().expect("loopback needs no ceremony");
}

#[test]
fn the_default_pairing_socket_is_reachable_from_the_network() {
    // The old default (`127.0.0.1:7420`) made every ticket a
    // machine-local one. A shell that has not been told otherwise now
    // listens where a peer can reach it.
    let (_dir, _shell, mut app) = app_fixture();
    let sync = app.sync_mut();
    assert!(
        !sync.bind_addr().ip().is_unspecified() || sync.bind_addr().port() == 7420,
        "the default names the pairing port"
    );
    assert_eq!(sync.bind_addr().to_string(), "0.0.0.0:7420");
}

#[test]
fn configuring_the_socket_replaces_the_default() {
    let (_dir, _shell, mut app) = app_fixture();
    let bind: std::net::SocketAddr = "0.0.0.0:9999".parse().expect("addr");
    let ip: std::net::IpAddr = "192.168.1.42".parse().expect("ip");
    app.configure_sync(bind, Some(ip));
    let sync = app.sync_mut();
    assert_eq!(sync.bind_addr(), bind);
    assert_eq!(
        sync.ticket_addr(),
        std::net::SocketAddr::new(ip, 9999),
        "the ticket is right before a socket is ever opened — that is \
         what gets pasted into the other machine"
    );
}

#[test]
fn a_wide_bind_pairs_end_to_end_over_a_real_socket() {
    // The whole point, exercised the way two machines exercise it: bind
    // every interface, hand over the ticket, and have the peer dial the
    // address the ticket names — not the one we bound. On a host with
    // no route out, detection falls back to loopback and this still
    // pairs; what it must never do is dial `0.0.0.0`.
    let (_dir_a, shell_a) = vault_with("1", "Mine");
    let (_dir_b, shell_b) = vault_with("2", "Theirs");

    let mut server = SyncApp::with_bind("a", "0.0.0.0:0".parse().expect("addr"), None);
    server.listen().expect("bound");
    server.snapshot(&shell_a);

    let mut client = SyncApp::new("b", "127.0.0.1:0".parse().expect("addr"));
    client.snapshot(&shell_b);

    // Each side trusts exactly the key in the ticket it was handed.
    client.add_peer(&server.ticket()).expect("their ticket");
    server.add_peer(&client.ticket()).expect("our ticket");
    server
        .inbound_ready()
        .expect("a trusted peer makes the public listener answer");

    let dial = closure_sync::SyncTicket::decode(&server.ticket())
        .expect("ticket decodes")
        .addr;
    assert!(
        !dial.ip().is_unspecified(),
        "the ticket must name something connect(2) accepts, got {dial}"
    );

    let listener = server.listener().expect("listening");
    let mut server_session = server.session().clone();
    let server_key = server.signing_key().clone();
    let server_trusts = server.trusted_keys();
    let accepted = std::thread::spawn(move || {
        closure_sync::TcpSyncTransport::serve_once_secure(
            &listener,
            &mut server_session,
            &server_key,
            &server_trusts,
        )
        .map(|()| server_session)
    });

    let mut client_session = client.session().clone();
    closure_sync::TcpSyncTransport::connect_and_sync_secure(
        dial,
        &mut client_session,
        client.signing_key(),
        &client.trusted_keys(),
    )
    .expect("dialled the address the ticket named");
    let server_session = accepted.join().expect("thread").expect("served");

    // Both replicas now hold both blocks: the round converged.
    let ours: Vec<String> = client_session
        .block_ids()
        .map(ToString::to_string)
        .collect();
    let theirs: Vec<String> = server_session
        .block_ids()
        .map(ToString::to_string)
        .collect();
    assert_eq!(ours.len(), 2, "client sees both blocks: {ours:?}");
    assert_eq!(theirs.len(), 2, "server sees both blocks: {theirs:?}");
}

#[test]
fn an_untrusted_stranger_is_refused_by_a_paired_listener() {
    // The other half of widening the bind: once a peer is trusted, a
    // *third* party's frames are rejected on the wire rather than
    // merged into the vault.
    let (_dir, shell) = vault_with("1", "Mine");
    let mut server = SyncApp::with_bind("a", "127.0.0.1:0".parse().expect("addr"), None);
    server.listen().expect("bound");
    server.snapshot(&shell);
    let friend = SyncApp::new("b", "127.0.0.1:1".parse().expect("addr"));
    server.add_peer(&friend.ticket()).expect("their ticket");

    let dial = server.ticket_addr();
    let listener = server.listener().expect("listening");
    let mut server_session = server.session().clone();
    let server_key = server.signing_key().clone();
    let server_trusts = server.trusted_keys();
    let round = std::thread::spawn(move || {
        closure_sync::TcpSyncTransport::serve_once_secure(
            &listener,
            &mut server_session,
            &server_key,
            &server_trusts,
        )
    });

    let (_sdir, stranger_shell) = vault_with("9", "Injected");
    let stranger = SyncApp::new("mallory", "127.0.0.1:2".parse().expect("addr"));
    let mut stranger_session = stranger.session().clone();
    stranger_session.record_local(
        &closure_core::Document::load_str(
            "* Injected\n:PROPERTIES:\n:ID: 01HQSYNC000000000000009\n:END:\n",
        )
        .expect("parse"),
    );
    let _ = &stranger_shell;
    // The stranger's frame is well-formed and correctly self-signed —
    // it is simply signed by a key nobody pasted.
    let _ = closure_sync::TcpSyncTransport::connect_and_sync_secure(
        dial,
        &mut stranger_session,
        stranger.signing_key(),
        &[],
    );
    let refusal = round
        .join()
        .expect("thread")
        .expect_err("an unknown signer must not reach the vault");
    assert!(
        refusal.to_string().contains("untrusted"),
        "and refused for that reason, not by accident: {refusal}"
    );
}

#[test]
fn a_vault_bigger_than_one_frame_pairs_over_a_real_socket() {
    // "P2P push: ~/vault seems to be too big". The replica crossed as a
    // single Noise message, which holds 64 KiB minus its tag, so past
    // that size the push failed rather than taking two messages.
    let (_dir_a, shell_a) = vault_with("1", "Mine");
    let (_dir_b, shell_b) = vault_with("2", "Theirs");

    let mut server = SyncApp::with_bind("a", "127.0.0.1:0".parse().expect("addr"), None);
    server.listen().expect("bound");
    server.snapshot(&shell_a);
    // Pad the replica well past a single frame.
    {
        let session = server.session_mut();
        for i in 0..400 {
            let id = format!("01HQPAD{i:019}");
            let body = "y".repeat(500);
            let doc = closure_core::Document::load_str(&format!(
                "* Padded {i}\n:PROPERTIES:\n:ID: {id}\n:END:\n{body}\n"
            ))
            .expect("parse");
            session.record_local(&doc);
        }
    }

    let mut client = SyncApp::new("b", "127.0.0.1:0".parse().expect("addr"));
    client.snapshot(&shell_b);
    client.add_peer(&server.ticket()).expect("their ticket");
    server.add_peer(&client.ticket()).expect("our ticket");

    let dial = server.ticket_addr();
    let listener = server.listener().expect("listening");
    let mut server_session = server.session().clone();
    let server_key = server.signing_key().clone();
    let server_trusts = server.trusted_keys();
    let round = std::thread::spawn(move || {
        closure_sync::TcpSyncTransport::serve_once_secure(
            &listener,
            &mut server_session,
            &server_key,
            &server_trusts,
        )
    });

    let mut client_session = client.session().clone();
    closure_sync::TcpSyncTransport::connect_and_sync_secure(
        dial,
        &mut client_session,
        client.signing_key(),
        &client.trusted_keys(),
    )
    .expect("a big vault still crosses");
    round.join().expect("thread").expect("served");
    assert!(
        client_session.block_ids().count() > 400,
        "the padded blocks arrived"
    );
}
