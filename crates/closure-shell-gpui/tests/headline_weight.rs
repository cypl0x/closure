//! "Use different font size, font variant (Maple (Mono) NF (italic) for
//! example) … Examples: Mape Mono NF Bold italic for some level of
//! headline. Do research which Maple Font variants are available and
//! think about it where to use which variant in which size"
//!
//! Every headline was bold, at every depth. Colour cycled by level and
//! weight did not, so a level-5 heading shouted exactly as loudly as
//! the level-1 above it — and in a file with five levels in it, that is
//! five things all claiming to be the most important.
//!
//! The faces the shell now ships are Regular, `SemiBold`, Bold and the
//! two italics, so depth can be spelled in weight the way an outline
//! spells it in colour: the top two levels carry the document, the
//! middle ones are structure, and the deep ones are detail — italic,
//! which is what org's own faces do with `org-level-5` and below in
//! most themes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{BodySpan, span_decoration};

#[test]
fn the_top_levels_are_heavier_than_the_deep_ones() {
    // The complaint: five levels, one weight.
    assert!(span_decoration(BodySpan::Headline(1)).bold);
    assert!(!span_decoration(BodySpan::Headline(5)).bold);
}

#[test]
fn weight_never_increases_with_depth() {
    // Whatever the exact scheme, a deeper heading must never look more
    // important than the one it sits under.
    let mut was_bold = true;
    for level in 1..=8u8 {
        let bold = span_decoration(BodySpan::Headline(level)).bold;
        assert!(!bold || was_bold, "level {level} got heavier");
        was_bold = bold;
    }
}

#[test]
fn the_deep_levels_lean() {
    // Detail rather than structure, and the italic face is bundled for
    // exactly this.
    assert!(span_decoration(BodySpan::Headline(5)).italic);
    assert!(!span_decoration(BodySpan::Headline(1)).italic);
}

#[test]
fn a_headline_is_never_plain() {
    // It must stay distinguishable from the prose under it at every
    // depth — by weight, by slant, or by the colour it already had.
    for level in 1..=8u8 {
        let d = span_decoration(BodySpan::Headline(level));
        assert!(d.bold || d.italic, "level {level} reads as prose");
    }
}

#[test]
fn the_keywords_keep_their_weight() {
    // TODO and DONE are the same weight at every level, because their
    // job is to be found rather than to say how deep they are.
    assert!(span_decoration(BodySpan::Todo).bold);
    assert!(span_decoration(BodySpan::Done).bold);
}
