//! The activity rail: every subsystem reachable with one click.
//!
//! closure has a P2P pairing surface, a network sniffer, an assistant,
//! a link graph, a journal and a cron list — and the only way into any
//! of them was a `g`-prefixed chord you had to already know. The status
//! bar carries a few glyph-plus-count indicators, but pairing had *no*
//! mouse entry point at all: a user who never read the keymap could not
//! discover that closure can sync with another machine.
//!
//! The rail is the fix, and it is derived here rather than assembled in
//! a render function so that every shell shows the same destinations in
//! the same order with the same chords (I4: the keymap is the single
//! source of truth), and so what each one *reports* can be asserted
//! without a window.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeSet;
use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const MODES: [InputMode; 5] = [
    InputMode::Doom,
    InputMode::Vim,
    InputMode::Emacs,
    InputMode::Helix,
    InputMode::Notion,
];

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO One\n:PROPERTIES:\n:ID: 01HQRAIL00000000000000001\n:END:\n\
         SCHEDULED: <2026-07-25 Sat>\n\
         body text\n\
         #+BEGIN_SRC sh\necho hi\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

// === the rail names every subsystem ===

#[test]
fn the_rail_carries_every_subsystem_the_user_cannot_otherwise_find() {
    let (_d, shell, app) = fixture(InputMode::Doom);
    let ids: Vec<&str> = app.destinations(&shell).iter().map(|d| d.id).collect();
    for expected in [
        "outline",
        "agenda",
        "blocks",
        "graph",
        "backlinks",
        "journal",
        "cron",
        "peers",
        "sniffer",
        "assistant",
        "conflicts",
        "palette",
    ] {
        assert!(ids.contains(&expected), "missing {expected}: {ids:?}");
    }
}

#[test]
fn pairing_has_a_mouse_entry_point() {
    // The regression this whole surface exists for: `sync` was bound to
    // `g s` and to nothing clickable, so P2P was invisible to anyone
    // who had not read the keymap.
    let (_d, shell, app) = fixture(InputMode::Doom);
    let peers = app
        .destinations(&shell)
        .into_iter()
        .find(|d| d.id == "peers")
        .expect("a peers destination");
    assert_eq!(peers.command, "sync");
    assert_eq!(peers.surface, ModalSurface::Sync);
    assert!(!peers.label.is_empty(), "it says what it is, not just ⇄");
}

#[test]
fn the_ids_are_unique() {
    let (_d, shell, app) = fixture(InputMode::Doom);
    let all = app.destinations(&shell);
    let unique: BTreeSet<&str> = all.iter().map(|d| d.id).collect();
    assert_eq!(unique.len(), all.len(), "an id is used twice");
}

#[test]
fn the_order_is_stable_across_calls() {
    // It repaints every frame; an order that shifts under the pointer
    // makes it unclickable.
    let (_d, shell, app) = fixture(InputMode::Doom);
    let ids = |a: &ModalApp| {
        a.destinations(&shell)
            .into_iter()
            .map(|d| d.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&app), ids(&app));
}

// === every button is real, in every mode ===

#[test]
fn every_destination_shows_the_keymaps_chord_in_every_mode() {
    // V1: an affordance that runs a command names the chord that runs
    // it. A rail button with no key would also be the one place in the
    // GUI that lies about being keyboard-reachable.
    for mode in MODES {
        let (_d, shell, app) = fixture(mode);
        for dest in app.destinations(&shell) {
            assert_eq!(
                dest.chord,
                app.chord_for(dest.command).map(ToOwned::to_owned),
                "{} must show {mode:?}'s chord for {}",
                dest.id,
                dest.command
            );
            assert!(
                dest.chord.is_some(),
                "{} has no chord in {mode:?} — bind {} there",
                dest.id,
                dest.command
            );
            assert!(!dest.icon.is_empty(), "{} has no icon", dest.id);
            assert!(!dest.label.is_empty(), "{} has no label", dest.id);
        }
    }
}

