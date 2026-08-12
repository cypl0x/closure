//! A bad `config.org` says so, in the window.
//!
//! `load_reporting` returns the complaint, `run` binds it, and the
//! comment above the call reads "Every reader of this file used to drop
//! the error on the floor... It reaches the status line now."
//!
//! It did not. A vault whose config says `outline_width = wide` opened
//! with no message anywhere — checked at three seconds and at six, whole
//! window, not a crop. The setting was ignored and the reader told
//! nothing, which is the state that comment was written to end.
//!
//! It survived because no test could see it: every window test builds
//! `GpuiView::new` directly, so the entire config path ran only in
//! production. That is the same shape as the org constructs that were
//! parsed, tested and unreachable — a thing that works everywhere
//! except where somebody would notice.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01CFGCOMPLAIN00000001
:END:
";

/// The complaint `Config::load_reporting` produces for a bad value —
/// built here the way `run` gets it, so the test exercises the string a
/// user would actually be shown rather than one invented for the test.
fn complaint_for(config_org: &str) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(closure_config::CONFIG_FILE);
    std::fs::write(&path, config_org).unwrap();
    closure_config::Config::load_reporting(&path).1
}

#[test]
fn a_bad_value_produces_a_complaint_at_all() {
    // Before asking whether the window shows it: is there one?
    let said = complaint_for("#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n");
    let said = said.expect("a bad config value produced no complaint");
    assert!(said.contains("outline_width"), "{said}");
}

#[gpui::test]
fn the_window_shows_it(cx: &mut gpui::TestAppContext) {
    let said = complaint_for("#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n")
        .expect("a complaint");
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, _cx| {
        v.apply_config(
            closure_shell_core::ViewMode::Clickable,
            false,
            &[],
            "127.0.0.1:0".parse().unwrap(),
            None,
            Some(said.clone()),
        );
    });
    vcx.run_until_parked();
    let shown = view.update(vcx, |v, _cx| v.status().to_owned());
    assert!(
        shown.contains("outline_width"),
        "the window is not showing the config complaint: {shown:?}"
    );
}

#[gpui::test]
fn a_good_config_says_nothing(cx: &mut gpui::TestAppContext) {
    // The control: a startup message that appears when nothing is wrong
    // is worse than none, because it trains the reader to ignore the line.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, _cx| {
        v.apply_config(
            closure_shell_core::ViewMode::Clickable,
            false,
            &[],
            "127.0.0.1:0".parse().unwrap(),
            None,
            None,
        );
    });
    vcx.run_until_parked();
    let shown = view.update(vcx, |v, _cx| v.status().to_owned());
    assert!(
        shown.is_empty(),
        "said something about a fine config: {shown:?}"
    );
}

#[test]
fn a_config_written_the_way_a_person_writes_one_still_complains() {
    // The shape the bug actually showed in. My first test put the
    // block at the top of the file; a real `config.org` has a headline
    // over it, because it is an org document somebody also reads. If
    // the complaint only survives the bare form, the test proves
    // nothing about the file anyone has.
    let said =
        complaint_for("* Settings\n#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n");
    let said = said.expect("a config under a headline produced no complaint");
    assert!(said.contains("outline_width"), "{said}");
}
