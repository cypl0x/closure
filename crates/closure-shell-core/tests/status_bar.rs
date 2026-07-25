//! The status bar's subsystem indicators.
//!
//! closure grew a lot of subsystems — a packet sniffer, an LLM gate,
//! CRDT sync, scheduled jobs — and none of them announced themselves.
//! You could not tell from the window whether the sniffer had captured
//! anything, whether the LLM could see your screen, or whether a merge
//! had left conflicts sitting unresolved. VS Code puts exactly this
//! kind of thing bottom-right, and it works because each item is one
//! glance and one click.
//!
//! The items are derived here rather than assembled in a render
//! function, so every shell shows the same set with the same chords,
//! and so the *state* each one reports can be asserted without a
//! window.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{IndicatorLevel, ModalApp, Shell};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO One\n:PROPERTIES:\n:ID: 01HQSTATUS0000000000001\n:END:\n\
         SCHEDULED: <2026-07-25 Sat>\n\
         #+BEGIN_SRC sh\necho hi\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_bar_lists_every_subsystem() {
    let (_d, shell, app) = fixture();
    let ids: Vec<&str> = app.indicators(&shell).iter().map(|i| i.id).collect();
    for expected in ["vault", "sniffer", "llm", "sync", "blocks"] {
        assert!(ids.contains(&expected), "missing {expected}: {ids:?}");
    }
}

#[test]
fn every_indicator_is_clickable_and_shows_its_chord() {
    // Same rule as the rest of the UI: an affordance that runs a
    // command names the chord that runs it, or it does not claim to
    // have one.
    let (_d, shell, app) = fixture();
    for item in app.indicators(&shell) {
        assert!(!item.label.is_empty(), "{} has no label", item.id);
        assert!(!item.tooltip.is_empty(), "{} has no tooltip", item.id);
        if let Some(command) = item.command {
            assert_eq!(
                app.chord_for(command),
                item.chord,
                "{} must show the keymap's chord for {command}",
                item.id
            );
        } else {
            assert!(
                item.chord.is_none(),
                "{} claims a chord with no command",
                item.id
            );
        }
    }
}

#[test]
fn a_quiet_system_reads_as_idle() {
    let (_d, shell, app) = fixture();
    let by = |id: &str| {
        app.indicators(&shell)
            .into_iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id} indicator"))
    };
    assert_eq!(by("sniffer").level, IndicatorLevel::Idle, "no flows yet");
    assert_eq!(by("sync").level, IndicatorLevel::Idle, "no conflicts");
    assert_eq!(by("llm").level, IndicatorLevel::Idle, "render access off");
}

#[test]
fn granting_render_access_makes_the_llm_indicator_loud() {
    // This one is a permission the user can flip at runtime, and it
    // lets a model read the screen — it should be visible that it is
    // on, not buried in a config file.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "toggle-llm-render");
    let llm = app
        .indicators(&shell)
        .into_iter()
        .find(|i| i.id == "llm")
        .expect("llm");
    assert_eq!(llm.level, IndicatorLevel::Active);
    assert!(
        llm.tooltip.contains("render"),
        "say what is granted: {}",
        llm.tooltip
    );
}

#[test]
fn pending_conflicts_make_the_sync_indicator_warn() {
    let (_d, shell, mut app) = fixture();
    app.set_conflicts(vec![closure_crdt::FieldConflict {
        block: closure_core::BlockId::from_existing("01HQSTATUS0000000000001"),
        field: closure_crdt::ConflictField::Title,
        base: None,
        ours: "ours".to_owned(),
        theirs: "theirs".to_owned(),
    }]);
    let sync = app
        .indicators(&shell)
        .into_iter()
        .find(|i| i.id == "sync")
        .expect("sync");
    assert_eq!(
        sync.level,
        IndicatorLevel::Warn,
        "unresolved work is a warning"
    );
    assert!(
        sync.label.contains('1'),
        "count in the label: {}",
        sync.label
    );
}

#[test]
fn the_vault_indicator_counts_what_is_loaded() {
    let (_d, shell, app) = fixture();
    let vault = app
        .indicators(&shell)
        .into_iter()
        .find(|i| i.id == "vault")
        .expect("vault");
    assert!(vault.label.contains('1'), "1 headline: {}", vault.label);
}

#[test]
fn the_blocks_indicator_counts_source_blocks() {
    let (_d, shell, app) = fixture();
    let blocks = app
        .indicators(&shell)
        .into_iter()
        .find(|i| i.id == "blocks")
        .expect("blocks");
    assert!(blocks.label.contains('1'), "1 block: {}", blocks.label);
}

#[test]
fn the_bar_is_stable_across_calls() {
    // It repaints every frame; an order that shifts under the pointer
    // makes it unclickable.
    let (_d, shell, app) = fixture();
    let ids = |a: &ModalApp| {
        a.indicators(&shell)
            .into_iter()
            .map(|i| i.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&app), ids(&app));
}
