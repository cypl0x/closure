//! "moving \"through\" level 1 (*) headline causes microfreezing … when
//! I try to pass \"through\" a level 1 headline (*) the interface
//! usually freezes or get lags … it usually freezes for a 1s or
//! something … When navigating through subheadings and subsubheadings
//! works smoothly."
//!
//! The report describes exactly where the cost is. A subheading's
//! subtree is small; a level-1 headline's subtree is the whole
//! section. The detail preview concatenated the body with *every* line
//! under it and laid out one shaped text run per line — so selecting a
//! top-level headline asked the window to shape a few hundred lines,
//! every frame, for a pane you are only glancing at.
//!
//! Neither the read nor the highlighting is the problem; both were
//! measured at well under a millisecond on a 600KB vault. Text layout
//! is, and the only fix that scales is not to lay out what nobody can
//! see.
//!
//! So a preview is a preview: bounded, with an honest note about what
//! it is not showing. The editor is where a subtree is read in full,
//! and it virtualizes its own lines already.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::Detail;
use closure_shell_gpui::{PREVIEW_LINES, preview_hidden, preview_text};

/// A detail whose subtree is `n` lines long.
fn detail_with(n: usize) -> Detail {
    use std::fmt::Write as _;
    let children = (0..n).fold(String::new(), |mut s, i| {
        let _ = writeln!(s, "line {i}");
        s
    });
    Detail {
        id: "01HQPREV0000000000000001".to_owned(),
        title: "Top".to_owned(),
        body: "the body itself\n".to_owned(),
        children,
        ..Detail::default()
    }
}

#[test]
fn a_small_subtree_is_shown_whole() {
    // Nothing changes for the case that was already smooth.
    let d = detail_with(10);
    let text = preview_text(&d);
    assert!(text.contains("the body itself"));
    assert!(text.contains("line 9"), "all ten lines: {text}");
    assert_eq!(preview_hidden(&d), 0);
}

#[test]
fn an_enormous_subtree_is_bounded() {
    // The level-1 case. Whatever the section holds, the pane lays out
    // a fixed number of lines — that is what makes the cost of
    // selecting a headline independent of what is under it.
    let d = detail_with(5_000);
    let lines = preview_text(&d).lines().count();
    assert!(
        lines <= PREVIEW_LINES,
        "{lines} lines laid out for a preview capped at {PREVIEW_LINES}"
    );
}

#[test]
fn the_cost_does_not_grow_with_the_subtree() {
    // The property the report is really about: passing *through* a
    // level-1 headline must not cost more than passing through a
    // subheading. Two subtrees three orders of magnitude apart have to
    // paint the same amount.
    let small = preview_text(&detail_with(PREVIEW_LINES * 2))
        .lines()
        .count();
    let huge = preview_text(&detail_with(PREVIEW_LINES * 200))
        .lines()
        .count();
    assert_eq!(small, huge, "a bigger section still paints the same");
}

#[test]
fn it_says_how_much_it_is_not_showing() {
    // Silently truncating a note is worse than the lag: it would look
    // like the subtree ends there.
    let d = detail_with(5_000);
    let hidden = preview_hidden(&d);
    assert!(hidden > 4_000, "{hidden} hidden of 5000");
    assert_eq!(
        hidden,
        5_000 + 1 - PREVIEW_LINES,
        "the body line counts too"
    );
}

#[test]
fn the_body_itself_is_never_the_thing_that_gets_cut() {
    // A note's own body comes first and matters most; it is the
    // children that make a level-1 headline enormous. Even at the cap,
    // what you wrote *here* is what you see.
    let d = detail_with(5_000);
    let text = preview_text(&d);
    assert!(
        text.starts_with("the body itself"),
        "the body leads: {}",
        &text[..40.min(text.len())]
    );
}
