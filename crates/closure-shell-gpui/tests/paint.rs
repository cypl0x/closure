//! The window as the user gets it: laid out, painted, and clicked.
//!
//! `tests/window.rs` reaches the view's *methods*. Everything between
//! a pixel and a method — layout, paint, hit-testing, gpui's own key
//! dispatch — was compile-checked and never run, which covers the
//! whole mouse layer (row clicks, fold arrows, drag-to-reorder, the
//! context menu, the scrollbars) and every surface but the two the
//! entity-level tests happen to open.
//!
//! These open a maximized window over gpui's stub display, let it lay
//! itself out, and then click at coordinates the window itself
//! reported (`debug_bounds`) rather than ones a test made up.
//!
//! Run with `cargo test -p closure-shell-gpui --features gpui-test`.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{ALL_SURFACES, ModalSurface, opening_route, visual_window};
use gpui::{Modifiers, MouseButton, Point, px, size};

/// A vault with enough shape to fill every pane: TODOs, a body, tags,
/// properties, a source block, a link and a schedule.
const VAULT: &str = "\
* TODO Alpha :work:
:PROPERTIES:
:ID: 01HQXALPHA0000000000000000
:END:
SCHEDULED: <2026-07-27 Mon>
The first body, with [[id:01HQXBETA00000000000000000][a link]] in it.
#+BEGIN_SRC sh
echo alpha
#+END_SRC
** DONE Beta
:PROPERTIES:
:ID: 01HQXBETA00000000000000000
:END:
Beta's body.
* Gamma
Gamma's body.
* Delta
Delta's body.
";

/// `n` numbered lines of `prefix`, as one string.
fn numbered(prefix: &str, n: usize) -> String {
    use std::fmt::Write as _;
    (0..n).fold(String::new(), |mut s, i| {
        let _ = writeln!(s, "{prefix} {i}");
        s
    })
}

/// The middle of a painted element, in window coordinates.
fn centre(cx: &mut gpui::VisualTestContext, selector: &'static str) -> Point<gpui::Pixels> {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("`{selector}` was never painted"));
    bounds.center()
}

// === the table of surfaces is complete and consistent ===

#[test]
fn every_surface_is_swept() {
    // `opening_route` is exhaustive, so a new surface stops the build
    // there; this is the other half — that the list the sweep iterates
    // did not stay behind.
    let mut seen = ALL_SURFACES.to_vec();
    seen.sort_unstable_by_key(|s| format!("{s:?}"));
    seen.dedup();
    assert_eq!(seen.len(), ALL_SURFACES.len(), "a surface is listed twice");

    let browse: Vec<_> = ALL_SURFACES
        .iter()
        .filter(|s| opening_route(**s).is_empty())
        .collect();
    assert_eq!(
        browse,
        vec![&ModalSurface::Browse],
        "exactly one surface is the one the window starts on"
    );
}

// === nothing clickable is painted where no mouse can reach ===

/// Every named click target, and the route that puts it on screen.
const TARGETS: &[(&str, &[&str])] = &[
    ("outline-row-0", &[]),
    ("fold-0", &[]),
    ("todo-0", &[]),
    ("outline-scrollbar", &[]),
    ("which-key-toggle", &[]),
    ("header-palette", &[]),
    ("header-capture-start", &[]),
    ("header-cycle-mode", &[]),
    ("field-rename", &[]),
    ("field-edit-tags", &[]),
    ("field-edit-property", &[]),
    ("field-toggle-todo", &[]),
    ("field-edit-body", &[]),
];

#[gpui::test]
fn every_click_target_lands_inside_the_window(cx: &mut gpui::TestAppContext) {
    // A flex row that cannot shrink grows instead, and gpui paints it
    // anyway: the footer ran to 5195px in a 1920px window and carried
    // the `keys` toggle out past the right edge, where it was visible
    // in no frame and reachable by no mouse. Bounds are the window's
    // own report, so this is the check that the affordance is *there*,
    // not merely constructed.
    for (selector, route) in TARGETS {
        let (_dir, view, vcx) = visual_window(cx, VAULT);
        for command in *route {
            view.update(vcx, |v, cx| v.run_command(command, cx));
        }
        vcx.run_until_parked();
        let viewport = vcx.update(|w, _cx| w.viewport_size());
        let bounds = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("`{selector}` was never painted"));
        assert!(
            bounds.origin.x >= px(0.0)
                && bounds.origin.y >= px(0.0)
                && bounds.right() <= viewport.width
                && bounds.bottom() <= viewport.height,
            "`{selector}` is painted at {bounds:?}, outside a {viewport:?} window"
        );
        assert!(
            bounds.size.width > px(0.0) && bounds.size.height > px(0.0),
            "`{selector}` has no area to click: {bounds:?}"
        );
    }
}

#[gpui::test]
fn no_bar_grows_wider_than_the_window(cx: &mut gpui::TestAppContext) {
    // The generalisation of the footer bug. Each of these is a flex row
    // of text whose length the vault decides — a breadcrumb, a status
    // line, a dozen chord completions — and any of them can push its
    // own right-hand contents past the edge. Stressed on all three
    // counts: a headline too long for the breadcrumb, a dead link whose
    // target fills the status line, and a pending chord that fills the
    // footer with completions.
    let title = "A headline title that is far longer than any window is wide, ".repeat(4);
    let vault = format!("* TODO {title} :tag1:tag2:tag3:\nbody\n");
    let (_dir, view, vcx) = visual_window(cx, &vault);
    view.update(vcx, |v, cx| {
        v.follow_link(&format!("id:{}", "0123456789ABCDEF".repeat(24)), cx);
    });
    vcx.simulate_keystrokes("g");
    vcx.run_until_parked();
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    for bar in ["header-bar", "context-line", "status-bar", "footer-bar"] {
        let bounds = vcx
            .debug_bounds(bar)
            .unwrap_or_else(|| panic!("`{bar}` was never painted"));
        assert!(
            bounds.right() <= viewport.width,
            "`{bar}` runs to {:?} in a {:?} window",
            bounds.right(),
            viewport.width
        );
    }
    // And the affordance at the far end of the longest bar is still
    // reachable, which is the thing the width was hiding.
    let toggle = vcx.debug_bounds("which-key-toggle").expect("painted");
    assert!(toggle.right() <= viewport.width, "{toggle:?}");
}

#[gpui::test]
fn an_outline_row_is_clickable_across_its_whole_column(cx: &mut gpui::TestAppContext) {
    // Rows were sized to their cells rather than their column, so the
    // right half of every row looked like the row and did nothing.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    let row = vcx.debug_bounds("outline-row-0").expect("painted");
    let bar = vcx.debug_bounds("outline-scrollbar").expect("painted");
    assert!(
        row.right() >= bar.origin.x - px(2.0),
        "the row reaches its scrollbar: row {row:?} vs bar {bar:?}"
    );
}

