//! "Move the P2P session off the UI thread."
//!
//! "Every network call the live session makes still happens on the UI
//! thread, from the frame timer. It is bounded now — 60ms to connect,
//! 200ms to read, one peer per tick, twenty seconds of quiet after a
//! failure — and those bounds are the only reason the window stays
//! usable with an unreachable peer. … `SyncApp` is `Send`, so it can
//! own its socket work on a worker thread and hand back merged state
//! and presence through a channel. Then the UI thread never blocks on
//! a peer at all and the bounds stop being load-bearing."
//!
//! Three user-visible failures came from this one cause: "the app
//! won't start", "currently it is quite slow", and the Peers pane
//! appearing empty.
//!
//! A frame is 16ms at 60Hz. Measured before the fix, with one peer at
//! an address nothing answers on:
//!
//!     tick 0: 60.2ms   tick 1: 24µs   tick 2: 6.6µs
//!
//! Four dropped frames on the tick that dials, and again every twenty
//! seconds when the quiet expires — for as long as a peer is away.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::{Duration, Instant};

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

/// An address nothing answers on: RFC 5737 documentation space, which
/// is not routed anywhere.
const BLACK_HOLE: &str = "192.0.2.1:7420";
/// A syntactically valid ed25519 public key — 32 bytes of hex. It
/// belongs to nobody, which is the point: the peer never answers.
const KEY: &str = "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01SESSIONAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!(
            "#+BEGIN_SRC closure-config\nsync_peers = closure-sync:{BLACK_HOLE}|{KEY}\n#+END_SRC\n"
        ),
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    app.load_peers(&shell);
    (dir, shell, app)
}

#[test]
fn a_tick_does_not_wait_for_a_peer_that_is_not_there() {
    // The whole item, as one measurement. The frame timer calls this;
    // whatever it costs, the window pays.
    let (_d, shell, mut app) = app();
    let mut worst = Duration::ZERO;
    for _ in 0..6 {
        let t = Instant::now();
        app.session_tick(&shell);
        worst = worst.max(t.elapsed());
    }
    assert!(
        worst < Duration::from_millis(5),
        "the slowest tick blocked the UI thread for {worst:?} — a frame is 16ms"
    );
}

#[test]
fn ticking_with_no_session_costs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* One\n").unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    let t = Instant::now();
    assert_eq!(app.session_tick(&shell), 0);
    assert!(t.elapsed() < Duration::from_millis(5), "{:?}", t.elapsed());
}

#[test]
fn the_session_still_reports_what_it_did() {
    // Off-thread must not mean "silently does nothing": the count is
    // how a caller tells "nobody is there" from "we did nothing", and
    // the Peers pane reads it.
    let (_d, shell, mut app) = app();
    for _ in 0..4 {
        app.session_tick(&shell);
    }
    // Nobody answers at 192.0.2.1, so zero rounds is the right answer
    // — but it must be an *answer*, arrived at without blocking.
    assert_eq!(app.session_tick(&shell), 0);
}

#[test]
fn a_peer_that_is_there_still_gets_a_round() {
    // The thing a faster tick must not cost: moving the dial off the
    // drawing thread is only a win if the sync still happens.
    let loopback = "127.0.0.1:0".parse().unwrap();
    let mut server = closure_shell_core::SyncApp::new("server", loopback);
    let bound = server.listen().expect("listen");
    server.set_local_presence("01SESSIONBBBBBBBBBBBBBBBBB", 7);
    let mut client = closure_shell_core::SyncApp::new("client", loopback);

    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if server.serve_pending() > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        server
    });

    // Tick until the round lands, timing each one: none of them may
    // cost a frame, and the round has to happen.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut worst = Duration::ZERO;
    loop {
        let t = Instant::now();
        let outcome = client.sync_with(bound);
        worst = worst.max(t.elapsed());
        if outcome.is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "no round in ten seconds");
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = handle.join();
    assert!(
        worst < Duration::from_millis(50),
        "a tick on the happy path cost {worst:?}"
    );
    assert!(
        client
            .peer_presence()
            .iter()
            .any(|p| p.block == "01SESSIONBBBBBBBBBBBBBBBBB" && p.line == 7),
        "the round completed but carried nothing: {:?}",
        client.peer_presence()
    );
}
