//! `#+STARTUP:` — how a document asks to be met.
//!
//! `overview` opens folded to top-level headlines, `content` to all
//! headlines, `showall` to everything. Ignored, so every file opened
//! the same way whatever it asked for. This queue file has carried
//! `#+STARTUP: overview` since the day it was written and closure has
//! never once honoured it.
//!
//! Reading it is here. What a shell does with a fold state is the
//! shell's, and there is already `:VISIBILITY:` machinery for that —
//! this says what the *file* asked for, which is a different question
//! from what the reader last left it as.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Startup, startup_of};

#[test]
fn each_fold_option_is_read() {
    assert_eq!(startup_of("#+STARTUP: overview\n"), Some(Startup::Overview));
    assert_eq!(startup_of("#+STARTUP: content\n"), Some(Startup::Content));
    assert_eq!(startup_of("#+STARTUP: showall\n"), Some(Startup::ShowAll));
    assert_eq!(
        startup_of("#+STARTUP: showeverything\n"),
        Some(Startup::ShowAll)
    );
}

#[test]
fn the_keyword_may_be_lower_case() {
    assert_eq!(startup_of("#+startup: overview\n"), Some(Startup::Overview));
}

#[test]
fn other_options_on_the_line_are_ignored_not_refused() {
    // `#+STARTUP: overview indent logdone` is ordinary. Reading the
    // fold option and passing over the rest is better than refusing a
    // line closure only partly understands.
    assert_eq!(
        startup_of("#+STARTUP: indent overview logdone\n"),
        Some(Startup::Overview)
    );
}

#[test]
fn a_file_that_says_nothing_asks_for_nothing() {
    // Distinguishable from `showall`: a file with no preference should
    // get whatever the reader's default is, not have one imposed.
    assert_eq!(startup_of("* A headline\nbody\n"), None);
    assert_eq!(startup_of("#+STARTUP: indent\n"), None);
}

#[test]
fn only_the_first_one_counts() {
    // Org takes the first; two directives disagreeing is a mistake in
    // the file, and picking the last would make it depend on where
    // somebody appended.
    assert_eq!(
        startup_of("#+STARTUP: overview\n#+STARTUP: showall\n"),
        Some(Startup::Overview)
    );
}

#[test]
fn a_startup_inside_a_block_is_not_a_directive() {
    // An example block showing somebody how to write one is not a
    // request to fold this file.
    let src = "#+BEGIN_EXAMPLE\n#+STARTUP: showall\n#+END_EXAMPLE\n";
    assert_eq!(startup_of(src), None);
}
