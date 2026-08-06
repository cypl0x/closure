//! "build top notch UI for the network snitcher."
//!
//! The pane had nothing in it and no way to get anything. Nothing in
//! the app ever called `record`, so the empty state told you to go and
//! run `closure sniff` — a UI whose first instruction is to use a
//! different program — and `log_capture_to_org`, the org-native
//! capture log, was written once in a test and read by nobody.
//!
//! So the log is the source: a flow you captured is a headline in your
//! vault, and the pane reads it the way every other pane reads the
//! vault. No privileges, no daemon, and it survives a restart —
//! which the in-memory event list never did.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{Shell, SnifferApp};
use closure_store::Vault;

const LOG: &str = "\
* <2026-08-06T09:00:00Z> host=telemetry.example.com:443 proto=tcp
* <2026-08-06T09:00:01Z> host=api.github.com:443 proto=tcp
* <2026-08-06T09:00:02Z> host=telemetry.example.com:443 proto=tcp
";

fn shell(log: &str) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("network.org"), log).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

#[test]
fn the_pane_reads_the_flows_the_log_already_has() {
    let (_d, shell) = shell(LOG);
    let mut app = SnifferApp::new();
    let n = app.load(&shell.vault);
    assert_eq!(n, 3, "{:?}", app.events());
    // `host:port protocol` — the same shape `record` is handed live, so
    // a flow reads the same whether it came off the wire or out of the
    // log. (This test asked for the bare address until the protocol
    // turned out to be dropped; see the protocol test below.)
    assert_eq!(app.events()[0].candidate, "telemetry.example.com:443 tcp");
}

#[test]
fn a_vault_with_no_log_is_empty_and_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* A note\n").unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = SnifferApp::new();
    assert_eq!(app.load(&shell.vault), 0);
}

#[test]
fn loading_twice_does_not_double_the_list() {
    // The pane reloads when you open it, and opening it twice is not
    // six flows.
    let (_d, shell) = shell(LOG);
    let mut app = SnifferApp::new();
    app.load(&shell.vault);
    app.load(&shell.vault);
    assert_eq!(app.events().len(), 3);
}

#[test]
fn a_rule_you_added_decides_a_flow_that_came_from_the_log() {
    // The log records what happened; the verdict is whatever the rules
    // say *now*, so blocking a host re-decides the flows already in
    // front of you rather than only the next one.
    let (_d, shell) = shell(LOG);
    let mut app = SnifferApp::new();
    app.load(&shell.vault);
    app.select(0);
    app.block_selected();
    let decided = app
        .events()
        .iter()
        .filter(|e| e.candidate.starts_with("telemetry"))
        .filter(|e| e.action == Some(closure_sniffer::Action::Block))
        .count();
    assert_eq!(decided, 2, "{:?}", app.events());
}

#[test]
fn the_log_is_written_by_the_thing_that_captures() {
    // The other half of the same wire: a capture appends a headline,
    // so what the pane reads is what the sniffer saw.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("network.org");
    closure_sniffer::log_capture_to_org(&path, "example.com:443", "tcp", "2026-08-06T09:00:00Z")
        .unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    shell.vault.reload().ok();
    let mut app = SnifferApp::new();
    assert_eq!(app.load(&shell.vault), 1);
}

#[test]
fn the_vaults_own_blocklist_decides_the_flows_it_shows() {
    // Seen on :1: four flows read from the log, and `telemetry.*` in
    // the vault's own `sniffer_blocklist` while the pane said "nothing
    // matched it" beside telemetry.example.com. The pane was applying
    // only the rules you had added by hand this session.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("network.org"), LOG).unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+begin_src closure-config\nsniffer_blocklist = telemetry.*\n#+end_src\n",
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut app = SnifferApp::new();
    app.load(&vault);
    let blocked = app
        .events()
        .iter()
        .filter(|e| e.action == Some(closure_sniffer::Action::Block))
        .count();
    assert_eq!(blocked, 2, "{:?}", app.events());
    let detail = app.inspect(0).expect("a flow");
    assert!(
        detail.rule.is_some_and(|r| r.pattern == "telemetry.*"),
        "the pane does not name the rule that decided it"
    );
}

#[test]
fn the_protocol_the_log_recorded_is_not_thrown_away() {
    // The log says `proto=tcp`; the pane showed `protocol —`.
    let (_d, shell) = shell(LOG);
    let mut app = SnifferApp::new();
    app.load(&shell.vault);
    assert_eq!(app.inspect(0).expect("a flow").protocol, "tcp");
}
