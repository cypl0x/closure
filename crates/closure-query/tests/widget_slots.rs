//! "Content passed *into* a widget body, so a widget can wrap what it
//! is given rather than only emit what it knows."
//!
//! Arguments are values on one line. A slot is a block: the rows of a
//! table, the paragraphs of a callout, the body of a page. A widget
//! that cannot take one can only ever be a template with holes in it,
//! and every layout that wraps something — a card, a panel, a
//! two-column split — needs to take one.
//!
//! `{{…}}` has nowhere to put a block, so the call site that carries
//! content is a block itself:
//!
//! ```org
//! #+BEGIN: closure-widget :call panel :with title=Notes
//! whatever goes inside
//! #+END:
//! ```
//!
//! `:name` defines, `:call` invokes — one keyword apart, and both are
//! ordinary org dynamic blocks, so a file full of them still opens in
//! Emacs. Inside the widget, the content arrives as `{{slot}}`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::expand_widgets;

/// A `panel` widget that wraps whatever it is given.
const PANEL: &str = "#+BEGIN: closure-widget :name panel :inputs title\n\
                     == {{title}} ==\n{{slot}}\n== end ==\n#+END:\n";

#[test]
fn a_call_blocks_content_lands_in_the_slot() {
    let src = format!(
        "{PANEL}#+BEGIN: closure-widget :call panel :with title=Notes\nthe inside\n#+END:\n"
    );
    let out = expand_widgets(&src).unwrap();
    assert!(out.contains("== Notes =="), "{out}");
    assert!(out.contains("the inside"), "{out}");
    assert!(out.contains("== end =="), "{out}");
}

#[test]
fn the_slot_keeps_more_than_one_line() {
    let src = format!(
        "{PANEL}#+BEGIN: closure-widget :call panel :with title=Notes\nfirst\nsecond\n#+END:\n"
    );
    let out = expand_widgets(&src).unwrap();
    let inside = out.split("== Notes ==").nth(1).unwrap_or_default();
    assert!(inside.contains("first"), "{out}");
    assert!(inside.contains("second"), "{out}");
}

#[test]
fn a_slot_may_itself_compose() {
    // The content is the caller's, so it is expanded in the caller's
    // scope before the widget ever sees it.
    let src = format!(
        "#+BEGIN: closure-widget :name badge\n[new]\n#+END:\n\
         {PANEL}#+BEGIN: closure-widget :call panel :with title=Notes\n{{{{badge}}}}\n#+END:\n"
    );
    let out = expand_widgets(&src).unwrap();
    assert!(out.contains("[new]"), "{out}");
}

#[test]
fn a_widget_with_no_slot_simply_ignores_the_content() {
    // A slot is optional, like an argument nobody read.
    let src = "#+BEGIN: closure-widget :name plain\njust this\n#+END:\n\
               #+BEGIN: closure-widget :call plain\nignored\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("just this"), "{out}");
}

#[test]
fn a_call_to_a_widget_that_does_not_exist_is_an_error() {
    let src = "#+BEGIN: closure-widget :call nosuch\ncontent\n#+END:\n";
    assert!(expand_widgets(src).is_err());
}

#[test]
fn the_call_sites_own_lines_survive_expansion() {
    // I12: the BEGIN and END lines are the file's, and the file is not
    // rewritten by being read.
    let src = format!(
        "{PANEL}#+BEGIN: closure-widget :call panel :with title=Notes\nthe inside\n#+END:\n"
    );
    let out = expand_widgets(&src).unwrap();
    assert!(
        out.contains("#+BEGIN: closure-widget :call panel :with title=Notes"),
        "the call site's own line was rewritten: {out}"
    );
    assert!(out.contains("#+END:"), "{out}");
}

#[test]
fn a_typed_argument_still_applies_on_a_call_block() {
    let src = "#+BEGIN: closure-widget :name box :inputs count:number\n[{{count}}] {{slot}}\n#+END:\n\
               #+BEGIN: closure-widget :call box :with count=banana\nx\n#+END:\n";
    assert!(expand_widgets(src).is_err(), "the type was not checked");
}
