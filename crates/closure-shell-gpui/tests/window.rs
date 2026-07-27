//! The window itself, driven headlessly.
//!
//! Everything under `#[cfg(feature = "gpui")]` in this crate — the
//! view, its render, its key path, its input handler — was
//! compile-checked by CI and never once run: gpui needs a GPU, so
//! there was nowhere to run it. gpui's `test-support` ships a
//! `TestAppContext` over a stub platform, and that is enough. These
//! build a real `GpuiView` over a real vault and press keys at it.
//!
//! Run with `cargo test -p closure-shell-gpui --features gpui-test`.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{ModalSurface, byte_for_utf16, test_window, utf16_for_byte};

// === the view exists and reads the vault ===

#[gpui::test]
fn the_window_opens_over_a_vault(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Alpha\nbody\n** Beta\n");
    window
        .update(cx, |view, _w, _cx| {
            assert_eq!(view.row_count(), 2, "both headlines are in the outline");
            assert_eq!(view.surface(), ModalSurface::Browse);
        })
        .expect("the window is live");
}

#[gpui::test]
fn the_window_draws_without_a_gpu(cx: &mut gpui::TestAppContext) {
    // The render path itself: it had no test at all, so a panic in it
    // was a thing the user found by launching closure.
    let (_dir, window) = test_window(cx, "* Alpha\n*bold* and =code=\n** Beta\n");
    window.update(cx, |_v, _w, cx| cx.notify()).expect("notify");
    cx.run_until_parked();
}

#[gpui::test]
fn an_empty_vault_draws_its_empty_state(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "");
    window
        .update(cx, |view, _w, _cx| assert_eq!(view.row_count(), 0))
        .expect("live");
    cx.run_until_parked();
}

// === keys reach the core through the window's own translation ===

#[gpui::test]
fn typing_into_the_body_editor_reaches_the_buffer(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx);
            assert_eq!(view.surface(), ModalSurface::EditBody);
            // A modal mode opens the buffer in NORMAL; `i` types.
            view.press("i", false, false, cx);
            for c in "hello".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            assert_eq!(view.body(), "hello");
        })
        .expect("live");
}

#[gpui::test]
fn an_uppercase_chord_survives_the_window_seam(cx: &mut gpui::TestAppContext) {
    // gpui lowercases letter keysyms, so `A` arrived as `a` and every
    // uppercase editor command was dead. This is that seam, in the
    // window rather than beside it.
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT
            for c in "one two".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            view.press("escape", false, false, cx);
            view.press("0", false, false, cx);
            view.press("d", false, false, cx);
            view.press("i", false, false, cx);
            view.press("w", false, false, cx);
            assert_eq!(view.body(), " two", "diw through the window");
            view.press("A", true, false, cx);
            view.press("!", false, false, cx);
            assert_eq!(view.body(), " two!", "A is append-at-end, not a");
        })
        .expect("live");
}

#[gpui::test]
fn a_ctrl_chord_reaches_the_editor(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT
            for c in "count 41".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            view.press("escape", false, false, cx);
            view.press("a", false, true, cx);
            assert_eq!(view.body(), "count 42");
        })
        .expect("live");
}

#[gpui::test]
fn escape_out_of_a_clean_editor_returns_to_browse(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx);
            view.press("escape", false, false, cx);
            view.press("escape", false, false, cx);
            assert_eq!(view.surface(), ModalSurface::Browse);
        })
        .expect("live");
}

// === the dirty guard, from the window's side ===

#[gpui::test]
fn a_window_closing_over_an_unsaved_edit_saves_it(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT
            for c in "rescued".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            assert!(view.save_pending_edit(), "there was something to save");
            assert!(view.vault_contains("rescued"), "and it reached the vault");
        })
        .expect("live");
}

// === UTF-16 ↔ byte, which is where input methods go wrong ===

#[test]
fn ascii_offsets_are_the_same_in_both_counts() {
    assert_eq!(utf16_for_byte("abc", 2), 2);
    assert_eq!(byte_for_utf16("abc", 2), 2);
}

#[test]
fn a_multibyte_char_is_one_utf16_unit() {
    // `ä` is 2 bytes, 1 UTF-16 unit.
    let text = "äb";
    assert_eq!(utf16_for_byte(text, 2), 1);
    assert_eq!(byte_for_utf16(text, 1), 2);
}

#[test]
fn an_astral_char_is_a_surrogate_pair() {
    // An emoji is 4 bytes and *two* UTF-16 units — the off-by-one that
    // puts a composed character in the wrong place.
    let text = "🦀x";
    assert_eq!(utf16_for_byte(text, 4), 2);
    assert_eq!(byte_for_utf16(text, 2), 4);
    assert_eq!(utf16_for_byte(text, 5), 3);
}

#[test]
fn an_offset_inside_a_surrogate_pair_rounds_down() {
    // Splitting one would panic on the next slice.
    assert_eq!(byte_for_utf16("🦀x", 1), 0);
}

