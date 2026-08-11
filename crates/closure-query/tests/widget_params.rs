//! "`{{name}}` substitution is `#define`, not composition. A call site
//! must be able to pass values."
//!
//! A widget could only ever emit what it already knew. Two pages that
//! wanted the same banner with a different title needed two widgets,
//! which is the copy-paste the whole idea exists to remove — and it is
//! why the vision's first item has been at a fraction of what it asks
//! for.
//!
//! So a widget declares what it takes — `:inputs who` on the BEGIN
//! line — and a reference may carry arguments: `{{greet who=world}}`,
//! or `{{greet who="the whole world"}}` when the value has spaces in
//! it. Inside the body they are read with the same `{{who}}` a widget
//! reference uses, and an argument shadows a widget of the same name.
//! Locals beat globals — the rule every language with both already
//! uses, and the one that lets a widget be written without knowing
//! every name in the vault.
//!
//! The declaration is not ceremony. A block is its own call site with
//! no arguments, so `{{who}}` in an undeclared widget is indistinguishable
//! from a reference to a widget called `who` that nobody defined —
//! and that has to stay an error. Declaring it is what makes it a
//! parameter, and an unbound parameter expands to nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::{WidgetError, expand_widgets};

/// A document defining `greet` and calling it once.
fn doc(def: &str, call: &str) -> String {
    format!(
        "#+BEGIN: closure-widget :name greet :inputs who\n{def}\n#+END:\n\
         #+BEGIN: closure-widget :name page\n{call}\n#+END:\n"
    )
}

#[test]
fn a_call_site_can_pass_a_value() {
    let out = expand_widgets(&doc("hello {{who}}", "{{greet who=world}}")).unwrap();
    assert!(out.contains("hello world"), "{out}");
}

#[test]
fn a_value_may_contain_spaces_if_it_is_quoted() {
    let out = expand_widgets(&doc("hello {{who}}", "{{greet who=\"the whole world\"}}")).unwrap();
    assert!(out.contains("hello the whole world"), "{out}");
}

#[test]
fn two_call_sites_get_their_own_values() {
    // The point of the feature: one widget, two pages.
    let src = "#+BEGIN: closure-widget :name greet :inputs who\nhello {{who}}\n#+END:\n\
               #+BEGIN: closure-widget :name a\n{{greet who=one}}\n#+END:\n\
               #+BEGIN: closure-widget :name b\n{{greet who=two}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("hello one"), "{out}");
    assert!(out.contains("hello two"), "{out}");
}

#[test]
fn several_arguments_at_once() {
    let src = "#+BEGIN: closure-widget :name greet :inputs greeting,who\n\
               {{greeting}}, {{who}}!\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{greet greeting=Hi who=\"you there\"}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("Hi, you there!"), "{out}");
}

#[test]
fn an_argument_shadows_a_widget_of_the_same_name() {
    // Locals beat globals, so a widget can be written without knowing
    // every name in the vault.
    let src = "#+BEGIN: closure-widget :name who\nthe global one\n#+END:\n\
               #+BEGIN: closure-widget :name greet :inputs who\nhello {{who}}\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{greet who=local}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("hello local"), "{out}");
    assert!(!out.contains("hello the global one"), "{out}");
}

#[test]
fn without_an_argument_a_name_is_still_a_widget() {
    // Everything that worked before this went on working.
    let src = "#+BEGIN: closure-widget :name who\nthe global one\n#+END:\n\
               #+BEGIN: closure-widget :name greet\nhello {{who}}\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{greet}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("hello the global one"), "{out}");
}

#[test]
fn arguments_reach_through_a_nested_call() {
    // `page` passes to `outer`, which passes its own value on to
    // `inner`. Arguments are per call, not per widget.
    let src = "#+BEGIN: closure-widget :name inner :inputs x\n[{{x}}]\n#+END:\n\
               #+BEGIN: closure-widget :name outer :inputs x\n{{inner x={{x}}}}\n#+END:\n\
               #+BEGIN: closure-widget :name page\n{{outer x=deep}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("[deep]"), "{out}");
}

#[test]
fn a_name_that_is_neither_an_argument_nor_a_widget_is_still_an_error() {
    let src = "#+BEGIN: closure-widget :name page\n{{nope who=x}}\n#+END:\n";
    assert_eq!(
        expand_widgets(src),
        Err(WidgetError::Unknown("nope".to_owned()))
    );
}

#[test]
fn a_declared_input_with_nothing_bound_expands_to_nothing() {
    // A widget block is its own call site with no arguments, so its
    // parameters have no values there. Empty rather than an error: the
    // template is being shown, not used.
    let src = "#+BEGIN: closure-widget :name greet :inputs who\nhello {{who}}\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("hello"), "{out}");
    assert!(!out.contains("{{who}}"), "the parameter leaked: {out}");
}

#[test]
fn an_undeclared_name_is_still_looked_up_as_a_widget() {
    // Which is what keeps a typo an error instead of an empty string.
    let src = "#+BEGIN: closure-widget :name greet\nhello {{whoo}}\n#+END:\n";
    assert_eq!(
        expand_widgets(src),
        Err(WidgetError::Unknown("whoo".to_owned()))
    );
}