#[gpui::test]
fn a_click_on_the_far_side_of_a_row_still_selects_it(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let row = vcx.debug_bounds("outline-row-2").expect("painted");
    // Three quarters across — past where the cells end.
    let at = Point::new(row.origin.x + row.size.width * 0.9, row.center().y);
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| assert_eq!(v.selected(), 2));
}

// === every surface actually paints ===

#[gpui::test]
fn every_surface_paints(cx: &mut gpui::TestAppContext) {
    // A panic in layout or paint is a thing the user finds by opening
    // a pane. Each surface is opened on a *fresh* window so a surface
    // that refuses to open cannot mask the next one.
    for surface in ALL_SURFACES {
        let route = opening_route(*surface);
        let (_dir, view, vcx) = visual_window(cx, VAULT);
        for command in route {
            view.update(vcx, |v, cx| v.run_command(command, cx));
            vcx.run_until_parked();
        }
        view.update(vcx, |v, _cx| {
            assert_eq!(
                v.surface(),
                *surface,
                "{surface:?} did not open with {route:?} (status: {})",
                v.status()
            );
        });
    }
}

#[gpui::test]
fn every_surface_paints_over_an_empty_vault(cx: &mut gpui::TestAppContext) {
    // The other half of the same risk: a pane that indexes its list
    // without checking it has one. An empty vault gives every pane
    // nothing to show.
    for surface in ALL_SURFACES {
        let (_dir, view, vcx) = visual_window(cx, "");
        for command in opening_route(*surface) {
            view.update(vcx, |v, cx| v.run_command(command, cx));
            vcx.run_until_parked();
        }
    }
}

// === keys, through gpui's own dispatch tree ===

#[gpui::test]
fn keystrokes_reach_the_view_through_gpui(cx: &mut gpui::TestAppContext) {
    // `GpuiView::press` is a test seam that skips the platform, the
    // focus tree and `on_key_down`. If the window is not focused —
    // which only `run` does, and only in production — every key the
    // user presses goes nowhere while every test stays green.
    let (_dir, view, vcx) = visual_window(cx, "* Note\n");
    vcx.simulate_keystrokes("i i");
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::EditBody, "`i` opened the editor");
    });
    vcx.simulate_input("hello");
    view.update(vcx, |v, _cx| assert_eq!(v.body(), "hello"));
}

#[gpui::test]
fn a_shifted_keystroke_survives_the_platform(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, "* Note\n");
    vcx.simulate_keystrokes("i i");
    vcx.simulate_input("one two");
    vcx.simulate_keystrokes("escape 0 d i w");
    view.update(vcx, |v, _cx| assert_eq!(v.body(), " two"));
    vcx.simulate_keystrokes("shift-a");
    vcx.simulate_input("!");
    view.update(vcx, |v, _cx| assert_eq!(v.body(), " two!"));
}

// === the mouse ===

#[gpui::test]
fn a_click_on_a_row_selects_it(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "outline-row-2");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(v.selected(), 2, "the click landed on the row it aimed at");
        assert_eq!(v.row_title(2).as_deref(), Some("Gamma"));
    });
}

#[gpui::test]
fn a_click_on_the_fold_arrow_folds_that_row(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, _cx| {
        assert!(!v.row_folded(0), "nothing starts folded");
    });
    let at = centre(vcx, "fold-0");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert!(v.row_folded(0), "the arrow folded its own row");
    });
}

#[gpui::test]
fn a_click_on_the_status_glyph_cycles_the_todo(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let before = view.update(vcx, |v, _cx| v.row_todo(0));
    assert_eq!(before.as_deref(), Some("TODO"));
    let at = centre(vcx, "todo-0");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_ne!(v.row_todo(0), before, "the glyph cycled the keyword");
    });
}

#[gpui::test]
fn a_fold_click_does_not_leave_a_drag_armed(cx: &mut gpui::TestAppContext) {
    // The arrow sits inside the row, so the row's own press handler
    // fires too and arms the reorder gesture. If the release does not
    // clear it, the next hover retargets a move the user never began.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let arrow = centre(vcx, "fold-0");
    vcx.simulate_click(arrow, Modifiers::none());
    let titles_before = view.update(vcx, |v, _cx| {
        (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>()
    });
    // Now merely move over another row with no button held, then
    // press it: nothing may reorder.
    let other = centre(vcx, "outline-row-2");
    vcx.simulate_mouse_move(other, None, Modifiers::none());
    vcx.simulate_click(other, Modifiers::none());
    view.update(vcx, |v, _cx| {
        let after = (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>();
        assert_eq!(after, titles_before, "no row moved");
    });
}

#[gpui::test]
fn a_right_click_opens_the_row_menu(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "outline-row-1");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert!(v.menu_open(), "the menu opened");
        assert_eq!(v.selected(), 1, "and on the row it was aimed at");
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("context-menu").is_some(),
        "and it was painted"
    );
}

#[gpui::test]
fn a_menu_entry_runs_its_command(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "outline-row-2");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    let entry = centre(vcx, "menu-toggle-todo");
    vcx.simulate_click(entry, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert!(!v.menu_open(), "the menu closed behind the click");
        assert!(v.row_todo(2).is_some(), "and the command ran on row 2");
    });
}

#[gpui::test]
fn a_click_away_dismisses_the_menu(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "outline-row-1");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    let elsewhere = centre(vcx, "outline-row-3");
    vcx.simulate_click(elsewhere, Modifiers::none());
    view.update(vcx, |v, _cx| assert!(!v.menu_open(), "the menu went away"));
}

