//! `#+CALL: setup()` runs the block it names.
//!
//! `call_target` read the line and nothing evaluated it, which is the
//! half of a feature that looks finished from a conformance matrix and
//! does nothing for a reader. Noweb assembles a named block into
//! another; this evaluates one from somewhere else, and between them
//! that is what "reusable block" means.
//!
//! It runs through the same backend and the same trust check as any
//! other block, because a call that could run code an ordinary block
//! could not would be a way around the check rather than a feature.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{CallError, run_call};

const DOC: &str = "\
#+NAME: greet
#+BEGIN_SRC sh
echo hello from the named block
#+END_SRC

#+CALL: greet()
";

#[test]
fn it_runs_the_block_the_line_names() {
    let out = run_call("#+CALL: greet()", DOC, &["sh".to_owned()]).expect("it ran");
    assert!(out.stdout.contains("hello from the named block"), "{out:?}");
}

#[test]
fn a_line_that_is_not_a_call_is_not_run() {
    assert!(matches!(
        run_call("echo not a call", DOC, &["sh".to_owned()]),
        Err(CallError::NotACall)
    ));
}

#[test]
fn a_call_to_a_name_that_is_not_there_says_which() {
    let err = run_call("#+CALL: nosuch()", DOC, &["sh".to_owned()]).unwrap_err();
    assert!(format!("{err}").contains("nosuch"), "{err}");
}

#[test]
fn an_untrusted_language_is_refused_like_any_other_block() {
    // The whole point of routing through the same check: a call must
    // not be a way to run what an ordinary block may not.
    let err = run_call("#+CALL: greet()", DOC, &[]).unwrap_err();
    assert!(matches!(err, CallError::NotTrusted(_)), "{err:?}");
}

#[test]
fn a_named_block_can_itself_use_noweb() {
    // The two halves compose: a called block is assembled before it is
    // run, so `<<setup>>` inside it means what it means anywhere else.
    let doc = "#+NAME: setup\n#+BEGIN_SRC sh\necho prepared\n#+END_SRC\n\
               #+NAME: main\n#+BEGIN_SRC sh\n<<setup>>\necho done\n#+END_SRC\n";
    let out = run_call("#+CALL: main()", doc, &["sh".to_owned()]).expect("it ran");
    assert!(out.stdout.contains("prepared"), "{out:?}");
    assert!(out.stdout.contains("done"), "{out:?}");
}
