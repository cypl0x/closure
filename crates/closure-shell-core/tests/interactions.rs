//! G5b: interaction states as a hermetic state machine. Hover / focus /
//! active(pressed) / disabled are tracked per element index, with a
//! defined precedence, so every shell can paint a focus ring / hover
//! highlight / disabled dimming from one tested source — the pixels are
//! the embedder's, the *state* is here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{ElementState, Interactions};

#[test]
fn default_state_is_normal() {
    let it = Interactions::default();
    assert_eq!(it.state_of(0), ElementState::Normal);
}

#[test]
fn focus_and_blur_track_the_focused_element() {
    let mut it = Interactions::default();
    it.focus(3);
    assert_eq!(it.state_of(3), ElementState::Focused);
    assert_eq!(it.state_of(2), ElementState::Normal);
    it.blur();
    assert_eq!(it.state_of(3), ElementState::Normal);
}

#[test]
fn hover_tracks_the_pointer() {
    let mut it = Interactions::default();
    it.hover(Some(1));
    assert_eq!(it.state_of(1), ElementState::Hovered);
    it.hover(None);
    assert_eq!(it.state_of(1), ElementState::Normal);
}

#[test]
fn press_is_active_until_released() {
    let mut it = Interactions::default();
    it.press(2);
    assert_eq!(it.state_of(2), ElementState::Active);
    it.release();
    assert_eq!(it.state_of(2), ElementState::Normal);
}

#[test]
fn disabled_wins_over_every_other_state() {
    let mut it = Interactions::default();
    it.set_disabled(0, true);
    it.focus(0);
    it.hover(Some(0));
    it.press(0);
    assert_eq!(it.state_of(0), ElementState::Disabled, "disabled is absorbing");
    it.set_disabled(0, false);
    assert_eq!(it.state_of(0), ElementState::Active, "re-enabled, press still held");
}

#[test]
fn precedence_is_active_then_focused_then_hovered() {
    let mut it = Interactions::default();
    it.focus(0);
    it.hover(Some(0));
    assert_eq!(it.state_of(0), ElementState::Focused, "focus beats hover");
    it.press(0);
    assert_eq!(it.state_of(0), ElementState::Active, "press beats focus");
}

#[test]
fn focus_next_and_prev_wrap_within_the_count() {
    let mut it = Interactions::default();
    it.focus(0);
    it.focus_next(3);
    assert_eq!(it.state_of(1), ElementState::Focused);
    it.focus_prev(3);
    it.focus_prev(3);
    assert_eq!(it.state_of(2), ElementState::Focused, "wrapped past 0 to the end");
}