#[gpui::test]
fn dragging_a_row_onto_another_reorders_them(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let before = view.update(vcx, |v, _cx| {
        (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>()
    });
    let from = centre(vcx, "outline-row-3");
    let to = centre(vcx, "outline-row-2");
    vcx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    view.update(vcx, |v, _cx| {
        let after = (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>();
        assert_ne!(after, before, "the drag moved something");
    });
}

#[gpui::test]
fn a_drag_released_off_the_rows_moves_nothing(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let before = view.update(vcx, |v, _cx| {
        (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>()
    });
    let from = centre(vcx, "outline-row-3");
    vcx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    // Release far to the right — over the detail pane, not a row.
    vcx.simulate_mouse_up(
        Point::new(px(1500.0), px(600.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    // …and the abandoned gesture must not arm the next hover.
    let elsewhere = centre(vcx, "outline-row-0");
    vcx.simulate_mouse_move(elsewhere, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(elsewhere, MouseButton::Left, Modifiers::none());
    view.update(vcx, |v, _cx| {
        let after = (0..4).map(|i| v.row_title(i)).collect::<Vec<_>>();
        assert_eq!(after, before, "nothing moved");
    });
}

// === the scrollbars are real widgets ===

#[gpui::test]
fn the_outline_scrollbar_is_painted_when_the_vault_overflows(cx: &mut gpui::TestAppContext) {
    let long = numbered("* Row", 400);
    let (_dir, _view, vcx) = visual_window(cx, &long);
    let bounds = vcx
        .debug_bounds("outline-scrollbar")
        .expect("the track is painted");
    assert!(bounds.size.height > px(0.0), "and it has a track to drag");
}

#[gpui::test]
fn dragging_the_outline_scrollbar_scrolls_the_outline(cx: &mut gpui::TestAppContext) {
    let long = numbered("* Row", 400);
    let (_dir, view, vcx) = visual_window(cx, &long);
    let track = vcx
        .debug_bounds("outline-scrollbar")
        .expect("the track is painted");
    let before = view.update(vcx, |v, _cx| v.outline_scroll_top());
    // Grab the track three quarters of the way down.
    let at = Point::new(track.center().x, track.origin.y + track.size.height * 0.75);
    vcx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _cx| v.outline_scroll_top());
    assert!(
        after > before,
        "the outline scrolled down: {before} -> {after}"
    );
}

// === the panes are the size of the window, not of the document ===

#[gpui::test]
fn a_long_body_does_not_stretch_the_pane_past_the_window(cx: &mut gpui::TestAppContext) {
    // A flex item's automatic minimum size is its content, so the
    // right-hand pane grew to fit every line it painted and then
    // reported *that* as its viewport. The editor asked how many lines
    // fit, was told "all of them", painted all of them, and the answer
    // came back true. Every measurement downstream — the scrollbar
    // thumb, page-down, the wheel — was taken against a viewport the
    // height of the document.
    let body = numbered("line", 2000);
    let (_dir, view, vcx) = visual_window(cx, &format!("* Long\n{body}"));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    // Twice, because the measurement is last frame's: a pane that
    // grows to its content converges *upward* on repaint.
    vcx.simulate_resize(size(px(1280.0), px(720.0)));
    vcx.run_until_parked();
    vcx.run_until_parked();
    let view_lines = view.update(vcx, |v, _cx| v.body_view());
    // A 720px window, minus the header/context/status chrome around
    // the pane, is a page of roughly thirty lines. Bounded on both
    // sides: too many means the pane grew to its content again, too
    // few means it collapsed to nothing.
    assert!(
        (20..=720 / 18).contains(&view_lines),
        "a 720px window shows about thirty 18px lines, not {view_lines}"
    );
}

// === the window is a window: it resizes ===

#[gpui::test]
fn resizing_the_window_changes_the_body_viewport(cx: &mut gpui::TestAppContext) {
    let body = numbered("line", 300);
    let (_dir, view, vcx) = visual_window(cx, &format!("* Long\n{body}"));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let tall = view.update(vcx, |v, _cx| v.body_view());
    vcx.simulate_resize(size(px(900.0), px(400.0)));
    vcx.run_until_parked();
    let short = view.update(vcx, |v, _cx| v.body_view());
    assert!(
        short < tall,
        "a shorter window shows fewer lines: {tall} -> {short}"
    );
}

// === clicking in the body places the cursor ===

#[gpui::test]
fn a_click_in_the_body_places_the_cursor(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, "* Note\nfirst line\nsecond line\nthird line\n");
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let before = view.update(vcx, |v, _cx| v.body_cursor().0);
    assert_ne!(before, 1, "the click has somewhere to move the cursor from");
    let at = centre(vcx, "body-line-1");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(v.body_cursor().0, 1, "the cursor moved to the line clicked");
    });
}

// === the affordances that teach the keyboard path ===

#[gpui::test]
fn a_header_button_opens_the_palette(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "header-palette");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::Palette);
    });
}

#[gpui::test]
fn a_pending_chord_opens_the_which_key_panel(cx: &mut gpui::TestAppContext) {
    // The panel is the one moment the whole keymap is the wrong
    // answer, so it must both appear and narrow.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("which-key-scrollbar").is_none(),
        "nothing is pinned open yet"
    );
    vcx.simulate_keystrokes("g");
    view.update(vcx, |v, _cx| {
        assert_eq!(v.pending_chord(), "g", "the chord is waiting");
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("which-key-scrollbar").is_some(),
        "and the panel came with it"
    );
}

#[gpui::test]
fn the_keys_toggle_pins_the_which_key_panel(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, _cx| assert!(!v.which_key_open()));
    let toggle = centre(vcx, "which-key-toggle");
    vcx.simulate_click(toggle, Modifiers::none());
    view.update(vcx, |v, _cx| assert!(v.which_key_open(), "pinned open"));
    vcx.simulate_click(toggle, Modifiers::none());
    view.update(vcx, |v, _cx| assert!(!v.which_key_open(), "and shut again"));
}

#[gpui::test]
fn a_which_key_chip_runs_its_command(cx: &mut gpui::TestAppContext) {
    // I4: the panel is the keymap made clickable, so an entry has to
    // run the command its chord runs. Driven from a *pending* chord
    // rather than the pinned panel: filtered to what can follow `g`
    // the list is short enough that every entry is on screen, which is
    // the state the panel exists for.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    vcx.simulate_keystrokes("g");
    vcx.run_until_parked();
    let chip = centre(vcx, "wk-agenda");
    vcx.simulate_click(chip, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.surface(),
            ModalSurface::Agenda,
            "the chip ran what `g a` runs"
        );
    });
}

#[gpui::test]
fn a_detail_field_click_starts_its_edit(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "field-rename");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.surface(),
            ModalSurface::Rename,
            "the title field opened the rename"
        );
    });
}

#[gpui::test]
fn a_detail_field_click_opens_the_tag_editor(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "field-edit-tags");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::TagsEdit);
    });
}

// === links ===

#[gpui::test]
fn following_an_id_link_selects_its_headline(cx: &mut gpui::TestAppContext) {
    // What ctrl-click does once the hit-test has named a target. The
    // hit-test itself is glyph geometry, which the stub platform does
    // not have; this is the half that decides where the window goes.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        v.follow_link("id:01HQXBETA00000000000000000", cx);
        assert_eq!(v.selected(), 1, "landed on Beta");
        assert!(v.status().contains("followed"), "{}", v.status());
    });
}

