//! "make TODO color (and font and weight and EVERYTHING) consistent",
//! "the TODO and DONE keyword shall be colored, themed, font,
//! consistently throughout the UI — In the prompt TODO is just white
//! text", and "(shift+c aka C) should color the word TODO just in the
//! same color as TODO is in the headline tree view".
//!
//! Three items, one cause: three places decided independently what a
//! keyword *is*.
//!
//! - `todo_glyph` called `DONE`, `CANCELLED` and `KILL` finished.
//! - the outline row's own match called the same three finished.
//! - the body highlighter called only `DONE` finished, so `CANCELLED`
//!   was a green filled dot in the tree and an alarm-red word in the
//!   buffer, on the same headline.
//!
//! One predicate, in the core, where the parser and every shell can
//! reach it. Which keywords mean finished is a property of org and of
//! the user's `todo_keywords`, not of whichever painter is running.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::keyword_is_done;

#[test]
fn org_s_own_finished_keywords_are_finished() {
    for k in ["DONE", "CANCELLED", "KILL"] {
        assert!(keyword_is_done(k), "{k}");
    }
}

#[test]
fn the_unfinished_ones_are_not() {
    for k in ["TODO", "NEXT", "WAIT", "HOLD", "PROJ", "STRT"] {
        assert!(!keyword_is_done(k), "{k}");
    }
}

#[test]
fn a_keyword_nobody_anticipated_is_unfinished() {
    // The safe way round: an unknown keyword shown as open is a task
    // you look at again; shown as done it is a task you lose.
    assert!(!keyword_is_done("FROBNICATE"));
    assert!(!keyword_is_done(""));
}

#[test]
fn the_glyph_agrees_with_the_predicate() {
    // They were two lists that happened to match; the glyph is derived
    // from the predicate now, so they cannot drift apart.
    for k in ["DONE", "CANCELLED", "KILL", "TODO", "NEXT", "WAIT", "ZZZ"] {
        let filled = closure_shell_core::todo_glyph_for(k) == "●";
        assert_eq!(filled, keyword_is_done(k), "{k}");
    }
}

#[test]
fn a_prompt_can_find_the_keyword_it_is_typing() {
    // "In the prompt TODO is just white text" — a field cannot colour
    // what it cannot locate, and a headline being typed has no stars
    // in front of it yet.
    use closure_shell_core::leading_keyword;
    assert_eq!(leading_keyword("TODO buy milk"), Some((0, 4)));
    assert_eq!(leading_keyword("DONE it"), Some((0, 4)));
    assert_eq!(leading_keyword("CANCELLED later"), Some((0, 9)));
}

#[test]
fn a_word_that_merely_starts_like_one_is_not_a_keyword() {
    use closure_shell_core::leading_keyword;
    assert_eq!(leading_keyword("TODOS are plural"), None);
    assert_eq!(leading_keyword("todo lowercase"), None);
    assert_eq!(leading_keyword("buy milk TODO"), None, "only leading");
    assert_eq!(leading_keyword(""), None);
    assert_eq!(leading_keyword("TODO"), Some((0, 4)), "on its own");
}
