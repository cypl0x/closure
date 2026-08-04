//! "dockable and resizable most left panel (Outline, Agenda, Blocks,
//! …). Currently it has a fixed width. I would like to make it
//! dockable, in order that just the icons are visible and none of the
//! text."
//!
//! The rail is painted, so what it costs the window is measurable.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const NOTE: &str = "\
* One
:PROPERTIES:
:ID: 01RAILGPUIAAAAAAAAAAAAAAAA
:END:
body
";

#[gpui::test]
fn docking_the_rail_gives_the_width_back(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, NOTE);
    vcx.run_until_parked();
    let wide = vcx.debug_bounds("rail").expect("the rail is painted");
    let wide_w = f32::from(wide.size.width);

    view.update(vcx, |v, cx| v.run_command("toggle-rail", cx));
    vcx.run_until_parked();
    let narrow = vcx.debug_bounds("rail").expect("the rail is still painted");
    let narrow_w = f32::from(narrow.size.width);

    assert!(
        narrow_w < wide_w * 0.5,
        "docked the rail is {narrow_w}px against {wide_w}px — it did not collapse"
    );
}

#[gpui::test]
fn the_icons_survive_docking(cx: &mut gpui::TestAppContext) {
    // "just the icons are visible": the buttons are still there and
    // still clickable, which is the whole point of docking rather than
    // hiding.
    let (_dir, view, vcx) = visual_window(cx, NOTE);
    view.update(vcx, |v, cx| v.run_command("toggle-rail", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("rail-outline").is_some(),
        "the Outline button went away with its label"
    );
    assert!(vcx.debug_bounds("rail-agenda").is_some());
}

#[gpui::test]
fn it_opens_again(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, NOTE);
    vcx.run_until_parked();
    let before = f32::from(vcx.debug_bounds("rail").expect("rail").size.width);
    for _ in 0..2 {
        view.update(vcx, |v, cx| v.run_command("toggle-rail", cx));
        vcx.run_until_parked();
    }
    let after = f32::from(vcx.debug_bounds("rail").expect("rail").size.width);
    assert!(
        (after - before).abs() < 0.5,
        "a round trip left the rail at {after}px instead of {before}px"
    );
}

#[gpui::test]
fn the_rail_has_a_handle_you_can_click(cx: &mut gpui::TestAppContext) {
    // A rail you can only dock from the keyboard is not a dockable
    // rail: the people who want the icons back are the people reaching
    // for the mouse.
    let (_dir, _view, vcx) = visual_window(cx, NOTE);
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("rail-handle").is_some(),
        "no handle to grab"
    );
}