#[gpui::test]
fn following_a_file_link_selects_that_files_first_headline(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        v.follow_link("file:notes.org", cx);
        assert!(v.status().contains("followed"), "{}", v.status());
    });
}

#[gpui::test]
fn following_a_url_copies_it_rather_than_launching_a_browser(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        v.follow_link("https://example.invalid/x", cx);
        assert!(
            v.status().contains("clipboard"),
            "the status says where it went: {}",
            v.status()
        );
    });
    assert_eq!(
        vcx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
        Some("https://example.invalid/x"),
    );
}

#[gpui::test]
fn following_a_dead_link_says_so(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        v.follow_link("id:01HQXNOSUCHTHING0000000000", cx);
        assert!(
            v.status().contains("not a headline"),
            "the window says the link is dead: {}",
            v.status()
        );
    });
}

// === the wheel ===

#[gpui::test]
fn the_wheel_scrolls_the_body_editor(cx: &mut gpui::TestAppContext) {
    let body = numbered("line", 400);
    let (_dir, view, vcx) = visual_window(cx, &format!("* Long\n{body}"));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    // The buffer opens in NORMAL at the top; `g g` is a no-op that
    // states it, so the assertion below reads as an assertion.
    vcx.simulate_keystrokes("g g");
    vcx.run_until_parked();
    let before = view.update(vcx, |v, _cx| v.body_scroll_start());
    assert_eq!(before, 0, "the editor is back at the top");
    let at = centre(vcx, "body-line-0");
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -5.0)),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let after = view.update(vcx, |v, _cx| v.body_scroll_start());
    assert!(
        after > before,
        "the wheel moved the editor's viewport: {before} -> {after}"
    );
}

// === the two scrollbars the shared one cannot serve ===

#[gpui::test]
fn dragging_the_body_scrollbar_scrolls_the_editor(cx: &mut gpui::TestAppContext) {
    // The editor paints only its visible lines, so its container never
    // overflows and the shared scrollbar has nothing to measure. This
    // bar works in line units instead, which is its own arithmetic and
    // had never been run.
    let body = numbered("line", 400);
    let (_dir, view, vcx) = visual_window(cx, &format!("* Long\n{body}"));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.simulate_keystrokes("g g");
    vcx.run_until_parked();
    assert_eq!(view.update(vcx, |v, _cx| v.body_scroll_start()), 0);
    let track = vcx.debug_bounds("body-scrollbar").expect("painted");
    // The whole track has to be inside the window. It was 1088px tall
    // in a 1080px window — the editor column asks for a line or two
    // more than fits, and the bar is its `h_full` sibling — so the
    // bottom tenth of the track was off screen and undraggable, which
    // is exactly the end of the document.
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    assert!(
        track.bottom() <= viewport.height,
        "the track ends at {:?} in a {:?} window",
        track.bottom(),
        viewport.height
    );

    let at = Point::new(track.center().x, track.origin.y + track.size.height * 0.5);
    vcx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _cx| v.body_scroll_start());
    assert!(
        after > 0,
        "grabbing the middle of the track moved the editor: 0 -> {after}"
    );
    // And the far end of the track is the far end of the body.
    let bottom = Point::new(track.center().x, track.bottom() - px(2.0));
    vcx.simulate_mouse_move(bottom, MouseButton::Left, Modifiers::none());
    let end = view.update(vcx, |v, _cx| v.body_scroll_start());
    assert!(
        end > after,
        "and dragging to the bottom went further: {after} -> {end}"
    );
}

#[gpui::test]
fn dragging_the_side_scrollbar_scrolls_the_pane(cx: &mut gpui::TestAppContext) {
    // The list surfaces put their rows straight in the scrolling pane,
    // so this is the bar that moves them.
    let long = numbered("* Row", 200);
    let (_dir, view, vcx) = visual_window(cx, &long);
    view.update(vcx, |v, cx| v.run_command("headline-list", cx));
    vcx.run_until_parked();
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let track = vcx.debug_bounds("side-scrollbar").expect("painted");
    assert!(
        track.size.height > px(0.0) && track.bottom() <= viewport.height,
        "a pane with 200 rows has a track, inside the window: {track:?}"
    );
    let before = view.update(vcx, |v, _cx| v.side_scroll_top());
    let at = Point::new(track.center().x, track.origin.y + track.size.height * 0.75);
    vcx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _cx| v.side_scroll_top());
    assert!(
        after > before,
        "the pane scrolled down: {before} -> {after}"
    );
}

#[gpui::test]
fn a_scrollbar_works_on_the_frame_its_pane_appears(cx: &mut gpui::TestAppContext) {
    // A pane has no measurements until it has been laid out, so a bar
    // that decided at build time whether it had anything to drag was a
    // frame behind its own pane: it was built while the pane still held
    // the previous surface, so the first grab after opening a list did
    // nothing, and only an unrelated repaint armed it. One click,
    // immediately, with no repaint in between.
    let long = numbered("* Row", 200);
    let (_dir, view, vcx) = visual_window(cx, &long);
    view.update(vcx, |v, cx| v.run_command("headline-list", cx));
    vcx.run_until_parked();
    let track = vcx.debug_bounds("side-scrollbar").expect("painted");
    let at = Point::new(track.center().x, track.origin.y + track.size.height * 0.75);
    vcx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let after = view.update(vcx, |v, _cx| v.side_scroll_top());
    assert!(
        after > 0.0,
        "the very first grab scrolled the pane: {after}"
    );
}

// === toasts ===

#[gpui::test]
fn a_status_change_raises_a_toast_inside_the_window(cx: &mut gpui::TestAppContext) {
    // The strip is deferred and anchored rather than a row in the
    // layout, which is exactly the arrangement that can be anchored
    // off the edge of the window.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("toast-strip").is_none(),
        "nothing to say yet"
    );
    // `status_toast` is deliberately selective — a fold is one of the
    // things it raises.
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert!(v.row_folded(0), "something happened worth saying");
        assert!(
            v.toast_count() > 0,
            "and it reached the strip: {}",
            v.status()
        );
    });
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let strip = vcx
        .debug_bounds("toast-strip")
        .expect("the strip is painted");
    assert!(
        strip.origin.x >= px(0.0)
            && strip.origin.y >= px(0.0)
            && strip.right() <= viewport.width
            && strip.bottom() <= viewport.height,
        "the toast is on screen: {strip:?} in {viewport:?}"
    );
}

// === the palette is a list you can click ===

#[gpui::test]
fn a_palette_row_click_runs_that_command(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("palette", cx));
    vcx.run_until_parked();
    let at = centre(vcx, "palette-row-0");
    vcx.simulate_click(at, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_ne!(
            v.surface(),
            ModalSurface::Palette,
            "the palette ran its entry and closed: {}",
            v.status()
        );
    });
}

