//! "Ideally I have a Google Doc like live cursor and edit where I can
//! see where the other person is editing. If that's too much you can
//! fallback to Notion like collaboration: No live cursor but you can
//! see where some stuff 'spawns' if someone else is editing the
//! document. If this is still too much manual sync (is it like that
//! right now?)"
//!
//! It was manual, and more manual than the question supposes. Pairing
//! is real (tickets, ed25519 trust), the transport is real (TCP,
//! Noise), the CRDT merge is real — but nothing in the running shell
//! ever opened a connection. `SyncApp::listen` was called from tests
//! and from nowhere else. What the app actually did was write bundle
//! files into a shared folder and read them back when you pressed a
//! key: not a peer-to-peer session at all, a dropbox with extra steps.
//!
//! And `Presence` — the type that carries which block a peer is on and
//! what line they are on inside it — was encoded, decoded, tested, and
//! never sent by anyone. `with_presence`, which paints the badge, had
//! no callers.
//!
//! So this is the session: two shells connect, exchange their
//! documents *and* where each of them is, and keep doing it without
//! being asked.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell, SyncApp};
use closure_store::Vault;

const ORG: &str = "\
* Alpha
:PROPERTIES:
:ID: 01LIVEAAAAAAAAAAAAAAAAAAAA
:END:
first body

* Beta
:PROPERTIES:
:ID: 01LIVEBBBBBBBBBBBBBBBBBBBB
:END:
second body
";

/// A one-file vault, kept alive by the returned directory.
fn vault() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell)
}

#[test]
fn a_peer_that_is_reading_a_block_is_visible_on_that_row() {
    // The Notion level, stated exactly: you can see *where* someone
    // else is. The badge already existed and nothing ever produced the
    // presence to put in it.
    let (_dir, shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);

    app.note_peer_presence("wap", "01LIVEBBBBBBBBBBBBBBBBBBBB", 3);

    // Asked per row rather than baked into the row. The row list is
    // memoised against the vault revision, and presence changes many
    // times a second without the vault changing at all — folding it in
    // would either break that memo or make every cursor twitch rebuild
    // every row in the vault. (`with_presence` did fold it in, onto
    // `RowView`, which the window does not paint: it had no callers.)
    let rows = app.rows(&shell);
    assert!(
        rows.iter().any(|r| r.id == "01LIVEBBBBBBBBBBBBBBBBBBBB"),
        "the row is there"
    );
    assert!(
        app.peers_on("01LIVEBBBBBBBBBBBBBBBBBBBB")
            .iter()
            .any(|p| p.peer == "wap"),
        "the peer is nowhere on the row"
    );
    assert!(
        app.peers_on("01LIVEAAAAAAAAAAAAAAAAAAAA").is_empty(),
        "the peer is on a row they are not on"
    );
}

#[test]
fn presence_says_which_line_not_only_which_block() {
    // The step from "Notion" towards "Google Doc": the wire type has
    // carried a line number all along and nothing read it.
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.note_peer_presence("wap", "01LIVEBBBBBBBBBBBBBBBBBBBB", 7);
    let at = app.peer_presence();
    assert_eq!(at.len(), 1);
    assert_eq!(at[0].peer, "wap");
    assert_eq!(at[0].block, "01LIVEBBBBBBBBBBBBBBBBBBBB");
    assert_eq!(at[0].line, 7);
}

#[test]
fn a_peer_that_moves_is_in_one_place_not_two() {
    // Presence is a position, not a log. A peer moving from one block
    // to another must not leave a ghost behind on the old row.
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.note_peer_presence("wap", "01LIVEAAAAAAAAAAAAAAAAAAAA", 1);
    app.note_peer_presence("wap", "01LIVEBBBBBBBBBBBBBBBBBBBB", 2);
    let at = app.peer_presence();
    assert_eq!(at.len(), 1, "the peer is in two places: {at:?}");
    assert_eq!(at[0].block, "01LIVEBBBBBBBBBBBBBBBBBBBB");
}

#[test]
fn two_peers_are_two_badges() {
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.note_peer_presence("wap", "01LIVEAAAAAAAAAAAAAAAAAAAA", 1);
    app.note_peer_presence("inari", "01LIVEAAAAAAAAAAAAAAAAAAAA", 4);
    assert_eq!(app.peer_presence().len(), 2);
}

#[test]
fn our_own_cursor_is_reported_for_broadcast() {
    // What we send. The selected row's id and the line the caret is
    // on — nothing else, because presence is session chatter and must
    // never carry document text.
    let (_dir, mut shell) = vault();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.run(&mut shell, "next-file");

    let mine = app.local_presence(&shell).expect("we are somewhere");
    assert_eq!(
        mine.block, "01LIVEBBBBBBBBBBBBBBBBBBBB",
        "we broadcast a block we are not on"
    );
}

#[test]
fn a_live_round_carries_presence_both_ways() {
    // The session itself: two shells, a real socket, and each one ends
    // the round knowing where the other is. This is the part that did
    // not exist — `listen` was called from tests and nowhere else.
    let mut server = SyncApp::new("server", "127.0.0.1:0".parse().unwrap());
    let bound = server.listen().expect("bound");

    let mut client = SyncApp::new("client", "127.0.0.1:0".parse().unwrap());
    client.set_local_presence("01LIVEAAAAAAAAAAAAAAAAAAAA", 2);
    server.set_local_presence("01LIVEBBBBBBBBBBBBBBBBBBBB", 5);

    // `serve_pending` is a poll, not a blocking accept — that is what
    // lets a shell call it from its frame loop without a thread. So the
    // server side polls, exactly as the frame loop does.
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if server.serve_pending() > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        server
    });
    client.sync_with(bound).expect("a live round");
    let server = handle.join().unwrap();

    let seen_by_client = client.peer_presence();
    assert!(
        seen_by_client
            .iter()
            .any(|p| p.block == "01LIVEBBBBBBBBBBBBBBBBBBBB" && p.line == 5),
        "the client did not learn where the server is: {seen_by_client:?}"
    );
    let seen_by_server = server.peer_presence();
    assert!(
        seen_by_server
            .iter()
            .any(|p| p.block == "01LIVEAAAAAAAAAAAAAAAAAAAA" && p.line == 2),
        "the server did not learn where the client is: {seen_by_server:?}"
    );
}

#[test]
fn presence_never_becomes_document_state() {
    // The invariant the wire magic exists to protect: a presence frame
    // must not be merged into the replica as though it were content.
    let mut app = SyncApp::new("a", "127.0.0.1:0".parse().unwrap());
    let before = app.block_count();
    app.set_local_presence("01LIVEAAAAAAAAAAAAAAAAAAAA", 9);
    app.note_peer("elsewhere", "01LIVEBBBBBBBBBBBBBBBBBBBB", 3);
    assert_eq!(
        app.block_count(),
        before,
        "presence created document blocks"
    );
}
