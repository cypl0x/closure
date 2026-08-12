//! `H_2O` and `x^2` — subscript and superscript.
//!
//! The last of org's inline markup closure preserved and never read.
//! In a notebook that holds chemistry, maths or any units at all, the
//! difference between `x^2` and the three characters `x`, `^`, `2` is
//! the difference between a formula and a typo.
//!
//! The whole difficulty is in what is *not* one. Org's rule is that a
//! bare word after the marker is the script, `{...}` groups a longer
//! one, and — the case that matters most here — a marker inside a word
//! with no letters or digits after it is ordinary text. `foo_bar` is a
//! variable name in every `snake_case` identifier ever written, and
//! marking its `bar` as a subscript would turn most source blocks and
//! half the file paths in a vault into typography.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{ScriptKind, scripts};

#[test]
fn a_superscript_is_found() {
    let got = scripts("x^2 + 1");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].kind, ScriptKind::Super);
    assert_eq!(got[0].text, "2");
}

#[test]
fn a_subscript_is_found() {
    let got = scripts("x_1 is the first");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].kind, ScriptKind::Sub);
    assert_eq!(got[0].text, "1");
}

#[test]
fn a_bare_script_runs_to_the_end_of_the_word_the_way_org_does() {
    // `H_2O` subscripts "2O", not "2". This is org's actual rule and it
    // is why chemistry gets written `H_{2}O` — the test that assumed
    // otherwise was wrong about org, not about the code, and copying
    // the assumption into closure would have made the same file render
    // two ways in two editors.
    let got = scripts("H_2O is water");
    assert_eq!(got[0].text, "2O", "{got:?}");
    let got = scripts("H_{2}O is water");
    assert_eq!(got[0].text, "2", "{got:?}");
}

#[test]
fn braces_group_a_longer_script() {
    let got = scripts("e^{i pi} + 1 = 0");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].text, "i pi");
}

#[test]
fn snake_case_is_not_typography() {
    // The case this feature lives or dies on. `foo_bar` is a variable
    // name, and a vault is full of them — in source blocks, in file
    // names, in property keys.
    assert!(scripts("foo_bar_baz").is_empty());
    assert!(scripts("let some_value = 1;").is_empty());
}

#[test]
fn a_marker_with_nothing_after_it_is_not_a_script() {
    assert!(scripts("a trailing ^").is_empty());
    assert!(scripts("x_ ").is_empty());
}

#[test]
fn a_marker_at_the_start_of_a_word_is_not_a_script() {
    // A script attaches to something. `_foo` is a leading underscore,
    // which is a name in most languages and emphasis in none of org's.
    assert!(scripts("_private and ^caret").is_empty());
}

#[test]
fn several_on_one_line() {
    let got = scripts("a^2 + b^2 = c^2");
    assert_eq!(got.len(), 3, "{got:?}");
}

#[test]
fn the_range_covers_the_marker_and_the_script() {
    let line = "x^2";
    let got = scripts(line);
    assert_eq!(&line[got[0].range.clone()], "^2");
}

#[test]
fn an_unclosed_brace_is_not_a_script() {
    // I5: malformed input yields no reading, never a panic and never a
    // run to the end of the line.
    assert!(scripts("e^{i pi").is_empty());
}