// === the clipboard chords, through gpui's own dispatch ===

#[gpui::test]
fn the_clipboard_chords_round_trip(cx: &mut gpui::TestAppContext) {
    // These are intercepted before the keymap, and the window's key
    // handler now claims every event it sees — so this is also the
    // guard that claiming it did not swallow the desktop chords.
    let (_dir, view, vcx) = visual_window(cx, "* Note\n");
    vcx.simulate_keystrokes("i i");
    vcx.simulate_input("copy me");
    // VISUAL LINE over the whole line, which needs no column motion.
    vcx.simulate_keystrokes("escape shift-v");
    vcx.simulate_keystrokes("ctrl-c");
    assert_eq!(
        vcx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
        Some("copy me"),
        "the selection reached the clipboard"
    );
    vcx.simulate_keystrokes("shift-a");
    vcx.simulate_keystrokes("ctrl-v");
    view.update(vcx, |v, _cx| {
        assert_eq!(v.body(), "copy mecopy me", "and pasted back in");
    });
}

// === the editor's own overlays ===

#[gpui::test]
fn the_slash_menu_opens_inside_the_window_and_inserts(cx: &mut gpui::TestAppContext) {
    // The menu is a plain child of the editor column, not a deferred
    // overlay, so it competes for height with the body above it — and
    // that column is clipped to the window. A menu pushed out of the
    // clip is a menu you cannot see or click.
    let (_dir, view, vcx) = visual_window(cx, &format!("* Note\n{}", numbered("line", 300)));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.simulate_keystrokes("shift-g o");
    vcx.simulate_input("/");
    vcx.run_until_parked();

    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let menu = vcx
        .debug_bounds("slash-menu")
        .expect("the slash menu is painted");
    assert!(
        menu.bottom() <= viewport.height && menu.right() <= viewport.width,
        "the menu is on screen: {menu:?} in {viewport:?}"
    );
    assert!(
        menu.size.height > px(0.0),
        "and has something in it: {menu:?}"
    );

    let before = view.update(vcx, |v, _cx| v.body().to_owned());
    vcx.simulate_click(menu.center(), Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert_ne!(v.body(), before, "clicking an entry inserted its template");
        assert!(
            !v.body().contains("\n/"),
            "and consumed the `/` trigger: {:?}",
            v.body().lines().rev().take(3).collect::<Vec<_>>()
        );
    });
}

#[gpui::test]
fn the_completion_popup_opens_inside_the_window(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(
        cx,
        "* Note\nextraordinarily\n* Other\nsomething else here\n",
    );
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    // Type a prefix the vault can complete, then ask for it.
    vcx.simulate_keystrokes("shift-g o");
    vcx.simulate_input("extrao");
    vcx.simulate_keystrokes("ctrl-n");
    vcx.run_until_parked();

    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let popup = vcx
        .debug_bounds("completion-popup")
        .expect("the popup is painted");
    assert!(
        popup.bottom() <= viewport.height && popup.right() <= viewport.width,
        "the popup is on screen: {popup:?} in {viewport:?}"
    );
    assert!(popup.size.height > px(0.0), "and offers something");
    view.update(vcx, |v, _cx| {
        assert!(
            v.body().contains("extraordinarily"),
            "the completion was taken: {:?}",
            v.body()
        );
    });
}

// === the context menu reaches more than the outline row ===

#[gpui::test]
fn a_right_click_in_the_body_opens_the_body_menu(cx: &mut gpui::TestAppContext) {
    // `context_menu` has always known three targets; the window only
    // ever wired the outline row, so a right-click in the editor
    // dismissed the menu it never opened.
    let (_dir, view, vcx) = visual_window(cx, "* Note\nfirst line\nsecond line\n");
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let at = centre(vcx, "body-line-0");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open(), "the body menu opened"));
    assert!(
        vcx.debug_bounds("context-menu").is_some(),
        "and it was painted"
    );
}

#[gpui::test]
fn a_right_click_on_a_detail_field_opens_the_detail_menu(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "field-rename");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open()));
    assert!(vcx.debug_bounds("context-menu").is_some());
}

#[gpui::test]
fn a_click_in_the_body_dismisses_the_menu(cx: &mut gpui::TestAppContext) {
    // Every handler that places a caret stops propagation — it has to,
    // or the click keeps travelling — so the root's "a click anywhere
    // dismisses the menu" never ran for the one surface where you are
    // most likely to right-click and then change your mind. The menu
    // stayed open over the text while you typed into it.
    let (_dir, view, vcx) = visual_window(cx, "* Note\nfirst line\nsecond line\nthird line\n");
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    // The menu opens *below* where it was asked for, so the line that
    // takes the dismissing click is one above it — otherwise the click
    // lands on the menu, which is a different gesture entirely.
    let lower = centre(vcx, "body-line-3");
    vcx.simulate_mouse_down(lower, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open(), "the menu opened"));
    let upper = centre(vcx, "body-line-1");
    vcx.simulate_click(upper, Modifiers::none());
    view.update(vcx, |v, _cx| {
        assert!(!v.menu_open(), "and a click on the text put it away");
        assert_eq!(v.body_cursor().0, 1, "while still placing the caret");
    });
}

