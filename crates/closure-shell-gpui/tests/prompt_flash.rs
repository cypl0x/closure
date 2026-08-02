//! "flash/animate the prompt when activated in order to retrieve the
//! attention. It is quite small and can be 'übersehen'."
//!
//! A prompt is one line in a strip at the top of a window whose middle
//! is a note you are reading. Opening one changes almost nothing on
//! screen, so the gesture succeeds and you keep typing into what you
//! thought was still the outline.
//!
//! So it flashes: the row's border comes up in the accent colour and
//! fades back over a few hundred milliseconds. gpui keys an animation
//! by element id and replays it when the id changes, so what is tested
//! here is the *generation* — the number that has to move exactly when
//! a prompt opens and stay put when it does not.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::ModalSurface;
use closure_shell_gpui::prompt_flash;

#[test]
fn opening_a_prompt_moves_the_generation() {
    let (last, flashes) = prompt_flash(Some(ModalSurface::Browse), ModalSurface::Capture, 7);
    assert_eq!(flashes, 8, "the flash replays");
    assert_eq!(last, Some(ModalSurface::Capture));
}

#[test]
fn staying_in_the_same_prompt_does_not() {
    // Every keystroke into a capture would otherwise restart the
    // animation, which is a prompt that never stops blinking.
    let (last, flashes) = prompt_flash(Some(ModalSurface::Capture), ModalSurface::Capture, 8);
    assert_eq!(flashes, 8);
    assert_eq!(last, Some(ModalSurface::Capture));
}

#[test]
fn moving_between_two_prompts_flashes_again() {
    // Escaping a capture into a rename is a new prompt and a new
    // question; it has the same claim on your attention as the first.
    let (_, flashes) = prompt_flash(Some(ModalSurface::Capture), ModalSurface::Rename, 8);
    assert_eq!(flashes, 9);
}

#[test]
fn a_surface_with_no_field_forgets_the_last_one() {
    // …so that leaving a prompt and coming back to the same one
    // flashes, rather than being mistaken for never having left.
    let (last, flashes) = prompt_flash(Some(ModalSurface::Capture), ModalSurface::Browse, 9);
    assert_eq!(flashes, 9, "leaving is not an opening");
    assert_eq!(last, None);

    let (_, flashes) = prompt_flash(last, ModalSurface::Capture, flashes);
    assert_eq!(flashes, 10, "and going back in flashes");
}

#[test]
fn the_editor_is_not_a_prompt() {
    // A buffer is where you were already typing; flashing it would be
    // an interruption rather than a hint.
    let (last, flashes) = prompt_flash(None, ModalSurface::EditBody, 3);
    assert_eq!(flashes, 3);
    assert_eq!(last, None);
}

#[test]
fn every_picker_flashes_too() {
    // They are prompts with a list under them, and the same sentence
    // applies: the panel appears over work you were looking at.
    for surface in [
        ModalSurface::Palette,
        ModalSurface::Search,
        ModalSurface::Buffers,
        ModalSurface::Headlines,
        ModalSurface::Ex,
        ModalSurface::Llm,
    ] {
        let (_, flashes) = prompt_flash(Some(ModalSurface::Browse), surface, 0);
        assert_eq!(flashes, 1, "{surface:?} did not flash");
    }
}

#[test]
fn the_generation_wraps_rather_than_overflowing() {
    let (_, flashes) = prompt_flash(Some(ModalSurface::Browse), ModalSurface::Capture, u32::MAX);
    assert_eq!(flashes, 0);
}