#[test]
fn clicking_a_destination_opens_the_surface_it_claims() {
    // The button's `surface` is what the rail highlights as active, so
    // a mapping that disagrees with the command paints the wrong button
    // as current — and a dead command paints a button that does
    // nothing.
    for mode in MODES {
        let (_d, mut shell, mut app) = fixture(mode);
        let dests = app.destinations(&shell);
        for dest in dests {
            app.run(&mut shell, dest.command);
            assert_eq!(
                app.surface(),
                dest.surface,
                "{} ran {} and landed on {:?}",
                dest.id,
                dest.command,
                app.surface()
            );
        }
    }
}

#[test]
fn the_open_surface_is_the_active_destination() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    let active = |a: &ModalApp, s: &Shell| {
        a.destinations(s)
            .into_iter()
            .filter(|d| d.active)
            .map(|d| d.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(active(&app, &shell), vec!["outline"], "Browse is home");
    app.run(&mut shell, "sync");
    assert_eq!(active(&app, &shell), vec!["peers"]);
    app.run(&mut shell, "sniffer");
    assert_eq!(active(&app, &shell), vec!["sniffer"]);
}

#[test]
fn the_outline_button_returns_from_any_surface() {
    // Esc walks back one surface; the rail's home button is the mouse's
    // way out of a pane it opened, whatever it is.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    for away in ["sync", "sniffer", "llm", "graph", "journal", "cron"] {
        app.run(&mut shell, away);
        assert_ne!(app.surface(), ModalSurface::Browse, "{away} opened");
        app.run(&mut shell, "browse");
        assert_eq!(
            app.surface(),
            ModalSurface::Browse,
            "the home button came back from {away}"
        );
    }
}

// === the badges report live state ===

#[test]
fn a_quiet_system_wears_no_badges() {
    let (_d, shell, app) = fixture(InputMode::Doom);
    let badged: Vec<&str> = app
        .destinations(&shell)
        .iter()
        .filter(|d| d.badge.is_some())
        .map(|d| d.id)
        .collect();
    assert!(badged.is_empty(), "nothing has happened yet: {badged:?}");
}

#[test]
fn captured_flows_badge_the_sniffer() {
    let (_d, shell, mut app) = fixture(InputMode::Doom);
    app.sniffer_mut().record(
        "api.example:443 TCP",
        &closure_sniffer::MockBackend::new(vec![]),
    );
    let badge = app
        .destinations(&shell)
        .into_iter()
        .find(|d| d.id == "sniffer")
        .expect("sniffer")
        .badge;
    assert_eq!(badge.as_deref(), Some("1"));
}

#[test]
fn unresolved_conflicts_badge_the_conflicts_button() {
    let (_d, shell, mut app) = fixture(InputMode::Doom);
    app.set_conflicts(vec![closure_crdt::FieldConflict {
        block: closure_core::BlockId::from_existing("01HQRAIL00000000000000001"),
        field: closure_crdt::ConflictField::Title,
        base: None,
        ours: "ours".to_owned(),
        theirs: "theirs".to_owned(),
    }]);
    let dest = app
        .destinations(&shell)
        .into_iter()
        .find(|d| d.id == "conflicts")
        .expect("conflicts");
    assert_eq!(dest.badge.as_deref(), Some("1"));
    assert!(dest.urgent, "unresolved work reads loudly");
}

#[test]
fn paired_peers_badge_the_rail() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    assert!(
        app.destinations(&shell)
            .into_iter()
            .find(|d| d.id == "peers")
            .expect("peers")
            .badge
            .is_none(),
        "no peers, no badge"
    );
    app.run(&mut shell, "sync");
    let ticket = {
        let other = closure_shell_core::SyncApp::new("desk", "127.0.0.1:7788".parse().expect("a"));
        other.ticket()
    };
    app.sync_mut()
        .add_peer(&ticket)
        .expect("a pasted ticket pairs");
    let badge = app
        .destinations(&shell)
        .into_iter()
        .find(|d| d.id == "peers")
        .expect("peers")
        .badge;
    assert_eq!(badge.as_deref(), Some("1"), "one known peer");
}
