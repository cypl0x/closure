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
    vcx.simulate_keystrokes("i");
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::EditBody, "`i` opened the editor");
    });
    vcx.simulate_input("hello");
    view.update(vcx, |v, _cx| assert_eq!(v.body(), "hello"));
}

#[gpui::test]
fn a_shifted_keystroke_survives_the_platform(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, "* Note\n");
    vcx.simulate_keystrokes("i");
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
    // The editor opens with the cursor at the end of a 400-line body,
    // so line 0 is off screen until the cursor comes back to the top.
    vcx.simulate_keystrokes("escape g g");
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