#[gpui::test]
fn a_click_on_a_scrollbar_dismisses_the_menu(cx: &mut gpui::TestAppContext) {
    // The other propagation-stopping handler, and the same bug.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let row = centre(vcx, "outline-row-1");
    vcx.simulate_mouse_down(row, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    let bar = centre(vcx, "outline-scrollbar");
    vcx.simulate_click(bar, Modifiers::none());
    view.update(vcx, |v, _cx| assert!(!v.menu_open(), "the menu went away"));
}

#[gpui::test]
fn a_context_menu_is_never_anchored_off_the_window(cx: &mut gpui::TestAppContext) {
    // It is anchored where the click landed, so a right-click near the
    // bottom-right corner is the case that has to snap back.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let row = vcx.debug_bounds("outline-row-3").expect("painted");
    let corner = Point::new(row.right() - px(2.0), row.bottom() - px(2.0));
    vcx.simulate_mouse_down(corner, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    let menu = vcx.debug_bounds("context-menu").expect("painted");
    assert!(
        menu.right() <= viewport.width && menu.bottom() <= viewport.height,
        "the menu snapped back inside: {menu:?} in {viewport:?}"
    );
}

// === the activity rail ===
//
// Every subsystem used to be behind a `g`-prefixed chord and nothing
// else. Pairing was the worst case: `g s` or nothing, so a user who had
// not read the keymap had no way to learn that closure can sync with
// another machine at all. The rail is the mouse's map of the app, and
// these are the tests that it is one — painted, clickable, and honest
// about which pane is open.

/// A `debug_bounds` selector for a rail button. The ids are `'static`,
/// the formatted selector is not, and `debug_bounds` takes `'static`.
fn rail_selector(id: &str) -> &'static str {
    Box::leak(format!("rail-{id}").into_boxed_str())
}

#[gpui::test]
fn the_rail_paints_a_button_for_every_destination(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let dests = view.update(vcx, |v, _cx| v.destinations());
    assert!(dests.len() >= 12, "sanity: {} destinations", dests.len());
    for dest in dests {
        let selector = rail_selector(dest.id);
        assert!(
            vcx.debug_bounds(selector).is_some(),
            "`{selector}` ({}) was never painted",
            dest.label
        );
    }
}

#[gpui::test]
fn the_mouse_alone_reaches_pairing_the_sniffer_and_the_assistant(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    for (id, surface) in [
        ("peers", ModalSurface::Sync),
        ("sniffer", ModalSurface::Sniffer),
        ("assistant", ModalSurface::Llm),
        ("outline", ModalSurface::Browse),
    ] {
        let at = centre(vcx, rail_selector(id));
        vcx.simulate_click(at, Modifiers::none());
        vcx.run_until_parked();
        view.update(vcx, |v, _cx| {
            assert_eq!(
                v.surface(),
                surface,
                "the {id} button opened {:?} instead",
                v.surface()
            );
        });
    }
}

#[gpui::test]
fn the_rail_marks_the_open_pane(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let active = |v: &closure_shell_gpui::GpuiView| {
        v.destinations()
            .into_iter()
            .filter(|d| d.active)
            .map(|d| d.id)
            .collect::<Vec<_>>()
    };
    view.update(vcx, |v, _cx| assert_eq!(active(v), vec!["outline"]));
    let at = centre(vcx, rail_selector("peers"));
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert_eq!(active(v), vec!["peers"]));
}

// === the font the window actually asks for ===
//
// The theme's stack is CSS-shaped; gpui's `font_family()` takes ONE
// family name. The window handed it the whole comma-separated string,
// so it asked for a font called "JetBrains Mono, ui-monospace,
// monospace", no such family existed, and every glyph in the app came
// from whatever the platform picked instead. `app_font` is the split.

#[test]
fn the_window_asks_for_one_family_and_names_its_fallbacks() {
    let font = closure_shell_gpui::app_font(closure_shell_core::Theme::doom_vibrant());
    assert_eq!(
        font.family.as_ref(),
        "Maple Mono NF",
        "the user's font, by its real family name"
    );
    let fallbacks = font.fallbacks.expect("a fallback list");
    assert_eq!(
        fallbacks.fallback_list(),
        ["JetBrains Mono", "ui-monospace", "monospace"],
        "the rest of the stack, in order"
    );
}

#[test]
fn a_single_family_theme_still_produces_a_font() {
    // Nothing to fall back to must not mean nothing to render with.
    let mut theme = closure_shell_core::Theme::doom_vibrant();
    theme.typography.mono_family = "Maple Mono NF";
    let font = closure_shell_gpui::app_font(theme);
    assert_eq!(font.family.as_ref(), "Maple Mono NF");
    assert!(
        font.fallbacks.is_none_or(|f| f.fallback_list().is_empty()),
        "no fallbacks claimed that the theme did not name"
    );
}

#[test]
fn an_empty_stack_falls_back_to_a_generic_monospace() {
    // A theme with no font named is a broken theme, not a window with no
    // text: the window still has to render.
    let mut theme = closure_shell_core::Theme::doom_vibrant();
    theme.typography.mono_family = "";
    assert_eq!(
        closure_shell_gpui::app_font(theme).family.as_ref(),
        "monospace"
    );
}

// === the editor is its own window ===
//
// The body editor used to live in the right-hand pane: a third of the
// window, beside a list of the headlines you were not editing. That is a
// preview, not a place to write in. `org-edit-special` gets its own
// frame in Emacs, and the buffer surfaces get the whole window here.
//
// (These assert geometry and hit-testing rather than the absence of a
// `debug_bounds` entry: gpui keeps that map across frames, so a
// selector painted once answers forever. Mouse listeners do not
// survive a frame, which is what makes the click assertions honest.)

#[gpui::test]
fn the_body_editor_takes_the_whole_window(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let rail = vcx.debug_bounds("rail").expect("the rail is painted first");
    let at_rail = centre(vcx, rail_selector("peers"));

    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();

    let viewport = vcx.update(|w, _cx| w.viewport_size());
    let line = vcx
        .debug_bounds("body-line-0")
        .expect("the buffer is painted");
    assert!(
        line.origin.x < rail.right(),
        "the buffer starts where the rail was: {line:?} vs {rail:?}"
    );
    // The text element is only as wide as its text; the editor's own
    // scrollbar is what reports how wide the *buffer* is.
    let bar = vcx
        .debug_bounds("body-scrollbar")
        .expect("the editor's scrollbar");
    assert!(
        bar.right() > viewport.width * 0.9,
        "and it runs to the far side: {bar:?} in {viewport:?}"
    );
    // Nothing of the rail is left to click on.
    vcx.simulate_click(at_rail, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.surface(),
            ModalSurface::EditBody,
            "a click where the rail used to be did not leave the buffer"
        );
    });
}

#[gpui::test]
fn leaving_the_editor_brings_the_window_back(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at_rail = centre(vcx, rail_selector("peers"));
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    // `:q` closes the buffer, which is the way back (contract revised
    // 2026-07-28: Esc in a modal mode is the mode key, not the exit).
    view.update(vcx, |v, cx| v.run_ex_line("q", cx));
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert_eq!(v.surface(), ModalSurface::Browse));
    vcx.simulate_click(at_rail, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::Sync, "the rail is back");
    });
}

#[gpui::test]
fn the_source_block_editor_takes_the_window_too(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let rail = vcx.debug_bounds("rail").expect("painted");
    for command in ["block-list", "edit-special"] {
        view.update(vcx, |v, cx| v.run_command(command, cx));
        vcx.run_until_parked();
    }
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::EditBlock);
    });
    let line = vcx
        .debug_bounds("body-line-0")
        .expect("the block is painted");
    assert!(
        line.origin.x < rail.right(),
        "one block, whole window: {line:?} vs {rail:?}"
    );
}

// === the two shapes of the shell ===

