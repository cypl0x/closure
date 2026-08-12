//! The Jobs and Bridges panes paint what they are for.
//!
//! Five of the seventeen affordances no test looked at: `jobs-pane`,
//! `jobs-empty`, `job-row-{}`, `bridges-pane` and `bridge-row-{}`.
//!
//! The empty state matters as much as the full one here, and there is
//! precedent: `job_rows` once read each whole document as a cron
//! listing, the parse failed on the first `* Headline`, and `.ok()`
//! dropped every job — so the pane was empty in any vault with a
//! headline in it, which is all of them. Nothing caught it because
//! nothing asserted that a vault *with* a job paints a row.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

/// A vault with two scheduled jobs, written the way the cron pane
/// reads them.
const WITH_JOBS: &str = "\
* Scheduled work
:PROPERTIES:
:ID: 01JOBSPANE0000000001
:END:
#+BEGIN_SRC closure-cron
0 9 * * * agenda
0 * * * * sync
#+END_SRC
";

const NO_JOBS: &str = "\
* Just a note
:PROPERTIES:
:ID: 01JOBSPANE0000000002
:END:
nothing scheduled here
";

#[gpui::test]
fn a_vault_with_jobs_paints_a_row_for_each(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, WITH_JOBS);
    view.update(vcx, |v, cx| v.run_command("cron", cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("jobs-pane").is_some(), "no jobs pane");
    assert!(
        vcx.debug_bounds("job-row-0").is_some(),
        "first job unpainted"
    );
    assert!(
        vcx.debug_bounds("job-row-1").is_some(),
        "second job unpainted"
    );
}

#[gpui::test]
fn a_vault_with_no_jobs_says_so_rather_than_showing_an_empty_pane(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, NO_JOBS);
    view.update(vcx, |v, cx| v.run_command("cron", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("jobs-empty").is_some(),
        "an empty schedule showed nothing at all"
    );
}

#[gpui::test]
fn the_empty_state_is_not_shown_when_there_are_jobs(cx: &mut gpui::TestAppContext) {
    // The pairing that makes both assertions mean something. Painting
    // the empty state *and* the rows would satisfy either test alone.
    let (_dir, view, vcx) = visual_window(cx, WITH_JOBS);
    view.update(vcx, |v, cx| v.run_command("cron", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("jobs-empty").is_none(),
        "said there was nothing scheduled beside two scheduled jobs"
    );
}

#[gpui::test]
fn the_bridges_pane_paints(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, NO_JOBS);
    view.update(vcx, |v, cx| v.run_command("bridges", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("bridges-pane").is_some(),
        "no bridges pane"
    );
}

#[gpui::test]
fn every_bridge_gets_a_row(cx: &mut gpui::TestAppContext) {
    // The bridges are the built-in protocol surfaces (LSP, MCP, ACP,
    // A2A), so a vault with nothing in it still has them — an empty
    // bridges pane would mean the list itself is gone.
    let (_dir, view, vcx) = visual_window(cx, NO_JOBS);
    view.update(vcx, |v, cx| v.run_command("bridges", cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("bridge-row-0").is_some(),
        "the bridges pane lists no bridges"
    );
}
