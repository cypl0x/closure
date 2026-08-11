//! "I9 says config errors fail the load. A widget's declared inputs are
//! config. A missing or wrong-typed argument is an error before render,
//! not a hole in the output."
//!
//! An argument was a string that got substituted wherever its name
//! appeared, so `{{card count=banana}}` rendered "banana" into a column
//! meant for a number and `{{card titel=Today}}` rendered nothing at
//! all and said nothing about why. Both are the failure I9 exists to
//! prevent: a mistake that turns into content instead of a message.
//!
//! So an input may carry a type — `:inputs count:number,done:bool` —
//! and the call site is checked against it. Untyped stays text, which
//! is what every input written before today is.
//!
//! Missing is deliberately *not* an error. A widget block is its own
//! call site with no arguments, which is how a definition shows itself,
//! so a parameter nobody bound expands to nothing. What is an error is
//! naming an input that does not exist, or giving one a value it cannot
//! hold — the two mistakes that are always mistakes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::{WidgetError, expand_widgets};

/// A document defining `card` with `inputs`, called with `args`.
///
/// The body reads back exactly what was declared — an undeclared name
/// is a widget lookup, so a template mentioning one would fail for a
/// reason that has nothing to do with types.
fn doc(inputs: &str, args: &str) -> String {
    let body: String = inputs
        .split(',')
        .map(|i| {
            let name = i.split(':').next().unwrap_or(i).trim();
            format!("[{{{{{name}}}}}]")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "#+BEGIN: closure-widget :name card :inputs {inputs}\n{body}\n#+END:\n\
         #+BEGIN: closure-widget :name page\n{{{{card {args}}}}}\n#+END:\n"
    )
}

#[test]
fn a_number_input_takes_a_number() {
    let out = expand_widgets(&doc("count:number", "count=42")).unwrap();
    assert!(out.contains("[42]"), "{out}");
}

#[test]
fn a_number_input_refuses_a_word() {
    let err = expand_widgets(&doc("count:number", "count=banana")).unwrap_err();
    let WidgetError::BadArgument {
        widget,
        input,
        expected,
        got,
    } = err
    else {
        panic!("wrong error: {err:?}");
    };
    // Everything needed to fix it, without opening another file.
    assert_eq!(widget, "card");
    assert_eq!(input, "count");
    assert_eq!(expected, "number");
    assert_eq!(got, "banana");
}

#[test]
fn a_bool_input_takes_true_and_false_and_nothing_else() {
    assert!(
        expand_widgets(&doc("done:bool", "done=true"))
            .unwrap()
            .contains("[true]")
    );
    assert!(expand_widgets(&doc("done:bool", "done=false")).is_ok());
    assert!(matches!(
        expand_widgets(&doc("done:bool", "done=yes")),
        Err(WidgetError::BadArgument { .. })
    ));
}

#[test]
fn an_untyped_input_still_takes_anything() {
    // Every widget written before today declares its inputs this way.
    let out = expand_widgets(&doc("title", "title=banana")).unwrap();
    assert!(out.contains("[banana]"), "{out}");
}

#[test]
fn an_argument_that_names_nothing_is_an_error() {
    // The typo that used to render as silence.
    let err = expand_widgets(&doc("title", "titel=Today")).unwrap_err();
    let WidgetError::UnknownArgument { widget, argument } = err else {
        panic!("wrong error: {err:?}");
    };
    assert_eq!(widget, "card");
    assert_eq!(argument, "titel");
}

#[test]
fn a_missing_argument_is_not_an_error() {
    // A definition block is a call site with no arguments; showing a
    // template is not a mistake.
    let src = "#+BEGIN: closure-widget :name card :inputs count:number\n\
               [{{count}}]\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("[]"), "{out}");
}

#[test]
fn the_error_arrives_instead_of_the_render_not_beside_it() {
    // A half-rendered document with a bad value in it is the failure
    // mode I9 is about: the mistake becomes content.
    assert!(expand_widgets(&doc("count:number", "count=banana")).is_err());
}

#[test]
fn several_inputs_can_be_typed_at_once() {
    let out = expand_widgets(&doc(
        "count:number,title,done:bool",
        "count=7 title=\"a note\" done=false",
    ))
    .unwrap();
    assert!(out.contains("[7] [a note] [false]"), "{out}");
}