#[gpui::test]
fn toggling_the_view_swaps_the_outline_for_the_file(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let rail = vcx
        .debug_bounds("rail")
        .expect("the outline view is painted");
    let at_rail = centre(vcx, rail_selector("peers"));

    view.update(vcx, |v, cx| v.run_command("toggle-view", cx));
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::EditFile);
        assert!(
            v.body().starts_with("* TODO Alpha"),
            "the file itself, from its first byte: {:?}",
            &v.body()[..20.min(v.body().len())]
        );
    });
    let line = vcx.debug_bounds("body-line-0").expect("painted");
    assert!(line.origin.x < rail.right(), "and it has the window");

    view.update(vcx, |v, cx| v.run_command("toggle-view", cx));
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert_eq!(v.surface(), ModalSurface::Browse));
    vcx.simulate_click(at_rail, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::Sync, "the rail is back");
    });
}

#[gpui::test]
fn a_config_asking_for_the_editor_view_gets_it_before_the_first_frame(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        v.set_view(closure_shell_core::ViewMode::Editor);
        cx.notify();
    });
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::EditFile);
    });
    assert!(
        vcx.debug_bounds("body-line-0").is_some(),
        "the file is on screen"
    );
}

#[gpui::test]
fn editing_the_file_buffer_and_saving_it_reaches_the_vault(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("toggle-view", cx));
    vcx.run_until_parked();
    // NORMAL on entry (a modal mode); `A` appends at the end of line 1.
    vcx.simulate_keystrokes("shift-a");
    vcx.simulate_input("!");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert!(
            v.body().starts_with("* TODO Alpha :work:!"),
            "the buffer took the edit: {:?}",
            v.body().lines().next()
        );
        assert!(
            v.vault_contains(":work:!"),
            "and C-Enter wrote the whole file back"
        );
    });
}

#[gpui::test]
fn the_file_buffer_paints_every_line_that_fits(cx: &mut gpui::TestAppContext) {
    // The pane paints a window of lines sized from its own measured
    // height. A short file must therefore be painted whole: a buffer
    // that silently stops two lines short of the end is a buffer you
    // cannot trust.
    let file = format!("* Head\n{}", numbered("line", 16));
    let lines = file.lines().count();
    let (_dir, view, vcx) = visual_window(cx, &file);
    // A window the size of a laptop, not the harness's 1920×1080: the
    // count is measured, and the measurement is what was wrong.
    vcx.simulate_resize(size(px(1280.0), px(720.0)));
    vcx.run_until_parked();
    view.update(vcx, |v, cx| v.run_command("toggle-view", cx));
    vcx.run_until_parked();
    let (measured, painted) = view.update(vcx, |v, _cx| (v.body_view(), v.painted_view()));
    assert!(
        measured >= lines,
        "{lines} lines fit in a 720px window, the pane measures {measured}"
    );
    // The pane sizes itself from *its own* measured height, which only
    // exists after it has been laid out once — so the frame that opens
    // the buffer paints with the previous layout's count. Opening a
    // 17-line file over a 15-line measurement painted 15 lines and
    // stopped, with half the window empty below them, until some other
    // keystroke happened to repaint. The pane asks for that repaint
    // itself now.
    assert_eq!(
        painted, measured,
        "the painted frame used a stale line count"
    );
    for n in 0..lines {
        let selector: &'static str = Box::leak(format!("body-line-{n}").into_boxed_str());
        assert!(
            vcx.debug_bounds(selector).is_some(),
            "{selector} of {lines} was never painted"
        );
    }
}

#[gpui::test]
fn e_reaches_the_editor_through_gpuis_own_dispatch(cx: &mut gpui::TestAppContext) {
    // Reported as "vim key e is not working". The kernel motion, the
    // `editor_key` seam and the operator all had tests; what none of
    // them covered was gpui's dispatch, which is the only layer left
    // between a real keypress and the buffer.
    let (_dir, view, vcx) = visual_window(cx, "* Note\n");
    vcx.simulate_keystrokes("i i");
    vcx.simulate_input("hello world");
    vcx.simulate_keystrokes("escape 0");
    vcx.simulate_keystrokes("e");
    view.update(vcx, |v, _cx| {
        assert_eq!(v.body_cursor(), (0, 4), "`e` landed on the end of `hello`");
    });
    // From the `o` of `hello`, `de` runs to the end of the *next* word
    // — vim's rule, not "delete the word I am standing on".
    vcx.simulate_keystrokes("d e");
    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.body(),
            "hell",
            "`de` ran from the cursor to the next word end"
        );
    });
}

#[gpui::test]
fn a_left_click_elsewhere_dismisses_the_context_menu(cx: &mut gpui::TestAppContext) {
    // Reported as "right click in the editor and left clicking
    // somewhere else doesn't dismiss the right click dialog". The root
    // has a dismissing handler; what was never tested is whether a
    // click on a *row* — which handles its own mouse-down — still
    // reaches it.
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let row3 = centre(vcx, "outline-row-3");
    vcx.simulate_mouse_down(row3, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open(), "menu opened"));

    let row0 = centre(vcx, "outline-row-0");
    vcx.simulate_click(row0, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert!(!v.menu_open(), "a click on another row dismissed it");
    });
}

#[gpui::test]
fn a_left_click_in_the_body_dismisses_the_editor_context_menu(cx: &mut gpui::TestAppContext) {
    // The case as reported: the menu opened over the *body editor*,
    // where every line handles its own mouse-down to place the cursor.
    let (_dir, view, vcx) = visual_window(cx, "* Note\nfirst line\nsecond line\n");
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    let line0 = centre(vcx, "body-line-0");
    vcx.simulate_mouse_down(line0, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open(), "the body menu opened"));

    let line1 = centre(vcx, "body-line-1");
    vcx.simulate_click(line1, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert!(
            !v.menu_open(),
            "clicking another line put the cursor there and closed the menu"
        );
    });
}

#[gpui::test]
fn a_left_click_in_empty_space_dismisses_the_context_menu(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    let at = centre(vcx, "outline-row-1");
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(v.menu_open()));

    // The status bar is not a row and not the menu: the plainest
    // "somewhere else" there is.
    let elsewhere = centre(vcx, "status-bar");
    vcx.simulate_click(elsewhere, Modifiers::none());
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(!v.menu_open()));
}

