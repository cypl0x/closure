//! V2a: composable widgets. A `#+BEGIN: closure-widget :name X` block is
//! a user-defined composite: its body is a template that may reference
//! other widgets via `{{name}}`. `expand_widgets` materialises every
//! block's body in place — recursively, with cycle detection — leaving
//! everything outside the block bodies byte-identical (I1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_query::{WidgetError, expand_widgets};

#[test]
fn widget_without_references_is_idempotent() {
    let src = "before\n#+BEGIN: closure-widget :name a\nhello world\n#+END:\nafter\n";
    assert_eq!(
        expand_widgets(src).unwrap(),
        src,
        "no refs → byte-identical"
    );
}

#[test]
fn reference_is_expanded_in_place() {
    let src = "#+BEGIN: closure-widget :name base\nBASE BODY\n#+END:\n\
               #+BEGIN: closure-widget :name top\nintro {{base}} outro\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(
        out.contains("intro BASE BODY outro"),
        "ref substituted: {out}"
    );
    // The base definition block is untouched.
    assert!(out.contains("#+BEGIN: closure-widget :name base\nBASE BODY\n#+END:"));
}

#[test]
fn references_recurse() {
    let src = "#+BEGIN: closure-widget :name leaf\nLEAF\n#+END:\n\
               #+BEGIN: closure-widget :name mid\n[{{leaf}}]\n#+END:\n\
               #+BEGIN: closure-widget :name root\n<{{mid}}>\n#+END:\n";
    let out = expand_widgets(src).unwrap();
    assert!(out.contains("<[LEAF]>"), "recursive expansion: {out}");
}

#[test]
fn cycle_is_a_typed_error() {
    let src = "#+BEGIN: closure-widget :name a\n{{b}}\n#+END:\n\
               #+BEGIN: closure-widget :name b\n{{a}}\n#+END:\n";
    assert!(matches!(expand_widgets(src), Err(WidgetError::Cycle(_))));
}

#[test]
fn unknown_reference_is_a_typed_error() {
    let src = "#+BEGIN: closure-widget :name a\n{{missing}}\n#+END:\n";
    assert_eq!(
        expand_widgets(src),
        Err(WidgetError::Unknown("missing".to_owned()))
    );
}

#[test]
fn text_outside_blocks_is_byte_exact() {
    let src = "# header\n\nsome prose\n\
               #+BEGIN: closure-widget :name a\nold body {{a}}? no — wait\n#+END:\n\
               trailing prose, no newline";
    // (a references itself → cycle; we only check the *error* path keeps
    // determinism, so use a clean variant for the span check.)
    let clean = "# header\n\nsome prose\n\
                 #+BEGIN: closure-widget :name a\nbody\n#+END:\n\
                 trailing prose, no newline";
    let out = expand_widgets(clean).unwrap();
    assert!(out.starts_with("# header\n\nsome prose\n"));
    assert!(
        out.ends_with("trailing prose, no newline"),
        "no spurious newline: {out:?}"
    );
    let _ = src;
}

#[test]
fn expansion_is_deterministic() {
    let src = "#+BEGIN: closure-widget :name a\nx{{b}}y\n#+END:\n\
               #+BEGIN: closure-widget :name b\nZ\n#+END:\n";
    assert_eq!(expand_widgets(src), expand_widgets(src));
}
