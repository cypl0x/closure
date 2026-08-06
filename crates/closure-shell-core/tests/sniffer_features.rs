//! The rest of "Features include:" — debug, and the network graph.
//!
//! Both read the same capture log the pane reads, because a second
//! source for the same flows is how two screens start disagreeing
//! about what your machine did.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::SnifferApp;
use closure_store::Vault;

const LOG: &str = "\
* <2026-08-06T09:00:00Z> host=telemetry.example.com:443 proto=tcp
* <2026-08-06T09:00:01Z> host=api.github.com:443 proto=tcp
* <2026-08-06T09:00:02Z> host=telemetry.example.com:443 proto=tcp
* <2026-08-06T09:00:03Z> host=telemetry.example.com:80 proto=tcp
";

fn loaded() -> (tempfile::TempDir, SnifferApp) {
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
    (dir, app)
}

#[test]
fn debug_shows_the_entry_the_flow_came_from() {
    // "debug": when a verdict surprises you, the question is what was
    // actually recorded — not what the pane made of it.
    let (_d, app) = loaded();
    let lines = app.debug(0).expect("a flow");
    assert!(
        lines.iter().any(|l| l.contains("network.org")),
        "the flow does not say where it came from: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("telemetry.example.com:443 tcp")),
        "the candidate the rules were matched against is not shown: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("telemetry.*")),
        "the pattern that decided it is not shown: {lines:?}"
    );
}

#[test]
fn debug_on_an_undecided_flow_says_which_rules_were_tried() {
    let (_d, app) = loaded();
    // api.github.com is the second flow and nothing matches it.
    let lines = app.debug(1).expect("a flow");
    assert!(
        lines.iter().any(|l| l.contains("no rule matched")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("telemetry.*")),
        "the rules that were tried are not named: {lines:?}"
    );
}

#[test]
fn the_graph_is_one_row_per_host_with_how_often_it_was_seen() {
    // "network graph": the log records a host per flow and nothing
    // else, so what there is to draw is who you talk to and how much —
    // a distribution, not a topology. Saying so beats drawing edges
    // that were never measured.
    let (_d, app) = loaded();
    let graph = app.graph();
    assert_eq!(graph.len(), 2, "{graph:?}");
    // Busiest first.
    assert_eq!(graph[0].host, "telemetry.example.com");
    assert_eq!(graph[0].flows, 3);
    assert_eq!(graph[0].blocked, 3);
    assert_eq!(graph[1].host, "api.github.com");
    assert_eq!(graph[1].blocked, 0);
}

#[test]
fn the_graph_of_nothing_is_empty_rather_than_a_row_of_zeroes() {
    let app = SnifferApp::new();
    assert!(app.graph().is_empty());
}
