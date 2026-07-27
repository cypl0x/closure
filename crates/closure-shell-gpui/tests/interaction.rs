//! The reference shell's input and viewport seams.
//!
//! Everything here is a pure conversion between what the window
//! *receives* (a gpui keystroke, a clipboard string, a pane's measured
//! height, a pending chord) and what the shell-agnostic core *takes*
//! (a key name, typed characters, a line count, a filtered keymap).
//! The window itself is display-bound; these are the parts that can be
//! pinned without a GPU, and every one of them was a bug first.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{
    ModalSurface, accepts_paste, body_viewport_lines, editor_key, outline_follows_selection,
    paste_chars, side_reveals_selection, which_key_filter,
};

// === Shifted keys ===
//
// gpui reports letter keysyms lowercased (`shift-a` arrives as key
// "a", modifiers.shift, key_char "A"), while the body editor's vim
// vocabulary distinguishes `a` (append) from `A` (append at line end).
// Passed through raw, every uppercase command in the editor was dead.

#[test]
fn shift_uppercases_a_letter_key() {
    assert_eq!(editor_key("a", true, Some("A")), "A");
    assert_eq!(editor_key("g", true, Some("G")), "G");
    assert_eq!(
        editor_key("a", false, Some("a")),
        "a",
        "no shift, no change"
    );
}

#[test]
fn caps_lock_counts_as_shifted() {
    // Caps Lock produces the uppercase char with no shift modifier;
    // the char is what the user typed, so it wins.
    assert_eq!(editor_key("o", false, Some("O")), "O");
}

#[test]
fn named_and_symbol_keys_are_untouched() {
    // gpui already names shifted punctuation by its symbol, and the
    // named keys must survive verbatim — the core matches on them.
    assert_eq!(editor_key("escape", false, None), "escape");
    assert_eq!(editor_key("enter", true, None), "enter");
    assert_eq!(editor_key("$", true, Some("$")), "$");
    assert_eq!(editor_key(">", true, Some(">")), ">");
    assert_eq!(editor_key("pageup", true, None), "pageup");
    // A shifted named key must not be uppercased into nonsense.
    assert_eq!(editor_key("tab", true, None), "tab");
}

#[test]
fn a_missing_key_char_still_honours_shift() {
    // X11 can deliver a shifted letter with no printable char (e.g.
    // when a compose sequence swallows it); shift alone is enough.
    assert_eq!(editor_key("d", true, None), "D");
}

// === Clipboard paste ===
//
// The window is the only place that can reach the system clipboard,
// and the core only takes typed characters — so a paste is a sequence
// of keystrokes, and only the surfaces that *are* text fields may
// receive one. Pasting into Browse would run the characters as chords.

#[test]
fn typing_surfaces_take_a_paste() {
    for surface in [
        ModalSurface::Search,
        ModalSurface::Capture,
        ModalSurface::Rename,
        ModalSurface::AddSibling,
        ModalSurface::TagsEdit,
        ModalSurface::PropertyEdit,
        ModalSurface::Palette,
        ModalSurface::BodySearch,
        ModalSurface::Ex,
        ModalSurface::Sync,
        ModalSurface::Llm,
    ] {
        assert!(accepts_paste(surface, false), "{surface:?} is a text field");
    }
}

#[test]
fn browse_and_the_read_only_lists_refuse_a_paste() {
    for surface in [
        ModalSurface::Browse,
        ModalSurface::Agenda,
        ModalSurface::Blocks,
        ModalSurface::Backlinks,
        ModalSurface::Headlines,
        ModalSurface::DbView,
        ModalSurface::UndoHistory,
        ModalSurface::Sniffer,
        ModalSurface::Conflicts,
        ModalSurface::Graph,
        ModalSurface::Journal,
        ModalSurface::Cron,
    ] {
        assert!(
            !accepts_paste(surface, false),
            "{surface:?} would run the text as chords"
        );
    }
}

#[test]
fn the_body_editor_takes_a_paste_only_in_insert() {
    assert!(accepts_paste(ModalSurface::EditBody, true));
    assert!(accepts_paste(ModalSurface::EditBlock, true));
    assert!(
        !accepts_paste(ModalSurface::EditBody, false),
        "NORMAL mode would read the text as vim commands"
    );
}

#[test]
fn a_pasted_line_becomes_the_characters_to_type() {
    assert_eq!(
        paste_chars("id:01H", true),
        vec!['i', 'd', ':', '0', '1', 'H']
    );
}

#[test]
fn newlines_survive_only_where_they_can_be_typed() {
    assert_eq!(paste_chars("a\nb", true), vec!['a', '\n', 'b'], "editor");
    assert_eq!(
        paste_chars("a\nb", false),
        vec!['a', ' ', 'b'],
        "a one-line field keeps the words, loses the break"
    );
}

#[test]
fn crlf_and_control_bytes_are_dropped() {
    assert_eq!(paste_chars("a\r\nb", true), vec!['a', '\n', 'b']);
    assert_eq!(paste_chars("a\u{7}\u{1b}b", true), vec!['a', 'b']);
}