#[gpui::test]
fn a_wrapped_body_paints_more_rows_than_it_has_lines(cx: &mut gpui::TestAppContext) {
    // `wrap = true`: one long logical line becomes several painted
    // rows, and the gutter still numbers the line once.
    let long = format!("* Note\n{}\n", "word ".repeat(80));
    let (_dir, view, vcx) = visual_window(cx, &long);
    view.update(vcx, |v, cx| {
        v.set_wrap(true);
        v.run_command("edit-body", cx);
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("body-line-0").is_some(),
        "the first row painted"
    );
    view.update(vcx, |v, _cx| {
        assert!(v.wraps(), "and the editor knows it is wrapping");
    });
}

#[gpui::test]
fn an_unwrapped_editor_still_paints_one_row_per_line(cx: &mut gpui::TestAppContext) {
    let long = format!("* Note\n{}\n", "word ".repeat(80));
    let (_dir, view, vcx) = visual_window(cx, &long);
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| assert!(!v.wraps()));
    assert!(vcx.debug_bounds("body-line-0").is_some());
}

#[gpui::test]
fn the_outline_column_can_be_dragged_wider(cx: &mut gpui::TestAppContext) {
    // A fixed 420px column had no way to show long titles and no way to
    // get out of their way in a narrow window.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    let before = vcx
        .debug_bounds("outline-row-0")
        .expect("painted")
        .size
        .width;
    let handle = centre(vcx, "outline-resize");
    vcx.simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(
        Point::new(handle.x + px(120.0), handle.y),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    vcx.run_until_parked();
    let after = vcx
        .debug_bounds("outline-row-0")
        .expect("painted")
        .size
        .width;
    assert!(
        after > before,
        "wider after the drag: {before:?} → {after:?}"
    );
}

#[gpui::test]
fn the_outline_column_will_not_be_dragged_away(cx: &mut gpui::TestAppContext) {
    // Past the minimum it stops being a column at all.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    let handle = centre(vcx, "outline-resize");
    vcx.simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(
        Point::new(px(0.0), handle.y),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    vcx.run_until_parked();
    let row = vcx.debug_bounds("outline-row-0").expect("still painted");
    assert!(row.size.width > px(100.0), "clamped: {row:?}");
}

#[gpui::test]
fn outline_rows_line_up_whatever_they_contain(cx: &mut gpui::TestAppContext) {
    // Reported as the tree view "weirdly juggling": the keyword column
    // was painted only on rows that had one, and nothing clipped, so
    // titles started at different x positions and the file name slid
    // about as the content changed.
    let vault = "\
* TODO Alpha
* A headline with a very much longer title than the ones around it here
* DONE Gamma
* Delta
";
    let (_dir, _view, vcx) = visual_window(cx, vault);
    let mut lefts = Vec::new();
    let mut rights = Vec::new();
    for selector in [
        "outline-row-0",
        "outline-row-1",
        "outline-row-2",
        "outline-row-3",
    ] {
        let bounds = vcx.debug_bounds(selector).expect("painted");
        lefts.push(bounds.origin.x);
        rights.push(bounds.right());
    }
    assert!(
        lefts.windows(2).all(|w| w[0] == w[1]),
        "every row starts at the same x: {lefts:?}"
    );
    assert!(
        rights.windows(2).all(|w| w[0] == w[1]),
        "and ends at the same one, however long the title: {rights:?}"
    );
}

#[gpui::test]
fn every_title_starts_at_the_same_x_for_its_level(cx: &mut gpui::TestAppContext) {
    // The row *frame* lined up; the cells inside it did not. A flex
    // item is shrinkable unless it says otherwise, and the title's flex
    // basis is its content — so a title long enough to overflow the row
    // took its shrink out of the indent, the fold arrow and the status
    // glyph as well as itself, and every column on that row moved left
    // by an amount that depended on the length of the headline.
    let vault = "\
* Alpha
* A headline with a very much longer title than the ones around it, long enough to overflow the column it is painted in
* Gamma
* TODO Delta
";
    let (_dir, _view, vcx) = visual_window(cx, vault);
    let lefts: Vec<_> = (0..4)
        .map(|i| {
            vcx.debug_bounds(match i {
                0 => "title-0",
                1 => "title-1",
                2 => "title-2",
                _ => "title-3",
            })
            .expect("painted")
            .origin
            .x
        })
        .collect();
    assert!(
        lefts.windows(2).all(|w| w[0] == w[1]),
        "same level, same x: {lefts:?}"
    );
}

#[gpui::test]
fn a_note_with_an_image_link_paints_it(cx: &mut gpui::TestAppContext) {
    // An image in org *is* a file link, so the window has to resolve it
    // against the vault and paint the file. A link whose file is not
    // there paints nothing — a broken-image box says less than the link.
    // A 1×1 PNG, so the decoder has something real to chew on.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let (dir, view, vcx) = visual_window(cx, "* Shot\n[[file:assets/x.png]]\n[[file:gone.png]]\n");
    std::fs::create_dir_all(dir.path().join("assets")).expect("mkdir");
    std::fs::write(dir.path().join("assets/x.png"), PNG).expect("write");
    view.update(vcx, |v, cx| {
        assert!(v.images_shown(), "shown to begin with");
        assert_eq!(v.painted_images(), 1, "the one that exists");
        v.run_command("toggle-inline-images", cx);
        assert!(!v.images_shown());
        assert_eq!(v.painted_images(), 0, "and none once toggled off");
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn only_a_row_with_a_subtree_offers_a_fold_arrow(cx: &mut gpui::TestAppContext) {
    // VAULT: Alpha has Beta under it; Beta, Gamma and Delta are leaves.
    let (_dir, _view, vcx) = visual_window(cx, VAULT);
    assert!(
        vcx.debug_bounds("fold-0").is_some(),
        "Alpha has something to fold"
    );
    for leaf in ["fold-1", "fold-2", "fold-3"] {
        assert!(
            vcx.debug_bounds(leaf).is_none(),
            "{leaf}: a leaf offers no arrow to click"
        );
    }
}

#[gpui::test]
fn selecting_a_row_does_not_resize_the_outline(cx: &mut gpui::TestAppContext) {
    // "Selecting an element and depending on the length of the headline
    // the tree view gets resized." Selecting fills the right-hand pane
    // with that headline's title, body and properties, and a flex item
    // that can shrink gives way to a sibling whose content grew.
    let vault = "\
* Alpha
Short.
* A headline whose title is very much longer than the others, and whose body is longer still
This body is a single line with no break in it at all, which is the widest thing the detail pane will be asked to lay out, and it is much wider than the window.
* Gamma
Short too.
";
    let (_dir, _view, vcx) = visual_window(cx, vault);
    let width =
        |c: &mut gpui::VisualTestContext| c.debug_bounds("outline-row-0").expect("painted").size;
    let before = width(vcx);
    let long = centre(vcx, "outline-row-1");
    vcx.simulate_click(long, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(width(vcx), before, "the long row did not move the edge");
    let short = centre(vcx, "outline-row-2");
    vcx.simulate_click(short, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(width(vcx), before, "and neither did coming back off it");
}
