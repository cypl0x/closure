//! "An expansion cycle must fail with the chain that caused it, not a
//! stack overflow and not a silent stop."
//!
//! Cycles were already caught, and the message named one widget: the
//! one the expander happened to be standing on when it noticed. In a
//! vault where `page` uses `panel` uses `header` uses `page`, being
//! told "widget cycle through `header`" leaves the reader to find the
//! other two. The chain is the whole answer, so the error carries it.
//!
//! Depth is the other half, and it is not the same thing. A widget that
//! nests a thousand deep without ever repeating a name is not a cycle,
//! and recursing through it would end the process rather than the
//! expansion. I5 says a kernel crate returns `Err` and never panics —
//! and blowing the stack is worse than a panic, because it takes the
//! window with it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;

use closure_query::{WidgetError, expand_widgets};

/// A chain of `n` widgets, each referring to the next; the last refers
/// to `tail`.
fn chain(n: usize, tail: &str) -> String {
    let mut src = String::new();
    for i in 0..n {
        let next = if i + 1 == n {
            tail.to_owned()
        } else {
            format!("w{}", i + 1)
        };
        let _ = write!(
            src,
            "#+BEGIN: closure-widget :name w{i}\n{{{{{next}}}}}\n#+END:\n"
        );
    }
    src
}

#[test]
fn a_widget_that_refers_to_itself_names_itself() {
    let src = "#+BEGIN: closure-widget :name loop\n{{loop}}\n#+END:\n";
    let WidgetError::Cycle(path) = expand_widgets(src).unwrap_err() else {
        panic!("not reported as a cycle");
    };
    assert_eq!(path, vec!["loop".to_owned(), "loop".to_owned()]);
}

#[test]
fn an_indirect_cycle_names_the_whole_ring_in_order() {
    let src = "#+BEGIN: closure-widget :name page\n{{panel}}\n#+END:\n\
               #+BEGIN: closure-widget :name panel\n{{header}}\n#+END:\n\
               #+BEGIN: closure-widget :name header\n{{page}}\n#+END:\n";
    let WidgetError::Cycle(path) = expand_widgets(src).unwrap_err() else {
        panic!("not reported as a cycle");
    };
    // Whichever block the expander reached first, the ring is closed
    // and every name in it is named, in the order they call each other.
    assert_eq!(path.len(), 4, "{path:?}");
    assert_eq!(path.first(), path.last(), "{path:?} is not a ring");
    for name in ["page", "panel", "header"] {
        assert!(path.iter().any(|n| n == name), "{name} missing: {path:?}");
    }
}

#[test]
fn the_message_reads_as_a_chain() {
    let src = "#+BEGIN: closure-widget :name a\n{{b}}\n#+END:\n\
               #+BEGIN: closure-widget :name b\n{{a}}\n#+END:\n";
    let text = expand_widgets(src).unwrap_err().to_string();
    assert!(text.contains(" -> "), "not a chain: {text}");
    assert!(text.contains('a') && text.contains('b'), "{text}");
}

#[test]
fn nesting_that_is_deep_but_not_circular_still_works() {
    // The limit exists to stop a runaway, not to make composition
    // shallow. Thirty-two deep is more than any page needs and well
    // inside it.
    let src = format!(
        "{}#+BEGIN: closure-widget :name end\nbottom\n#+END:\n",
        chain(32, "end")
    );
    let out = expand_widgets(&src).unwrap();
    assert!(out.contains("bottom"), "{out}");
}

#[test]
fn nesting_past_the_limit_is_an_error_and_not_a_crash() {
    // No cycle here: every name is different. Recursing it would end
    // the process, which I5 forbids more strongly than it forbids a
    // panic — a blown stack takes the window with it.
    let src = format!(
        "{}#+BEGIN: closure-widget :name end\nbottom\n#+END:\n",
        chain(5_000, "end")
    );
    let WidgetError::TooDeep { limit, path } = expand_widgets(&src).unwrap_err() else {
        panic!("a five thousand deep nest was not reported as too deep");
    };
    assert!(limit > 0);
    assert_eq!(path.len(), limit, "the path should be what it got through");
}

#[test]
fn a_call_block_that_calls_itself_is_caught_too() {
    let src = "#+BEGIN: closure-widget :name panel\n{{slot}}{{panel}}\n#+END:\n\
               #+BEGIN: closure-widget :call panel\ninside\n#+END:\n";
    assert!(matches!(
        expand_widgets(src),
        Err(WidgetError::Cycle(_) | WidgetError::TooDeep { .. })
    ));
}
