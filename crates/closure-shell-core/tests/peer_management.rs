//! "[#A] We have to drastically improve the Peers interface (g s)":
//!
//! - "What does the listen button really do? It does something, but
//!   what? What happens if I press ('toggle' can't see it UI wise.
//!   Awful UX currently)."
//! - "Is the peer ticket input field append only to the config? We may
//!   need some add/deactivate/delete UI component"
//!
//! Both are the same complaint: the pane could add and could not
//! answer. Pairing was a one-way door — a ticket pasted by accident,
//! or a machine that no longer exists, stayed in `config.org` forever
//! and there was no way to see or change that from the screen it was
//! added on.
//!
//! And the listen button was a verb with no state beside it: pressing
//! it bound a socket and said nothing, so pressing it again looked
//! identical to not having pressed it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01PEERMGMT000000000000AA\n:END:\nbody\n";

const ONE: &str = "closure-sync:192.168.2.204:7420|\
6f09318c6edbe96521bcdb2f9ccee6bae79ee509e320b34eeb82dd20caf31d38";
const TWO: &str = "closure-sync:172.20.10.7:7420|\
db308fc821543fc03782ad1b72f4704d44cb9f294052c1b326113af8e9edea95";

fn app_with(peers: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!("#+BEGIN_SRC closure-config\nsync_peers = {peers}\n#+END_SRC\n"),
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(closure_config::InputMode::Doom);
    app.load_peers(&shell);
    (dir, shell, app)
}

#[test]
fn a_peer_can_be_forgotten() {
    let (_d, shell, mut app) = app_with(&format!("{ONE}, {TWO}"));
    assert_eq!(app.sync_mut().peers().len(), 2);
    app.forget_peer(0, &shell);
    assert_eq!(
        app.sync_mut().peers().len(),
        1,
        "the peer is still there after being forgotten"
    );
}

#[test]
fn forgetting_a_peer_survives_a_restart() {
    // The whole point: it was append-only to config.org, so removing
    // one in the session left it to come back on the next launch.
    let (dir, shell, mut app) = app_with(&format!("{ONE}, {TWO}"));
    app.forget_peer(0, &shell);

    let source = std::fs::read_to_string(dir.path().join("config.org")).unwrap();
    assert!(
        !source.contains("192.168.2.204"),
        "the forgotten peer is still in config.org:\n{source}"
    );
    assert!(
        source.contains("172.20.10.7"),
        "forgetting one peer removed the other:\n{source}"
    );
}

#[test]
fn forgetting_the_last_peer_leaves_a_readable_config() {
    let (dir, shell, mut app) = app_with(ONE);
    app.forget_peer(0, &shell);
    let source = std::fs::read_to_string(dir.path().join("config.org")).unwrap();
    closure_config::Config::from_org_source(&source).expect("config.org still parses");
    assert_eq!(app.sync_mut().peers().len(), 0);
}

#[test]
fn forgetting_a_peer_that_is_not_there_is_harmless() {
    let (_d, shell, mut app) = app_with(ONE);
    app.forget_peer(9, &shell);
    assert_eq!(app.sync_mut().peers().len(), 1);
}

#[test]
fn the_pane_can_say_whether_it_is_listening() {
    // "What does the listen button really do? It does something, but
    // what?" — it binds a socket. Nothing said so, and nothing said
    // whether it already had.
    let (_d, shell, mut app) = app_with(ONE);
    assert!(
        app.listening_on().is_none(),
        "claims to be listening before anything was bound"
    );
    app.sync_mut().listen().expect("bind");
    let _ = &shell;
    assert!(
        app.listening_on().is_some(),
        "bound a socket and still reports nothing"
    );
}