#[test]
fn offsets_past_the_end_clamp() {
    assert_eq!(utf16_for_byte("ab", 99), 2);
    assert_eq!(byte_for_utf16("ab", 99), 2);
    assert_eq!(utf16_for_byte("", 0), 0);
    assert_eq!(byte_for_utf16("", 5), 0);
}

#[test]
fn the_two_are_inverse_over_mixed_text() {
    let text = "aä€🦀z";
    for (byte, _) in text
        .char_indices()
        .chain(std::iter::once((text.len(), 'x')))
    {
        assert_eq!(
            byte_for_utf16(text, utf16_for_byte(text, byte)),
            byte,
            "round trip at byte {byte}"
        );
    }
}

// === composed text: dead keys, compose sequences, any IME ===
//
// The window read `KeyDownEvent.key_char` and nothing else, so `´`+`e`
// produced nothing and a CJK input method could not type at all. Those
// arrive through `EntityInputHandler`, in UTF-16 units.

#[gpui::test]
fn committed_text_lands_in_the_body(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT takes the IME
            view.ime_commit(None, "é", w, cx);
            assert_eq!(view.body(), "é");
        })
        .expect("live");
}

#[gpui::test]
fn a_composition_replaces_its_own_preedit(cx: &mut gpui::TestAppContext) {
    // The provisional text is handed over repeatedly as the user
    // types; each hand-off must replace the last, not append to it.
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT takes the IME
            view.ime_mark(None, "n", w, cx);
            view.ime_mark(None, "ni", w, cx);
            view.ime_mark(None, "にほん", w, cx);
            assert_eq!(view.body(), "にほん", "one composition, not three");
            view.ime_commit(None, "日本", w, cx);
            assert_eq!(view.body(), "日本", "the commit replaced the preedit");
        })
        .expect("live");
}

#[gpui::test]
fn an_abandoned_composition_leaves_nothing_behind(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT
            for c in "ab".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            view.ime_mark(None, "ni", w, cx);
            assert_eq!(view.body(), "abni");
            view.ime_unmark(w, cx);
            assert_eq!(view.body(), "ab", "the provisional text went with it");
        })
        .expect("live");
}

#[gpui::test]
fn composed_text_is_refused_outside_insert(cx: &mut gpui::TestAppContext) {
    // In NORMAL the characters would be vim commands, and in Browse
    // they would be chords.
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, w, cx| {
            view.ime_commit(None, "é", w, cx);
            assert_eq!(view.body(), "", "Browse takes no composed text");
            view.press("i", false, false, cx);
            view.press("escape", false, false, cx);
            view.ime_commit(None, "é", w, cx);
            assert_eq!(view.body(), "", "NORMAL takes none either");
        })
        .expect("live");
}

#[gpui::test]
fn a_composition_is_one_undo_step(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Note\n");
    window
        .update(cx, |view, w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT takes the IME
            view.ime_commit(None, "日本", w, cx);
            view.press("escape", false, false, cx);
            view.press("u", false, false, cx);
            assert_eq!(view.body(), "", "undone in one step");
        })
        .expect("live");
}

// === the vault changing underneath the window ===

#[gpui::test]
fn an_external_edit_is_picked_up(cx: &mut gpui::TestAppContext) {
    // closure is local-first, so the files are the API: an Emacs on the
    // same directory, a `git pull` or an inbound sync round all write
    // org the window used to know nothing about.
    let (dir, window) = test_window(cx, "* Alpha\n");
    std::fs::write(dir.path().join("notes.org"), "* Alpha\n* Added\n").expect("external write");
    window
        .update(cx, |view, _w, cx| {
            view.poll_vault(cx);
            assert_eq!(view.row_count(), 2, "the new headline arrived");
            assert!(
                view.status().contains("changed on disk"),
                "{}",
                view.status()
            );
        })
        .expect("live");
}

#[gpui::test]
fn an_unchanged_vault_is_not_reloaded(cx: &mut gpui::TestAppContext) {
    let (_dir, window) = test_window(cx, "* Alpha\n");
    window
        .update(cx, |view, _w, cx| {
            let before = view.status().to_owned();
            view.poll_vault(cx);
            assert_eq!(view.status(), before, "nothing changed, nothing said");
        })
        .expect("live");
}

#[gpui::test]
fn an_open_edit_is_never_reloaded_out_from_under(cx: &mut gpui::TestAppContext) {
    // Reparsing under the editor would swap the text out from beneath
    // the cursor, and the buffer is the user's, not the file's.
    let (dir, window) = test_window(cx, "* Alpha\n");
    window
        .update(cx, |view, _w, cx| {
            view.press("i", false, false, cx); // open the body
            view.press("i", false, false, cx); // INSERT
            for c in "mine".chars() {
                view.press(&c.to_string(), false, false, cx);
            }
            std::fs::write(dir.path().join("notes.org"), "* Alpha\n* Added\n")
                .expect("external write");
            view.poll_vault(cx);
            assert_eq!(view.body(), "mine", "the buffer is untouched");
        })
        .expect("live");
}
