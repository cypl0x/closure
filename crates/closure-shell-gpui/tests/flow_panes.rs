//! The sniffer's three sub-panes paint what they promise.
//!
//! `flow-detail`, `flow-graph` and `flow-debug` are three of the
//! remaining affordances no test looked at. All three are `Option`s —
//! each returns `None` when there is nothing to show — so each has two
//! states worth asserting, and asserting only the empty one is how a
//! pane that never paints looks healthy.
//!
//! The flows come from a real `network.org` in the vault, written in
//! the shape `closure_sniffer::log_capture_to_org` writes, so the test
//! exercises the file format rather than a constructor no user reaches.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01FLOWPANES000000001
:END:
";

/// Three captured flows, in the shape the sniffer's own logger writes.
const FLOWS: &str = "\
* 2026-08-12T09:00:00Z host=api.example.invalid proto=tcp
* 2026-08-12T09:00:01Z host=api.example.invalid proto=tcp
* 2026-08-12T09:00:02Z host=other.example.invalid proto=udp
";

fn with_flows(
    cx: &mut gpui::TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::Entity<closure_shell_gpui::GpuiView>,
    &mut gpui::VisualTestContext,
) {
    let (dir, view, vcx) = visual_window(cx, VAULT);
    std::fs::write(dir.path().join("network.org"), FLOWS).unwrap();
    view.update(vcx, |v, cx| {
        // The vault was read when the window opened, so it has not seen
        // network.org yet — a real session gets there by the file
        // watcher, which a test does not run.
        v.run_command("reload-shell", cx);
        v.run_command("reload-flows", cx);
        v.run_command("sniffer", cx);
    });
    vcx.run_until_parked();
    (dir, view, vcx)
}

#[gpui::test]
fn a_captured_flow_gets_a_detail(cx: &mut gpui::TestAppContext) {
    let (_d, _view, vcx) = with_flows(cx);
    assert!(
        vcx.debug_bounds("flow-detail").is_some(),
        "three captured flows and nothing said about the selected one"
    );
}

#[gpui::test]
fn the_graph_appears_once_there_is_something_to_graph(cx: &mut gpui::TestAppContext) {
    let (_d, _view, vcx) = with_flows(cx);
    assert!(
        vcx.debug_bounds("flow-graph").is_some(),
        "no graph beside three flows across two hosts"
    );
}

#[gpui::test]
fn an_empty_vault_graphs_nothing(cx: &mut gpui::TestAppContext) {
    // The control that makes the one above mean something: a graph
    // painted unconditionally would satisfy it while showing nothing.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("sniffer", cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("flow-graph").is_none());
}

#[gpui::test]
fn the_debug_view_is_off_until_it_is_asked_for(cx: &mut gpui::TestAppContext) {
    // "What was recorded, and what it was matched against" is a
    // developer's view. On by default it would be noise in the pane a
    // reader opens to see their traffic.
    let (_d, _view, vcx) = with_flows(cx);
    assert!(vcx.debug_bounds("flow-debug").is_none());
}

#[gpui::test]
fn debug_flow_shows_what_a_rule_was_matched_against(cx: &mut gpui::TestAppContext) {
    let (_d, view, vcx) = with_flows(cx);
    view.update(vcx, |v, cx| v.run_command("debug-flow", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("flow-debug").is_some(),
        "asked what the rule saw and was shown nothing"
    );
}
