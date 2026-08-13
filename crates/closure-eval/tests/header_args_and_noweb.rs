//! Header args and noweb, given input that is not tidy.
//!
//! These are the parts of `closure-eval` a reader touches directly: the
//! `:var`/`:results`/`:tangle` line on a block, and `<<name>>`
//! references between blocks. Both are parsers, and 148 of this crate's
//! 663 lines were unexecuted — the arms for input that does not match
//! the example in the manual.
//!
//! Neither runs anything, so all of it is hermetic.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{HeaderArgs, expand_noweb, var_prelude};

#[test]
fn a_header_line_that_is_empty_parses_to_the_defaults() {
    let h = HeaderArgs::parse("");
    let d = HeaderArgs::default();
    assert_eq!(format!("{h:?}"), format!("{d:?}"));
}

#[test]
fn unknown_directives_are_ignored_rather_than_refused() {
    // Forward compatibility, and it is the right call: org gains header
    // args, and a block using one closure has not learned yet should
    // still run rather than fail to parse.
    for raw in [
        ":nosuchdirective yes",
        ":exports both :nosuch thing",
        ":results value :future-thing 3",
    ] {
        let _ = HeaderArgs::parse(raw);
    }
}

#[test]
fn a_directive_with_no_value_does_not_take_the_next_one() {
    // `:var :results silent` — the `:var` is missing its binding, and
    // reading `:results` as that binding would silently drop the
    // results directive *and* invent a variable called `:results`.
    let h = HeaderArgs::parse(":var :results silent");
    let text = format!("{h:?}");
    assert!(
        !text.contains(":results\""),
        "a bare `:var` swallowed the next directive: {text}"
    );
}

#[test]
fn every_shape_of_header_line_parses_without_panicking() {
    for raw in [
        ":var",
        ":var x",
        ":var x=1",
        ":var x=1 :var y=2",
        ":var x=has spaces",
        ":var x=",
        ":tangle",
        ":tangle no",
        ":tangle out.sh",
        ":results",
        ":results silent",
        ":results value verbatim",
        ":exports none",
        "   ",
        ":::",
        ":var x=1:y=2",
        "-var x=1",
    ] {
        let _ = HeaderArgs::parse(raw);
    }
}

#[test]
fn a_var_prelude_is_produced_for_the_languages_that_have_one() {
    let vars = vec![("x".to_owned(), "1".to_owned())];
    for lang in ["sh", "bash", "python", "ruby", "js", "node", "nosuchlang"] {
        let _ = var_prelude(lang, &vars);
    }
    // The shell one at least has to mention the name, or a block's
    // `:var` is decoration.
    assert!(var_prelude("sh", &vars).contains('x'));
}

#[test]
fn a_var_value_with_a_quote_in_it_does_not_break_the_prelude() {
    // The prelude is source code. A value carrying a quote or a newline
    // must not end the assignment early — that is how a `:var` becomes
    // an injection rather than a binding.
    let vars = vec![("x".to_owned(), "he said \"hi\"; rm -rf /".to_owned())];
    let out = var_prelude("sh", &vars);
    assert!(
        !out.contains("; rm -rf /\n"),
        "a value escaped its quoting: {out}"
    );
}

#[test]
fn noweb_with_nothing_to_expand_returns_the_source() {
    let src = "echo hello\n";
    assert_eq!(expand_noweb(src, "").expect("no refs"), src);
}

#[test]
fn a_reference_to_a_block_that_is_not_there_is_an_error() {
    // Silently leaving `<<nosuch>>` in the source would run it as a
    // shell redirection, which is a very different program.
    assert!(expand_noweb("<<nosuch>>\n", "").is_err());
}

#[test]
fn a_ring_of_references_ends_rather_than_spins() {
    // The bound every expander in this codebase has (I5).
    let doc = "#+NAME: a\n#+BEGIN_SRC sh\n<<b>>\n#+END_SRC\n\
               #+NAME: b\n#+BEGIN_SRC sh\n<<a>>\n#+END_SRC\n";
    // An error is an equally correct answer to a cycle; running away
    // is not.
    if let Ok(text) = expand_noweb("<<a>>\n", doc) {
        assert!(text.len() < 10_000, "it ran away: {} bytes", text.len());
    }
}

#[test]
fn noweb_survives_input_that_is_not_a_reference() {
    for src in ["<<", ">>", "<<>>", "<< >>", "a << b >> c", "<<<a>>>", "<<a"] {
        let _ = expand_noweb(src, "#+NAME: a\n#+BEGIN_SRC sh\necho\n#+END_SRC\n");
    }
}
