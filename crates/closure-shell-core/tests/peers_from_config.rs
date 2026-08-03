//! "I have commented out the `sync_peers`. I can't see any in the Peers
//! interface after restart even with them defined like that in the
//! config."
//!
//! Pairing that has to be redone every session is not pairing, which
//! is why the tickets live in `config.org` in the first place. If they
//! do not come back on the next launch, the file is decoration.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01PEERCFG0000000000000AAA\n:END:\nbody\n";

/// The exact line from the report, two peers on one key.
const TWO: &str = "closure-sync:192.168.2.204:7420|\
6f09318c6edbe96521bcdb2f9ccee6bae79ee509e320b34eeb82dd20caf31d38, \
closure-sync:172.20.10.7:7420|\
db308fc821543fc03782ad1b72f4704d44cb9f294052c1b326113af8e9edea95";

fn vault_with(peers: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!("#+BEGIN_SRC closure-config\nsync_peers = {peers}\n#+END_SRC\n"),
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell)
}

#[test]
fn peers_named_in_the_config_are_there_after_a_restart() {
    let (_d, shell) = vault_with(TWO);
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    assert_eq!(
        app.sync_mut().peers().len(),
        2,
        "the config named two peers and the shell has none"
    );
}

#[test]
fn the_peers_pane_lists_them() {
    // Not just held — *shown*. The report is about the interface.
    let (_d, mut shell) = vault_with(TWO);
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    app.run(&mut shell, "sync");
    // What the pane paints from.
    let peers = app
        .sync()
        .expect("sync is initialised by the command")
        .peers();
    assert_eq!(peers.len(), 2, "the pane would show {} of 2", peers.len());
    assert!(
        peers
            .iter()
            .any(|p| p.addr.to_string().contains("192.168.2.204")),
        "a peer's address is not there"
    );
}

#[test]
fn a_second_load_does_not_double_them() {
    // The config is re-read on a poll, and pairing twice with the same
    // key must stay one peer.
    let (_d, shell) = vault_with(TWO);
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    app.load_peers(&shell);
    assert_eq!(app.sync_mut().peers().len(), 2);
}
