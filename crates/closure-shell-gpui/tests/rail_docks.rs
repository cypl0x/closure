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

#[gpui::test]
fn the_rail_has_a_grab_handle_on_its_edge(cx: &mut gpui::TestAppContext) {
    // "dockable *and resizable* most left panel". Docking is two
    // widths; the item asked for the range as well, and the outline
    // column beside it has had a drag handle since the day it stopped
    // being a fixed 420px.
    let (_dir, _view, vcx) = visual_window(cx, NOTE);
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("rail-resize").is_some(), "no edge to drag");
}

#[gpui::test]
fn a_dragged_width_is_what_gets_painted(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, NOTE);
    vcx.run_until_parked();
    view.update(vcx, |v, cx| {
        v.set_rail_width(240.0);
        cx.notify();
    });
    vcx.run_until_parked();
    // The column, not the whole rail: the 6px grab strip beside it is
    // furniture, and a width the user set should be the width they see
    // the buttons in.
    let w = f32::from(
        vcx.debug_bounds("rail-column")
            .expect("rail column")
            .size
            .width,
    );
    assert!((w - 240.0).abs() < 1.0, "asked for 240px and got {w}px");
}

#[gpui::test]
fn a_docked_rail_ignores_the_width(cx: &mut gpui::TestAppContext) {
    // Docked is a state, not a width: dragging it wide and then
    // docking must give the icons, not a wide column of icons.
    let (_dir, view, vcx) = visual_window(cx, NOTE);
    view.update(vcx, |v, cx| {
        v.set_rail_width(300.0);
        v.run_command("toggle-rail", cx);
    });
    vcx.run_until_parked();
    let w = f32::from(vcx.debug_bounds("rail").expect("rail").size.width);
    assert!(w < 60.0, "docked but {w}px wide");
}
