//! The capture log had no writer and no reader.
//!
//! `log_capture_to_org` was called once, in its own test, and by
//! nothing else — while the sniffer pane's empty state told you to go
//! and run `closure sniff`, which did not write it either. Both ends of
//! the same wire were missing, so a flow you sniffed left no trace and
//! a pane you opened had nothing to show.
//!
//! Now `closure sniff` appends to the `network.org` of the vault whose
//! config it was pointed at, which is the file the pane reads.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn vault(blocklist: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!("#+begin_src closure-config\nsniffer_blocklist = {blocklist}\n#+end_src\n"),
    )
    .unwrap();
    dir
}

fn sniff(dir: &std::path::Path, candidate: &str) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_closure"))
        .arg("sniff")
        .arg("--config")
        .arg(dir.join("config.org"))
        .arg(candidate)
        .output()
        .expect("run");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn sniffing_a_candidate_writes_it_to_the_vaults_log() {
    let dir = vault("telemetry.*");
    sniff(dir.path(), "telemetry.example.com:443");
    let log = std::fs::read_to_string(dir.path().join("network.org"))
        .expect("the sniff left no trace in the vault");
    assert!(log.contains("host=telemetry.example.com:443"), "{log}");
    assert!(log.starts_with("* <"), "not an org headline:\n{log}");
}

#[test]
fn two_sniffs_are_two_headlines() {
    let dir = vault("telemetry.*");
    sniff(dir.path(), "telemetry.example.com:443");
    sniff(dir.path(), "api.github.com:443");
    let log = std::fs::read_to_string(dir.path().join("network.org")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("* <")).count(),
        2,
        "{log}"
    );
}

#[test]
fn what_was_written_is_what_the_pane_reads() {
    // The loop closed: sniff writes, the pane loads, the flow is there.
    let dir = vault("telemetry.*");
    sniff(dir.path(), "telemetry.example.com:443");
    let vault = closure_store::Vault::open(dir.path()).unwrap();
    let mut app = closure_shell_core::SnifferApp::new();
    assert_eq!(app.load(&vault), 1);
    // `host:port protocol`, the shape a live capture is handed too.
    assert_eq!(app.events()[0].candidate, "telemetry.example.com:443 tcp");
}

#[test]
fn no_config_means_no_vault_to_write_to_and_that_is_not_an_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_closure"))
        .args(["sniff", "example.com:443"])
        .output()
        .expect("run");
    assert!(out.status.success(), "{out:?}");
}
