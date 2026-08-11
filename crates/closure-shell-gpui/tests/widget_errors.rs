//! "A widget that fails to expand currently contributes nothing. The
//! pane must say what failed and why, in place."
//!
//! It now says it, and says it as prose in the middle of the preview,
//! where it reads as part of the note rather than as a report about
//! the note. A composition that fails is not content — it is the one
//! line in the pane that is about the document rather than in it, and
//! it should look like it.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const BROKEN: &str = "\
* Home
:PROPERTIES:
:ID: 01HQWERR000000000000001
:END:
#+BEGIN: closure-widget :name board
{{nosuchwidget}}
#+END:
";

const FINE: &str = "\
* Home
:PROPERTIES:
:ID: 01HQWERR000000000000002
:END:
#+BEGIN: closure-widget :name card
just text
#+END:
";

#[gpui::test]
fn a_failed_composition_is_marked_as_a_failure(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, BROKEN);
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("widget-error").is_some(),
        "the pane shows the failure as ordinary prose, or not at all"
    );
}

#[gpui::test]
fn a_composition_that_works_is_not_marked(cx: &mut gpui::TestAppContext) {
    let (_dir, _view, vcx) = visual_window(cx, FINE);
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("widget-error").is_none(),
        "a working composition was reported as broken"
    );
}
