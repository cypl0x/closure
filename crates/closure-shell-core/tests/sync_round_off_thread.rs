//! "Move the P2P session off the UI thread" — the half that was left.
//!
//! The dial moved to a worker and the tick that cost 60.2ms came down
//! to 87.9µs. What stayed on the drawing thread was the round itself:
//! write our bundle, wait up to `READ_BUDGET` for theirs, exchange
//! presence. For a host that accepts and then never speaks the
//! protocol — a wrong port, some other service, a peer mid-restart —
//! that is 200ms on the thread that paints, which is twelve dropped
//! frames.
//!
//! It decomposes: the bytes are CPU and the wire is IO. `from_session`
//! and `apply_message` need the CRDT and touch no socket; writing and
//! reading need the socket and no CRDT. So the session stays on the
//! caller's thread and only the wire goes to the worker.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{Shell, SyncApp};
use closure_store::Vault;

/// A listener that accepts and then says nothing at all — the failure
/// this is about.
fn silent_peer() -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        // Hold the accepted connection open, and never write.
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept() {
            held.push(stream);
            if held.len() > 4 {
                return;
            }
        }
    });
    (addr, handle)
}

fn app() -> (tempfile::TempDir, Shell, SyncApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* A\n").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let shell = Shell::new(vault);
    (
        dir,
        shell,
        SyncApp::new("test", "127.0.0.1:0".parse().unwrap()),
    )
}

/// The longest a single call may take. A frame at 60Hz is 16ms; this is
/// generous enough not to be flaky on a loaded machine and far below
/// the 200ms read budget it is there to keep off this thread.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(8);

#[test]
fn a_peer_that_accepts_and_never_speaks_costs_the_caller_nothing() {
    let (_d, _shell, mut app) = app();
    let (addr, _peer) = silent_peer();
    // Several ticks: the first dials, a later one finds the socket
    // open and starts the round, and none of them may block.
    for tick in 0..40 {
        let started = std::time::Instant::now();
        let _ = app.sync_with(addr);
        let took = started.elapsed();
        assert!(
            took < BUDGET,
            "tick {tick} spent {took:?} on the caller's thread"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_peer_that_is_not_there_still_costs_nothing() {
    // The case already fixed, asserted so the rework cannot undo it.
    let (_d, _shell, mut app) = app();
    // 198.51.100.0/24 is TEST-NET-2: reserved, and routed nowhere.
    let addr: std::net::SocketAddr = "198.51.100.7:7420".parse().unwrap();
    for _ in 0..5 {
        let started = std::time::Instant::now();
        let _ = app.sync_with(addr);
        assert!(started.elapsed() < BUDGET, "{:?}", started.elapsed());
    }
}

#[test]
fn a_real_round_still_happens() {
    // The point of all this is that the sync still works.
    //
    // The server runs on its own thread, which is what two peers are:
    // two processes. Serving from the same thread that ticks the
    // client deadlocks the *test* — the server blocks on a read the
    // client cannot reach the code to satisfy — and that is an
    // artefact of the harness, not of the protocol.
    let (_d1, _s1, mut server) = app();
    let (_d2, _s2, mut client) = app();
    let addr = server.listen().expect("listen");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let serving = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                server.serve_pending();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut done = false;
    while std::time::Instant::now() < deadline && !done {
        done = client.sync_with(addr).is_ok();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = serving.join();
    assert!(done, "the round never completed");
}
