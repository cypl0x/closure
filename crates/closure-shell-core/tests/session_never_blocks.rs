//! The window froze on a vault whose `sync_peers` names a machine that
//! is not there.
//!
//! Reported as "the app won't start with my vault". It started; it was
//! wedged. `session_tick` runs on the frame timer — that is what makes
//! the session continuous — and it dialled each peer with
//! `TcpStream::connect`, which has *no timeout*. A peer that refuses
//! costs a millisecond; a peer that is simply absent, on a network
//! that drops rather than refuses, costs the OS default of about two
//! minutes. On the UI thread.
//!
//! Measured against the user's own vault on :1: with their
//! `sync_peers` line, two keypresses two seconds apart changed exactly
//! zero pixels. With the line removed, the same vault and the same
//! build repainted normally. I introduced this in 56713cf.
//!
//! So: a bounded connect, and a peer that has just failed is not
//! retried on the very next tick.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01FROZEN00000000000000000\n:END:\nbody\n";

/// An address that is routable but has nothing listening. `192.0.2.0/24`
/// is TEST-NET-1 (RFC 5737): reserved for documentation, so packets to
/// it are dropped rather than refused — which is the case that hung.
const BLACK_HOLE: &str = "closure-sync:192.0.2.1:7420|\
6f09318c6edbe96521bcdb2f9ccee6bae79ee509e320b34eeb82dd20caf31d38";

fn vault_with_peer() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!("#+BEGIN_SRC closure-config\nsync_peers = {BLACK_HOLE}\n#+END_SRC\n"),
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell)
}

#[test]
fn a_tick_against_an_unreachable_peer_returns_promptly() {
    // The budget is generous — this asserts "does not hang", not a
    // frame time, so it cannot go flaky on a loaded machine. Before
    // the fix this took the OS connect timeout: minutes.
    let (_dir, shell) = vault_with_peer();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);

    let start = std::time::Instant::now();
    app.session_tick(&shell);
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(3),
        "one tick took {took:?} on the UI thread"
    );
}

#[test]
fn a_peer_that_just_failed_is_not_dialled_again_immediately() {
    // Even a bounded connect costs its budget every time. The frame
    // timer fires every 1.5s, so without a backoff an absent peer
    // stutters the window for as long as it is absent.
    let (_dir, shell) = vault_with_peer();
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    app.session_tick(&shell);

    let start = std::time::Instant::now();
    for _ in 0..5 {
        app.session_tick(&shell);
    }
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_millis(200),
        "five ticks after a failure took {took:?} — the peer is being redialled every time"
    );
}

#[test]
fn a_vault_with_no_peers_costs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    let start = std::time::Instant::now();
    for _ in 0..20 {
        app.session_tick(&shell);
    }
    assert!(start.elapsed() < std::time::Duration::from_millis(200));
}
