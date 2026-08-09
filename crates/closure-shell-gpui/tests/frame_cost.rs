//! "When holding j or k and going through all of the headlines quickly
//! it sometimes freezes for some milliseconds."
//!
//! `tests/traversal_cost.rs` in the kernel holds the *derivation* to a
//! cost that does not grow with the subtree. This holds the other half:
//! the frame. A window that derives its rows once and then copies the
//! whole list to read `.len()` off it has put the size of the vault
//! into every repaint, and a repaint is what a keystroke causes.
//!
//! Measured as a shape rather than a clock — four times the vault must
//! not cost four times as much to draw, on any machine. Sixteen times
//! the vault measured 751ms -> 816ms here, so this is a guard on a
//! property the window has rather than a bug being fixed: the row list
//! is derived once per revision and every pane reads it shared.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use closure_shell_gpui::visual_window;

/// `n` headlines under one chapter, as one org file.
fn vault_of(n: usize) -> String {
    (0..n).fold(
        String::from("* Chapter\n:PROPERTIES:\n:ID: 01HQFRAME000000000000000A\n:END:\n"),
        |mut s, i| {
            let _ = write!(
                s,
                "** Section {i}\n:PROPERTIES:\n:ID: 01HQFRAME{i:017}\n:END:\n\
                 a paragraph of body text under section {i}\n"
            );
            s
        },
    )
}

/// What twenty keystrokes cost, frames included.
fn hold_j(cx: &mut gpui::TestAppContext, n: usize) -> Duration {
    let (_dir, _view, vcx) = visual_window(cx, &vault_of(n));
    // Warm: the first frame of a window is not what the report is about.
    vcx.simulate_keystrokes("j");
    vcx.run_until_parked();
    let t = Instant::now();
    for _ in 0..20 {
        vcx.simulate_keystrokes("j");
        vcx.run_until_parked();
    }
    t.elapsed()
}

#[allow(clippy::cast_precision_loss)]
fn ratio(small: Duration, large: Duration) -> f64 {
    if small.as_nanos() == 0 {
        return 0.0;
    }
    large.as_nanos() as f64 / small.as_nanos() as f64
}

#[gpui::test]
fn a_keystroke_does_not_cost_the_whole_vault(cx: &mut gpui::TestAppContext) {
    let a = hold_j(cx, 250);
    let b = hold_j(cx, 1_000);
    let g = ratio(a, b);
    assert!(
        g < 2.0,
        "four times the vault cost {g:.1} times as much to hold `j` in \
         ({a:?} -> {b:?}) — a frame is copying the whole row list"
    );
}