#[test]
fn tabs_paste_as_spaces() {
    // A literal tab in INSERT triggers org-tempo/table expansion, so a
    // pasted indent must not arrive as one.
    assert_eq!(paste_chars("\tx", true), vec![' ', ' ', 'x']);
}

// === Which pane a cursor move belongs to ===
//
// Nine surfaces reuse `selected` as their *own* list cursor while the
// outline stays on screen. Revealing that index in the outline scrolled
// the wrong pane — the left column jumped while the cursor the user was
// moving stayed out of sight.

#[test]
fn the_outline_follows_the_selection_on_the_browse_surfaces() {
    for surface in [
        ModalSurface::Browse,
        ModalSurface::Search,
        ModalSurface::Capture,
        ModalSurface::Rename,
        ModalSurface::AddSibling,
        ModalSurface::TagsEdit,
        ModalSurface::PropertyEdit,
        ModalSurface::Ex,
        ModalSurface::EditBody,
    ] {
        assert!(outline_follows_selection(surface), "{surface:?}");
    }
}

#[test]
fn a_side_list_cursor_does_not_scroll_the_outline() {
    for surface in [
        ModalSurface::Agenda,
        ModalSurface::Blocks,
        ModalSurface::Backlinks,
        ModalSurface::Headlines,
        ModalSurface::DbView,
        ModalSurface::BodySearch,
        ModalSurface::Graph,
        ModalSurface::Journal,
        ModalSurface::Cron,
    ] {
        assert!(
            !outline_follows_selection(surface),
            "{surface:?} owns `selected` itself"
        );
    }
}

#[test]
fn the_flat_side_lists_reveal_their_own_cursor() {
    // These paint one child per row, so the pane's scroll handle can
    // address a row by index.
    for surface in [
        ModalSurface::Headlines,
        ModalSurface::BodySearch,
        ModalSurface::Backlinks,
        ModalSurface::Journal,
        ModalSurface::Cron,
        ModalSurface::UndoHistory,
    ] {
        assert!(side_reveals_selection(surface), "{surface:?}");
    }
    // The panes that wrap their rows in sections cannot: a child index
    // is not a row index there.
    assert!(!side_reveals_selection(ModalSurface::Graph));
    assert!(!side_reveals_selection(ModalSurface::Browse));
}

// === The editor viewport ===
//
// It was a constant 40 lines. In a short window the cursor walked off
// the bottom with nothing scrolling after it; in a tall one the pane
// painted 40 lines and left the rest blank.

#[test]
fn the_viewport_is_the_pane_height_in_lines() {
    // 18px lines, 46px of header/padding: 400px of pane holds 19.
    assert_eq!(body_viewport_lines(400.0, 18.0, 46.0), 19);
    assert_eq!(body_viewport_lines(1000.0, 18.0, 46.0), 53);
}

#[test]
fn a_tiny_or_unmeasured_pane_still_paints_something() {
    // Before the first layout the height is zero; a viewport of zero
    // lines would paint an empty editor forever.
    assert!(body_viewport_lines(0.0, 18.0, 46.0) >= 4);
    assert!(body_viewport_lines(20.0, 18.0, 46.0) >= 4);
    assert!(body_viewport_lines(f32::NAN, 18.0, 46.0) >= 4);
    assert!(body_viewport_lines(400.0, 0.0, 46.0) >= 4, "no line height");
}

// === which-key while a chord is in flight ===
//
// The panel opens on a pending chord and showed the *entire* keymap,
// which is the one moment it should be showing only what can follow.

fn groups() -> Vec<(String, Vec<(String, String)>)> {
    vec![
        (
            "Navigate".to_owned(),
            vec![
                ("SPC f f".to_owned(), "find-file".to_owned()),
                ("j".to_owned(), "next-file".to_owned()),
            ],
        ),
        (
            "Capture".to_owned(),
            vec![("C-c c".to_owned(), "capture-start".to_owned())],
        ),
    ]
}

#[test]
fn no_pending_chord_shows_the_whole_keymap() {
    assert_eq!(which_key_filter(groups(), ""), groups());
}

#[test]
fn a_pending_chord_keeps_only_its_continuations() {
    let filtered = which_key_filter(groups(), "SPC");
    assert_eq!(filtered.len(), 1, "the Capture group has nothing under SPC");
    assert_eq!(filtered[0].0, "Navigate");
    assert_eq!(
        filtered[0].1,
        vec![("SPC f f".to_owned(), "find-file".to_owned())]
    );
}

#[test]
fn the_prefix_matches_whole_strokes_only() {
    // "SPC f" must not match a hypothetical "SPC fx" stroke, and "S"
    // must not match "SPC" — chords are space-separated strokes.
    let g = vec![(
        "G".to_owned(),
        vec![
            ("SPC f f".to_owned(), "a".to_owned()),
            ("SPC fx y".to_owned(), "b".to_owned()),
        ],
    )];
    let filtered = which_key_filter(g, "SPC f");
    assert_eq!(filtered[0].1.len(), 1);
    assert_eq!(filtered[0].1[0].1, "a");
}

#[test]
fn a_pending_chord_with_no_continuation_yields_nothing() {
    assert!(which_key_filter(groups(), "z").is_empty());
}
