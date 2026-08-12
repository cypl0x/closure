//! Notion's plus: a way to add a block with the mouse, where you are
//! pointing.
//!
//! Notion mode is meant to be the one a newcomer meets, and its whole
//! premise is that you never have to know a chord. Adding a headline
//! needed one — `add-sibling`, or the palette, or a slash command
//! inside a block that does not exist yet — which is the gap the mode
//! exists to close.
//!
//! In Notion the plus sits beside the block you are hovering and adds
//! one after it. Here a block you can point at is an outline row, so
//! that is where it goes. It runs the registered command like every
//! other affordance (I8): the mouse is a way of naming a command, not
//! a second way of editing.
//!
//! Only in Notion mode. A modal mode showing a mouse affordance on
//! every row is noise for someone who came for the keyboard.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{mode_window, visual_window};

const VAULT: &str = "\
* One
:PROPERTIES:
:ID: 01BLOCKPLUS000000000001
:END:
body
* Two
:PROPERTIES:
:ID: 01BLOCKPLUS000000000002
:END:
";

#[gpui::test]
fn notion_mode_offers_a_plus_on_a_row(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = mode_window(cx, VAULT, closure_config::InputMode::Notion);
    assert!(
        vcx.debug_bounds("row-plus-0").is_some(),
        "no way to add a block with the mouse"
    );
}

#[gpui::test]
fn the_keyboard_modes_do_not(cx: &mut gpui::TestAppContext) {
    // `visual_window` opens in Vim.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("row-plus-0").is_none(),
        "a mouse affordance on every row of a modal mode is noise"
    );
}

#[gpui::test]
fn clicking_it_starts_a_new_headline_after_that_row(cx: &mut gpui::TestAppContext) {
    // Notion's plus makes the block immediately and puts the cursor in
    // it. closure's `add-sibling` asks for a title first, and the plus
    // runs that command rather than inventing a second way to add a
    // headline — the mouse is a way of *naming* a command (I8), and a
    // second path would be a second set of rules about ids, undo and
    // where the new row lands.
    let (_dir, _view, vcx) = mode_window(cx, VAULT, closure_config::InputMode::Notion);
    let at = vcx.debug_bounds("row-plus-0").expect("the plus");
    vcx.simulate_click(at.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("prompt-row").is_some(),
        "the click named no command"
    );
}
