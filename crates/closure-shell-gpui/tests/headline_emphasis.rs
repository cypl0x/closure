//! "/emphasis/ *bold* ~strikethrough~ or anything format wise in the
//! header? … This is from my Doom Emacs config and these are ordinary
//! org-mode headlines. Look how pretty they are. Can't we get this to
//! work in closure as well?"
//!
//! The body editor has painted org emphasis from the start — italic,
//! bold, strike, underline, verbatim, code, each with its own face.
//! The outline's titles never asked: they were painted as one flat run
//! in the level's colour, so a headline called `/italic/ headline`
//! showed the slashes and nothing else.
//!
//! Nothing new is needed for it. The classifier already returns the
//! right spans for a bare title; the row painter simply threw them
//! away. Same spans, same faces, same code path as the buffer — which
//! is what makes a note look like itself in the tree and in the text.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{BodySpan, title_runs};

/// The kinds a title's runs carry, in order, ignoring plain stretches.
fn kinds(title: &str) -> Vec<BodySpan> {
    title_runs(title)
        .into_iter()
        .map(|(_, kind)| kind)
        .filter(|k| *k != BodySpan::Plain)
        .collect()
}

#[test]
fn an_italic_headline_is_italic() {
    assert_eq!(kinds("/italic/ headline"), vec![BodySpan::Italic]);
}

#[test]
fn a_bold_headline_is_bold() {
    assert_eq!(kinds("*bold* headline"), vec![BodySpan::Bold]);
}

#[test]
fn strike_and_underline_too() {
    assert_eq!(
        kinds("+strike+ and _underline_"),
        vec![BodySpan::Strike, BodySpan::Underline]
    );
}

#[test]
fn a_plain_headline_is_one_plain_run() {
    let runs = title_runs("just an ordinary headline");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1, BodySpan::Plain);
}

#[test]
fn the_runs_cover_the_whole_title_exactly_once() {
    // A painter walks these to slice the string; a gap or an overlap
    // is a dropped or doubled piece of the title.
    let title = "/italic/ then *bold* then plain";
    let runs = title_runs(title);
    let mut at = 0usize;
    for (range, _) in &runs {
        assert_eq!(range.start, at, "gap or overlap in {runs:?}");
        at = range.end;
    }
    assert_eq!(at, title.len(), "runs stop short of the title");
}

#[test]
fn a_title_that_is_not_markup_is_left_alone() {
    // `2 * 3 = 6` is arithmetic, not bold-then-code. org requires the
    // markers to hug non-whitespace, which is what keeps a headline
    // full of punctuation from turning into a ransom note.
    assert_eq!(kinds("2 * 3 = 6"), Vec::<BodySpan>::new());
}
