//! Where the buffer sits under the cursor.
//!
//! The editor's viewport followed one rule: park the cursor on the last
//! visible line whenever it went off the bottom. That is right for a
//! `j`, and wrong for everything that *jumps* — a search hit, `G`, a
//! link followed into the middle of a file — because it lands the line
//! you asked for at the very edge of the pane with no context under it.
//!
//! Doom's own settings say what to do instead (`lisp/doom-emacs.el`):
//! `scroll-margin 0` — no forced context on an ordinary move — and
//! `scroll-conservatively 10` — scroll by the minimum for a move of up
//! to ten lines, recentre for anything further. `C-l` is the manual
//! override, and cycles centre → top → bottom the way
//! `recenter-top-bottom` does; `zz`/`zt`/`zb` are the same three from
//! evil's side of the keyboard.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// An open body editor over `lines` numbered lines, in `mode`.
fn editor(sh: &mut Shell, mode: InputMode, lines: usize) -> ModalApp {
    let text = (0..lines)
        .map(|i| format!("l{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = ModalApp::new(mode);
    app.on_key(sh, "i", false, false, Some('i')); // open the body
    app.on_key(sh, "i", false, false, Some('i')); // INSERT
    for c in text.chars() {
        if c == '\n' {
            app.on_key(sh, "enter", false, false, None);
        } else {
            app.on_key(sh, &c.to_string(), false, false, Some(c));
        }
    }
    app.on_key(sh, "escape", false, false, None); // NORMAL
    app
}

/// Put the cursor on `line` without leaving a scroll override behind.
fn goto(app: &mut ModalApp, line: usize) {
    app.body_goto_line(line);
}

const VIEW: usize = 10;

// === C-l, the manual override ===

#[test]
fn ctrl_l_centres_the_cursor_line() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 50);

    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(
        app.body_scroll_start(VIEW),
        45,
        "the cursor line sits in the middle of the viewport"
    );
}

#[test]
fn ctrl_l_cycles_centre_then_top_then_bottom() {
    // `recenter-top-bottom`: the second and third press are the whole
    // point of the binding — one key, three framings.
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 50);

    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(app.body_scroll_start(VIEW), 45, "centre");
    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(app.body_scroll_start(VIEW), 50, "top");
    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(app.body_scroll_start(VIEW), 41, "bottom");
    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(app.body_scroll_start(VIEW), 45, "and round again");
}

#[test]
fn moving_the_cursor_restarts_the_cycle() {
    // Two C-l presses either side of a motion are two *first* presses:
    // the cycle describes where the current line went, and the current
    // line changed.
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 50);
    app.on_key(&mut sh, "l", true, false, None);
    app.on_key(&mut sh, "k", false, false, Some('k')); // line 49
    app.on_key(&mut sh, "l", true, false, None);
    assert_eq!(app.body_scroll_start(VIEW), 44, "centre, not top");
}

#[test]
fn zz_zt_and_zb_frame_the_line_directly() {
    // evil's three, and unlike C-l each one says which framing it wants
    // rather than cycling.
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Vim, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 50);

    app.on_key(&mut sh, "z", false, false, Some('z'));
    app.on_key(&mut sh, "z", false, false, Some('z'));
    assert_eq!(app.body_scroll_start(VIEW), 45, "zz centres");

    app.on_key(&mut sh, "z", false, false, Some('z'));
    app.on_key(&mut sh, "t", false, false, Some('t'));
    assert_eq!(app.body_scroll_start(VIEW), 50, "zt puts it at the top");

    app.on_key(&mut sh, "z", false, false, Some('z'));
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert_eq!(app.body_scroll_start(VIEW), 41, "zb puts it at the bottom");
}

#[test]
fn a_pending_z_does_not_type_itself_into_the_buffer() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Vim, 20);
    app.set_body_viewport(VIEW);
    let before = app.body_buffer().to_owned();
    app.on_key(&mut sh, "z", false, false, Some('z'));
    app.on_key(&mut sh, "z", false, false, Some('z'));
    assert_eq!(app.body_buffer(), before, "z is a prefix, not text");
}

#[test]
fn z_in_insert_mode_is_just_a_letter() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Vim, 5);
    app.set_body_viewport(VIEW);
    app.on_key(&mut sh, "i", false, false, Some('i')); // INSERT
    app.on_key(&mut sh, "z", false, false, Some('z'));
    assert!(
        app.body_buffer().contains('z'),
        "typing must never lose a keystroke to a chord: {:?}",
        app.body_buffer()
    );
}

// === following the cursor (scroll-conservatively 10) ===

#[test]
fn a_short_move_off_the_edge_scrolls_by_the_minimum() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 20);
    app.on_key(&mut sh, "l", true, false, None); // centre: start 15
    assert_eq!(app.body_scroll_start(VIEW), 15);

    // Line 25 is the last visible one; 26 is one past it.
    app.body_goto_line(26);
    assert_eq!(
        app.body_scroll_follow(VIEW),
        17,
        "one line off the bottom scrolls one line, not a screenful"
    );
}

#[test]
fn a_far_jump_recentres_instead_of_parking_at_the_edge() {
    // The line you jumped to is the line you want to read *around* —
    // pinned to the bottom edge it has no context under it at all.
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 5);
    app.body_scroll_follow(VIEW);

    app.body_goto_line(60);
    assert_eq!(
        app.body_scroll_follow(VIEW),
        55,
        "more than ten lines away: recentre"
    );
}

#[test]
fn a_move_inside_the_viewport_does_not_scroll_at_all() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 100);
    app.set_body_viewport(VIEW);
    goto(&mut app, 50);
    app.on_key(&mut sh, "l", true, false, None); // start 45
    app.body_goto_line(47);
    assert_eq!(
        app.body_scroll_follow(VIEW),
        45,
        "the line was already on screen"
    );
}

#[test]
fn following_never_scrolls_past_the_end_of_the_buffer() {
    let (_d, mut sh) = shell();
    let mut app = editor(&mut sh, InputMode::Doom, 30);
    app.set_body_viewport(VIEW);
    app.body_goto_line(29);
    let start = app.body_scroll_follow(VIEW);
    assert!(
        start <= 20,
        "30 lines, 10 visible: last start is 20 ({start})"
    );
    assert!(
        (start..start + VIEW).contains(&29),
        "and the cursor is still on screen ({start})"
    );
}
