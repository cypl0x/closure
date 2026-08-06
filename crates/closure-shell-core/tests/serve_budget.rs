//! Adjacent to "move the P2P session off the UI thread", and found
//! while doing it.
//!
//! `serve_pending` accepts *every* waiting connection in one go, and
//! each one it accepts is now bounded at 200ms rather than forever —
//! which is a fix, and not enough on its own. Ten hosts that connect
//! and say nothing is ten times 200ms on the thread that draws the
//! window: two seconds, from a port anyone able to reach it can knock
//! on.
//!
//! A tick serves what it can afford and leaves the rest for the next
//! one. They are still there: the listener does not drop them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::SyncApp;

#[test]
fn a_crowd_of_silent_callers_does_not_own_the_thread() {
    let mut app = SyncApp::new("host", "127.0.0.1:0".parse().unwrap());
    let addr = app.listen().expect("listen");

    // Ten callers that connect and never speak.
    // Held, not dropped: a caller that hangs up is not the case.
    let mut callers = Vec::new();
    for _ in 0..10 {
        callers.push(std::net::TcpStream::connect(addr).expect("connect"));
    }
    assert_eq!(callers.len(), 10);

    let started = std::time::Instant::now();
    app.serve_pending();
    let took = started.elapsed();
    assert!(
        took < std::time::Duration::from_millis(900),
        "one tick spent {took:?} serving callers that said nothing"
    );
}

#[test]
fn a_caller_left_over_is_served_by_a_later_tick() {
    // The other half: bounding the work must not lose the work.
    let mut app = SyncApp::new("host", "127.0.0.1:0".parse().unwrap());
    let addr = app.listen().expect("listen");
    let mut callers = Vec::new();
    for _ in 0..6 {
        callers.push(std::net::TcpStream::connect(addr).expect("connect"));
    }

    let mut ticks = 0;
    // Each tick times out on the silent callers it takes; five ticks
    // is enough to have reached all six if the leftovers survive.
    for _ in 0..5 {
        app.serve_pending();
        ticks += 1;
    }
    assert_eq!(ticks, 5);
    assert_eq!(callers.len(), 6, "a caller was lost");
}
