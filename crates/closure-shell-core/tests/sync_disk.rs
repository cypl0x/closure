//! Syncing through a shared directory instead of a socket.
//!
//! The socket path needs both machines up at once and a route between
//! them. A directory both of them can see — Syncthing, a Dropbox, a USB
//! stick, a mounted share — needs neither: one side leaves a bundle,
//! the other picks it up whenever it next runs.
//!
//! It is the same replica and the same trust anchor as the wire: a
//! bundle is the signed `SyncMessage` the socket would have sent, and
//! it is merged only when it is signed by a peer whose ticket has been
//! pasted. A file in a shared folder is exactly as trustworthy as
//! whoever can write to the folder, which is why it is verified rather
//! than believed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell, SyncApp};
use closure_store::Vault;

fn vault_with(name: &str, title: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* {title}\n:PROPERTIES:\n:ID: 01HQDISK0000000000000{name}\n:END:\n"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

/// Two apps that have paired with each other, their vaults, and the
/// folder they both write into.
struct Pair {
    drop: tempfile::TempDir,
    a: SyncApp,
    shell_a: Shell,
    b: SyncApp,
    shell_b: Shell,
    /// The vault directories, kept alive for the test's lifetime.
    _vaults: (tempfile::TempDir, tempfile::TempDir),
}

fn paired() -> Pair {
    let (da, shell_a) = vault_with("01A", "Alpha");
    let (db, shell_b) = vault_with("01B", "Beta");
    let mut a = SyncApp::new("a", "127.0.0.1:7101".parse().expect("addr"));
    let mut b = SyncApp::new("b", "127.0.0.1:7102".parse().expect("addr"));
    a.add_peer(&b.ticket()).expect("pair a→b");
    b.add_peer(&a.ticket()).expect("pair b→a");
    Pair {
        drop: tempfile::tempdir().expect("drop"),
        a,
        shell_a,
        b,
        shell_b,
        _vaults: (da, db),
    }
}

#[test]
fn a_bundle_left_in_the_folder_is_picked_up_by_the_other_side() {
    let mut p = paired();
    p.a.snapshot(&p.shell_a);
    p.b.snapshot(&p.shell_b);
    p.a.export_bundle(p.drop.path()).expect("export");

    let (merged, conflicts) = p.b.import_bundles(p.drop.path()).expect("import");
    assert_eq!(merged, 1, "one bundle read");
    assert!(conflicts.is_empty(), "different blocks do not conflict");
    let ids: Vec<String> = p.b.session().block_ids().map(ToString::to_string).collect();
    assert_eq!(ids.len(), 2, "both blocks now: {ids:?}");
}

#[test]
fn our_own_bundle_is_not_read_back() {
    // Both sides write into the same folder, and a replica that merges
    // itself would count a round that did nothing.
    let mut p = paired();
    p.a.snapshot(&p.shell_a);
    p.a.export_bundle(p.drop.path()).expect("export");
    let (merged, _) = p.a.import_bundles(p.drop.path()).expect("import");
    assert_eq!(merged, 0, "ours is ours");
}

#[test]
fn a_bundle_from_a_stranger_is_refused() {
    // The folder is as trustworthy as whoever can write to it, so the
    // signature is what decides — the same rule the socket has.
    let mut p = paired();
    let (_dc, shell_c) = vault_with("01C", "Gamma");
    let mut stranger = SyncApp::new("c", "127.0.0.1:7103".parse().expect("addr"));
    stranger.snapshot(&shell_c);
    stranger.export_bundle(p.drop.path()).expect("export");
    p.a.snapshot(&p.shell_a);
    p.b.snapshot(&p.shell_b);

    let (merged, _) = p.b.import_bundles(p.drop.path()).expect("import");
    assert_eq!(merged, 0, "nobody pasted their ticket");
    let ids: Vec<String> = p.b.session().block_ids().map(ToString::to_string).collect();
    assert!(
        !ids.iter().any(|id| id.contains("01C")),
        "and nothing of theirs got in: {ids:?}"
    );
}

#[test]
fn a_directory_with_nothing_in_it_is_not_an_error() {
    let mut p = paired();
    p.b.snapshot(&p.shell_b);
    let (merged, conflicts) = p.b.import_bundles(p.drop.path()).expect("import");
    assert_eq!(merged, 0);
    assert!(conflicts.is_empty());
}

#[test]
fn a_corrupt_bundle_is_skipped_rather_than_fatal() {
    // A half-written file is what a folder sync *does* while it is
    // copying; the next round has to still work.
    let mut p = paired();
    p.a.snapshot(&p.shell_a);
    p.a.export_bundle(p.drop.path()).expect("export");
    fs::write(p.drop.path().join("half.closure-sync"), b"not a bundle").expect("write");
    p.b.snapshot(&p.shell_b);
    let (merged, _) = p.b.import_bundles(p.drop.path()).expect("import");
    assert_eq!(merged, 1, "the good one still landed");
}

#[test]
fn exporting_twice_replaces_our_bundle_rather_than_piling_up() {
    let mut p = paired();
    p.a.snapshot(&p.shell_a);
    p.a.export_bundle(p.drop.path()).expect("first");
    p.a.export_bundle(p.drop.path()).expect("second");
    let count = fs::read_dir(p.drop.path())
        .expect("read")
        .filter_map(Result::ok)
        .count();
    assert_eq!(count, 1, "one file per replica, kept up to date");
}

// === the two commands, over a vault ===

#[test]
fn the_commands_carry_a_rename_from_one_vault_to_the_other() {
    // What the feature is for, end to end: no socket, no route, no
    // second machine awake — just a folder and two runs of the app.
    let dir_a = tempfile::tempdir().expect("a");
    let dir_b = tempfile::tempdir().expect("b");
    let drop = tempfile::tempdir().expect("drop");
    let note = "* Shared\n:PROPERTIES:\n:ID: 01HQDISK00000000000000RT\n:END:\nbody\n";
    for (dir, other) in [(&dir_a, &dir_b), (&dir_b, &dir_a)] {
        let _ = other;
        fs::write(dir.path().join("notes.org"), note).expect("write");
        fs::write(
            dir.path().join("config.org"),
            format!(
                "#+BEGIN_SRC closure-config\nsync_dir = {}\n#+END_SRC\n",
                drop.path().display()
            ),
        )
        .expect("config");
    }
    let mut shell_a = Shell::new(Vault::open(dir_a.path()).expect("a"));
    let mut shell_b = Shell::new(Vault::open(dir_b.path()).expect("b"));
    let mut app_a = ModalApp::new(InputMode::Doom);
    let mut app_b = ModalApp::new(InputMode::Doom);
    // Pair once, the way the socket path pairs.
    let ticket_b = app_b.sync_mut().ticket();
    let ticket_a = app_a.sync_mut().ticket();
    app_a.sync_mut().add_peer(&ticket_b).expect("pair");
    app_b.sync_mut().add_peer(&ticket_a).expect("pair");

    // One round first, so both replicas share a base for the block.
    // Two machines that have never exchanged anything each stamp what
    // they parsed with their own clock, and "who is newer" is then a
    // race between two file reads rather than a statement about edits.
    // This is the same handshake the socket path does on its first
    // round; here it is two files.
    app_a.run(&mut shell_a, "sync-export");
    app_b.run(&mut shell_b, "sync-import");
    app_b.run(&mut shell_b, "sync-export");
    app_a.run(&mut shell_a, "sync-import");

    let id = closure_core::BlockId::from_existing("01HQDISK00000000000000RT");
    shell_a.rename_headline(&id, "Renamed on A").expect("rename");
    app_a.run(&mut shell_a, "sync-export");
    assert!(
        app_a.status().contains("bundle"),
        "it said what it did: {}",
        app_a.status()
    );

    app_b.run(&mut shell_b, "sync-import");
    let (headline, _) = shell_b.vault.find_by_id(&id).expect("still there");
    assert_eq!(
        headline.title(),
        "Renamed on A",
        "the rename crossed on a file: {}",
        app_b.status()
    );
}

#[test]
fn the_commands_say_so_when_no_folder_is_configured() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    let mut shell = Shell::new(Vault::open(dir.path()).expect("open"));
    let mut app = ModalApp::new(InputMode::Doom);
    for command in ["sync-export", "sync-import"] {
        app.run(&mut shell, command);
        assert!(
            app.status().contains("sync_dir"),
            "{command} names the key to set: {}",
            app.status()
        );
    }
}

#[test]
fn a_divergent_title_is_reported_from_a_bundle_too() {
    let mut p = paired();
    let (_dc, shell_b2) = vault_with("01A", "Alpha renamed elsewhere");
    p.a.snapshot(&p.shell_a);
    p.b.snapshot(&shell_b2);
    p.a.export_bundle(p.drop.path()).expect("export");
    let (_, conflicts) = p.b.import_bundles(p.drop.path()).expect("import");
    assert_eq!(conflicts.len(), 1, "surfaced, not resolved: {conflicts:?}");
}
