//! Noweb references and `#+CALL:` — literate programming's two halves.
//!
//! `<<name>>` inside a source block means "the block called `name`
//! goes here". It is what lets a literate document explain a program
//! in the order a reader needs and assemble it in the order the
//! compiler needs, which is the entire argument for writing one. Both
//! were preserved and neither was resolved, so tangling a literate
//! file produced a file with `<<setup>>` in it.
//!
//! `#+CALL: name()` is the same idea for evaluation rather than
//! assembly: run a named block from somewhere else.
//!
//! Same rules as every other composition here: it is a view, a cycle
//! names its ring, and a depth limit catches a nest that is deep
//! without being circular.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{NowebError, call_target, expand_noweb};

const DOC: &str = "\
#+NAME: setup
#+BEGIN_SRC sh
echo setting up
#+END_SRC

#+NAME: main
#+BEGIN_SRC sh
<<setup>>
echo main
#+END_SRC
";

#[test]
fn a_reference_is_replaced_by_the_block_it_names() {
    let out = expand_noweb("<<setup>>\necho main\n", DOC).unwrap();
    assert!(out.contains("echo setting up"), "{out}");
    assert!(out.contains("echo main"), "{out}");
    assert!(!out.contains("<<setup>>"), "{out}");
}

#[test]
fn indentation_at_the_reference_is_kept() {
    // A noweb reference inside an indented block is being pasted into
    // a place where indentation is syntax — Python, YAML, a makefile.
    let doc = "#+NAME: body\n#+BEGIN_SRC python\nx = 1\ny = 2\n#+END_SRC\n";
    let out = expand_noweb("def f():\n    <<body>>\n", doc).unwrap();
    assert!(out.contains("    x = 1"), "{out}");
    assert!(out.contains("    y = 2"), "the second line lost it: {out}");
}

#[test]
fn a_reference_inside_a_referenced_block_resolves() {
    let doc = "#+NAME: a\n#+BEGIN_SRC sh\n<<b>>\n#+END_SRC\n\
               #+NAME: b\n#+BEGIN_SRC sh\ndeep\n#+END_SRC\n";
    assert!(expand_noweb("<<a>>\n", doc).unwrap().contains("deep"));
}

#[test]
fn a_cycle_names_its_ring() {
    let doc = "#+NAME: a\n#+BEGIN_SRC sh\n<<b>>\n#+END_SRC\n\
               #+NAME: b\n#+BEGIN_SRC sh\n<<a>>\n#+END_SRC\n";
    let NowebError::Cycle(ring) = expand_noweb("<<a>>\n", doc).expect_err("a cycle") else {
        panic!("not reported as a cycle");
    };
    assert_eq!(ring.first(), ring.last(), "{ring:?} is not a ring");
}

#[test]
fn a_reference_to_nothing_says_which_name() {
    let err = expand_noweb("<<nosuch>>\n", DOC).expect_err("unknown");
    assert!(format!("{err}").contains("nosuch"), "{err}");
}

#[test]
fn text_without_references_is_returned_as_it_was() {
    let src = "echo plain\n";
    assert_eq!(expand_noweb(src, DOC).unwrap(), src);
}

#[test]
fn angle_brackets_that_are_not_a_reference_are_left_alone() {
    // `a << b` is a shift, and `<<` at the start of a heredoc is not a
    // reference either. Only `<<name>>` alone on its line is.
    let src = "echo $((1 << 2))\n";
    assert_eq!(expand_noweb(src, DOC).unwrap(), src);
}

#[test]
fn a_call_line_names_the_block_it_runs() {
    assert_eq!(call_target("#+CALL: setup()").as_deref(), Some("setup"));
    assert_eq!(call_target("#+CALL: setup"), None, "org writes the parens");
    assert_eq!(call_target("not a call"), None);
}
