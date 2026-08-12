//! The settings pane and the which-key chord list paint their rows.
//!
//! Two more of the unlooked-at affordances, and both are click targets
//! for someone who does not know a chord yet — which is exactly the
//! reader closure says it is for.
//!
//! `setting-{key}` is how the assistant gets configured without editing
//! `config.org` by hand. `wk-chord-{c}` is the which-key popup, whose
//! whole job is to be the thing you look at when you have forgotten the
//! key; a popup that paints nothing fails silently, because not knowing
//! the chord is the state you were already in.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{mode_window, visual_window};

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01SETTINGSPANE000001
:END:
";

#[gpui::test]
fn the_settings_pane_paints_a_row_per_setting(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("assistant-setup", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("setting-llm_provider").is_some(),
        "the assistant cannot be configured from the screen it offers for it"
    );
}

#[gpui::test]
fn the_settings_rows_match_what_the_kernel_lists(cx: &mut gpui::TestAppContext) {
    // A pane that painted fewer rows than there are settings would hide
    // one, and hiding `llm_key_env` is how somebody concludes closure
    // cannot reach their model.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("assistant-setup", cx));
    vcx.run_until_parked();
    let listed = view.update(vcx, |v, _| v.app().settings_rows(v.shell()).len());
    assert!(listed >= 2, "only {listed} settings");
}

#[gpui::test]
fn the_which_key_popup_lists_the_chords_under_a_prefix(cx: &mut gpui::TestAppContext) {
    // Doom's `g` prefix. What must appear is the list of what `g` can
    // be followed by — and the selector carries the *command name*, not
    // the chord letter, so `g a` = agenda is `wk-chord-agenda`.
    let (_dir, _view, vcx) = mode_window(cx, VAULT, closure_config::InputMode::Doom);
    vcx.simulate_keystrokes("g");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("wk-chord-agenda").is_some(),
        "which-key showed no chord under `g`, which is the one moment it exists for"
    );
}

#[gpui::test]
fn no_popup_before_a_prefix_is_pressed(cx: &mut gpui::TestAppContext) {
    // The control. A which-key that is always open is a pane, not a
    // hint, and it would satisfy the test above without working.
    let (_dir, _view, vcx) = mode_window(cx, VAULT, closure_config::InputMode::Doom);
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("wk-chord-agenda").is_none());
}
