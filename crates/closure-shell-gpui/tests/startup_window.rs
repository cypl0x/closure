//! A file that opens `#+STARTUP: overview` opens folded in the window.
//!
//! `startup_of` read the directive and the core knew what to do with
//! it, and the real window still showed every child row, because the
//! only thing that ever called `apply_startup` was the test for
//! `apply_startup`. A feature wired to nothing but its own test is the
//! shape of green that means least, and the only reason this was caught
//! is that somebody opened the window and looked at it.
//!
//! So the test lives here rather than beside the core: the question is
//! not "does the core know how to fold" — it did — but "does opening a
//! vault fold it", and the window is where opening a vault happens.
//!
//! It counts painted rows rather than reading titles, because a painted
//! row is what the reader actually got. Absence is only sound to assert
//! about a row index that was never painted at all, which is true of a
//! window on its opening frame and of no window after that.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

/// Two top-level headlines, four rows in all. With `overview` the
/// window must paint two; without it, four.
const FOLDED: &str = "\
#+STARTUP: overview
* Project closure
:PROPERTIES:
:ID: 01STARTUPWIN0000000001
:END:
** Kernel
:PROPERTIES:
:ID: 01STARTUPWIN0000000002
:END:
** GPUI shell
:PROPERTIES:
:ID: 01STARTUPWIN0000000003
:END:
* Reading
:PROPERTIES:
:ID: 01STARTUPWIN0000000004
:END:
";

/// The same vault with the directive taken out, so the tests below
/// differ in exactly the thing under test.
const OPEN: &str = "\
* Project closure
:PROPERTIES:
:ID: 01STARTUPWIN0000000001
:END:
** Kernel
:PROPERTIES:
:ID: 01STARTUPWIN0000000002
:END:
** GPUI shell
:PROPERTIES:
:ID: 01STARTUPWIN0000000003
:END:
* Reading
:PROPERTIES:
:ID: 01STARTUPWIN0000000004
:END:
";

#[gpui::test]
fn the_children_are_not_painted_on_the_opening_frame(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, FOLDED);
    assert!(
        vcx.debug_bounds("outline-row-0").is_some(),
        "nothing was painted at all"
    );
    assert!(
        vcx.debug_bounds("outline-row-1").is_some(),
        "the second top-level headline should still be there"
    );
    assert!(
        vcx.debug_bounds("outline-row-2").is_none(),
        "the file asked for `overview` and the window painted its children anyway"
    );
}

#[gpui::test]
fn a_file_that_says_nothing_still_opens_open(cx: &mut gpui::TestAppContext) {
    // The control. If this folded too, the feature would be "closure
    // folds everything" wearing a directive's name.
    let (_dir, _view, vcx) = visual_window(cx, OPEN);
    assert!(
        vcx.debug_bounds("outline-row-3").is_some(),
        "a file that asked for nothing came up folded"
    );
}

#[gpui::test]
fn the_fold_can_be_opened_like_any_other(cx: &mut gpui::TestAppContext) {
    // A startup fold is a starting position, not a lock. If `overview`
    // produced rows the reader could not open, the directive would have
    // made the file less readable rather than more.
    let (_dir, view, vcx) = visual_window(cx, FOLDED);
    view.update(vcx, |v, cx| v.run_command("toggle-fold", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("outline-row-2").is_some(),
        "the reader unfolded the headline and its children stayed hidden"
    );
}
