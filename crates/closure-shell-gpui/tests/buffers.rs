//! Q1 in the window: the buffer list, the file picker, the tab strip.
//!
//! The core keeps the buffers ([`closure_shell_core`]'s `buffers.rs`);
//! this is the half that only the window can be wrong about — that the
//! two new surfaces paint at all, that a click on a row switches to the
//! buffer under it, and that the strip appears when a second buffer
//! does.
//!
//! Run with `cargo test -p closure-shell-gpui --features gpui-test`.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{ModalSurface, test_window};

const VAULT: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQGBUF00000000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQGBUF00000000000000002
:END:
beta body
";

#[gpui::test]
fn the_buffer_list_paints_what_the_session_has_open(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.run_command("edit-body", cx);
            view.run_command("browse", cx);
            view.press("j", false, false, cx);
            view.run_command("edit-body", cx);
            view.run_command("buffer-list", cx);
            assert_eq!(view.surface(), ModalSurface::Buffers);
            assert_eq!(view.buffer_row_count(), 2, "both notes are buffers");
        })
        .expect("live");
    // The pane itself has to paint, not merely exist.
    cx.run_until_parked();
}

#[gpui::test]
fn clicking_a_buffer_row_switches_to_it(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.run_command("edit-body", cx);
            view.run_command("browse", cx);
            view.press("j", false, false, cx);
            view.run_command("edit-body", cx);
            // Beta is current; row 1 is Alpha.
            view.buffer_click(1, cx);
            assert_eq!(view.surface(), ModalSurface::EditBody);
            assert_eq!(view.current_buffer_name(), Some("Alpha".to_owned()));
        })
        .expect("live");
    cx.run_until_parked();
}

#[gpui::test]
fn the_file_picker_paints_the_vault(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            view.run_command("recent-files", cx);
            assert_eq!(view.surface(), ModalSurface::Files);
            assert!(view.file_row_count() >= 1, "the one file is listed");
        })
        .expect("live");
    cx.run_until_parked();
}

#[gpui::test]
fn the_tab_strip_appears_with_the_second_buffer(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, VAULT);
    window
        .update(cx, |view, _w, cx| {
            assert!(!view.tab_strip_visible(), "no buffers, no strip");
            view.run_command("edit-body", cx);
            assert!(!view.tab_strip_visible(), "one buffer is furniture");
            view.run_command("browse", cx);
            view.press("j", false, false, cx);
            view.run_command("edit-body", cx);
            assert!(view.tab_strip_visible(), "two buffers earn a strip");
        })
        .expect("live");
    cx.run_until_parked();
}
