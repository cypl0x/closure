//! Org macros: `#+MACRO: name text` and `{{{name}}}`.
//!
//! Preserved and never expanded, so a document that factored a phrase
//! out into a macro showed the macro.
//!
//! Close enough to closure's own widgets to be worth keeping apart on
//! purpose. A widget is `{{name}}` with two braces, has typed inputs,
//! slots and a vault-wide registry; an org macro is `{{{name}}}` with
//! three, is defined by a keyword in the same file, and takes
//! positional arguments. Sharing an expander between them is the
//! obvious mistake: one of the two would quietly acquire the other's
//! rules, and org's meaning is not closure's to change.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::expand_org_macros;

const DOC: &str = "\
#+MACRO: project closure
#+MACRO: greet Hello, $1 and $2!
* Notes
:PROPERTIES:
:ID: 01MACRO0000000000000001
:END:
";

#[test]
fn a_macro_is_replaced_by_its_text() {
    let out = expand_org_macros("built with {{{project}}} today", DOC);
    assert_eq!(out, "built with closure today");
}

#[test]
fn positional_arguments_are_substituted() {
    let out = expand_org_macros("{{{greet(world,friend)}}}", DOC);
    assert_eq!(out, "Hello, world and friend!");
}

#[test]
fn a_macro_nobody_defined_is_left_alone() {
    // Same rule as an unknown entity: leaving the source is honest,
    // and a blank is a wrong answer that looks like an empty one.
    let out = expand_org_macros("a {{{nosuch}}} here", DOC);
    assert_eq!(out, "a {{{nosuch}}} here");
}

#[test]
fn a_widget_reference_is_not_a_macro() {
    // Two braces are closure's, three are org's. An expander that
    // cannot tell them apart gives one of them the other's rules.
    let out = expand_org_macros("a {{card}} and a {{{project}}}", DOC);
    assert_eq!(out, "a {{card}} and a closure");
}

#[test]
fn text_with_no_macros_comes_back_identical() {
    let src = "just prose, with { braces } in it";
    assert_eq!(expand_org_macros(src, DOC), src);
}

#[test]
fn a_macro_that_expands_to_a_macro_does_not_loop() {
    // Org expands macros once; a recursive definition must end rather
    // than spin, which is the same bound every other expander here has.
    let doc = "#+MACRO: a {{{b}}}\n#+MACRO: b {{{a}}}\n";
    let out = expand_org_macros("{{{a}}}", doc);
    assert!(out.len() < 200, "it ran away: {out}");
}

#[test]
fn several_on_one_line() {
    let out = expand_org_macros("{{{project}}} and {{{project}}}", DOC);
    assert_eq!(out, "closure and closure");
}
