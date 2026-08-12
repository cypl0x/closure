//! The last five affordances no test looked at.
//!
//! `palette-overlay`, `prompt-completions`, `detail-meta`,
//! `header-vault` and `image-overlay`. Four of them are the things a
//! reader sees most often — the command palette, the completions under
//! a prompt, the metadata strip under a headline, the vault name in the
//! title bar — which is a fair illustration of how little "obviously it
//! works" is worth as a reason not to test something.
//!
//! Two are overlays that paint only when open, so both states are
//! asserted: an overlay painted unconditionally would satisfy the
//! open-case test while covering the window.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A headline with metadata
:PROPERTIES:
:ID: 01OVERLAYS0000000001
:END:
some body text
";

#[gpui::test]
fn the_vault_name_is_in_the_header(cx: &mut gpui::TestAppContext) {
    // Two closure windows are told apart by their vault, so this is
    // the one thing in the header that cannot be decoration.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(vcx.debug_bounds("header-vault").is_some());
}

#[gpui::test]
fn a_selected_headline_shows_its_metadata_strip(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("detail-meta").is_some(),
        "the file, level and word count are not shown for a selected headline"
    );
}

#[gpui::test]
fn the_palette_overlay_is_closed_until_opened(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("palette-overlay").is_none(),
        "the palette is covering the window before anyone asked for it"
    );
}

#[gpui::test]
fn the_palette_overlay_opens(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("palette", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("palette-overlay").is_some(),
        "M-x opened nothing"
    );
}

#[gpui::test]
fn a_prompt_offers_completions(cx: &mut gpui::TestAppContext) {
    // Three things have to be true at once, which is why this was the
    // one of the five that took a red test: a prompt must be open, a
    // prefix must be typed (an empty prompt completes nothing, rightly),
    // and the completion cycle must be *asked for* with `C-n` — it is
    // the body editor's gesture over a prompt, not something that fires
    // as you type. The candidates are TODO keywords and words already
    // in the vault, so `TO` has one and `#+` — my first guess — has
    // none.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("capture", cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("T O");
    vcx.simulate_keystrokes("ctrl-n");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("prompt-completions").is_some(),
        "a prompt with candidates offered none"
    );
}

#[gpui::test]
fn nothing_offers_completions_when_no_prompt_is_open(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(vcx.debug_bounds("prompt-completions").is_none());
}

#[gpui::test]
fn the_image_overlay_is_closed_until_an_image_is_opened(cx: &mut gpui::TestAppContext) {
    // The other half — an image actually being shown — needs a real
    // decodable file on disk and is `image_click.rs`'s business. What
    // is asserted here is that the overlay is not covering a window
    // that has no image in it, which is the state every session starts
    // in and the one a stuck overlay would break.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(vcx.debug_bounds("image-overlay").is_none());
}
