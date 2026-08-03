//! A peer you paired with has to still be you tomorrow.
//!
//! `SyncApp::new` called `generate_key()`, so every launch minted a
//! fresh ed25519 identity. That makes the whole pairing story a lie
//! that only holds while the window stays open:
//!
//! - the ticket you handed someone stops matching you the moment you
//!   restart;
//! - `sync_peers` in config.org — which is the documented way to pair
//!   without retyping a ticket — is stale the first time either side
//!   is reopened;
//! - and because frames are verified against the *trusted* key, a peer
//!   that reconnects is not merely unrecognised, it is refused.
//!
//! The identity therefore lives in the vault, next to the notes it
//! speaks for.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::SyncApp;

fn addr() -> std::net::SocketAddr {
    "127.0.0.1:7420".parse().unwrap()
}

#[test]
fn reopening_a_vault_keeps_the_same_identity() {
    let dir = tempfile::tempdir().unwrap();

    let mut first = SyncApp::new("me", addr());
    first.load_identity(dir.path());
    let before = first.ticket();

    let mut second = SyncApp::new("me", addr());
    second.load_identity(dir.path());
    let after = second.ticket();

    assert_eq!(
        before, after,
        "the ticket changed across a restart, so every pairing is void"
    );
}

#[test]
fn two_vaults_are_two_identities() {
    // The other half: the key belongs to the vault, not to the binary.
    // Two vaults on one machine must not impersonate each other.
    let one = tempfile::tempdir().unwrap();
    let two = tempfile::tempdir().unwrap();

    let mut a = SyncApp::new("a", addr());
    a.load_identity(one.path());
    let mut b = SyncApp::new("b", addr());
    b.load_identity(two.path());

    assert_ne!(a.public_key().to_bytes(), b.public_key().to_bytes());
}

#[test]
fn the_key_is_written_once_and_then_only_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = SyncApp::new("me", addr());
    app.load_identity(dir.path());

    let path = dir.path().join(".closure").join("identity.key");
    assert!(path.is_file(), "no key was written");
    let written = std::fs::read(&path).unwrap();

    let mut again = SyncApp::new("me", addr());
    again.load_identity(dir.path());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        written,
        "the second open rewrote the identity"
    );
}

#[test]
fn the_identity_is_not_an_org_file_in_the_outline() {
    // It lives under `.closure/`, so it is not a note, does not appear
    // in the vault listing, and does not get committed as content.
    let dir = tempfile::tempdir().unwrap();
    let mut app = SyncApp::new("me", addr());
    app.load_identity(dir.path());
    let stray: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|e| e == "org")
        })
        .collect();
    assert!(
        stray.is_empty(),
        "the identity landed in the vault: {stray:?}"
    );
}

#[test]
fn a_corrupt_identity_does_not_stop_the_shell_opening() {
    // A truncated or hand-edited key file must degrade to "a new
    // identity" rather than to a shell that will not start. Losing
    // pairings is recoverable; not opening the vault is not.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".closure")).unwrap();
    std::fs::write(dir.path().join(".closure").join("identity.key"), b"nope").unwrap();

    let mut app = SyncApp::new("me", addr());
    app.load_identity(dir.path());
    assert!(!app.ticket().is_empty(), "the shell has no identity at all");
}
