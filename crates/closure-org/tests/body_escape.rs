//! Org's comma escape for body lines that would otherwise parse as
//! headlines.
//!
//! A body is just the text between a headline and the next one, so a
//! line starting with `*` at column zero *is* a headline — writing one
//! into a body silently splits the outline and reparents whatever
//! followed. Org's own answer is the leading comma (`,* not a
//! headline`), which the reader strips again; these are the two halves
//! of that bijection.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{escape_body, unescape_body};

#[test]
fn a_headline_lookalike_gains_a_comma() {
    assert_eq!(escape_body("* Foo\n"), ",* Foo\n");
    assert_eq!(escape_body("** Bar\n"), ",** Bar\n");
    assert_eq!(escape_body("*\n"), ",*\n");
}

#[test]
fn ordinary_prose_is_untouched() {
    assert_eq!(escape_body("plain\n"), "plain\n");
    assert_eq!(escape_body("  * a list item\n"), "  * a list item\n");
    assert_eq!(escape_body("a * b\n"), "a * b\n");
    assert_eq!(escape_body("#+BEGIN_SRC rust\n"), "#+BEGIN_SRC rust\n");
    assert_eq!(escape_body("*bold* text\n"), "*bold* text\n");
}

#[test]
fn an_already_escaped_line_gains_another_comma() {
    // Otherwise the round trip is not a bijection: the reader would
    // turn a literal `,* x` into `* x`.
    assert_eq!(escape_body(",* Foo\n"), ",,* Foo\n");
    assert_eq!(escape_body(",,* Foo\n"), ",,,* Foo\n");
    assert_eq!(escape_body(",plain\n"), ",plain\n", "no star, no escape");
}

#[test]
fn unescape_undoes_escape() {
    for body in [
        "* Foo\n",
        "** Bar\n** Baz\n",
        ",* already\n",
        "plain\n* mid\nplain\n",
        "",
        "no trailing newline",
        "*",
    ] {
        assert_eq!(
            unescape_body(&escape_body(body)),
            body,
            "round trip for {body:?}"
        );
    }
}

#[test]
fn unescape_leaves_unescaped_text_alone() {
    assert_eq!(unescape_body("plain\n"), "plain\n");
    assert_eq!(unescape_body(",plain\n"), ",plain\n");
    assert_eq!(unescape_body("  ,* indented\n"), "  ,* indented\n");
}

#[test]
fn escaping_is_idempotent_under_unescape() {
    // Reading a hand-written org file that already uses the escape
    // gives the text the author meant.
    assert_eq!(unescape_body(",* Foo\n"), "* Foo\n");
    assert_eq!(unescape_body(",,* Foo\n"), ",* Foo\n");
}

// === Block interiors ===
//
// A `#+BEGIN_…`/`#+END_…` pair is a greater element: the parser does
// not read a starred line inside one as a headline, so escaping there
// is not needed to keep the outline intact. It is also actively
// wrong — the comma lands in the file, `code_blocks()` hands the block
// content back verbatim, and `eval-block` then runs `,* x`.

#[test]
fn a_starred_line_inside_a_src_block_is_left_verbatim() {
    let body = "#+BEGIN_SRC sh\n* ) echo default;;\n#+END_SRC\n";
    assert_eq!(escape_body(body), body, "the comma would reach the shell");
}

#[test]
fn every_block_kind_protects_its_content() {
    for (open, close) in [
        ("#+BEGIN_SRC rust", "#+END_SRC"),
        ("#+BEGIN_EXAMPLE", "#+END_EXAMPLE"),
        ("#+BEGIN_QUOTE", "#+END_QUOTE"),
        ("#+BEGIN_VERSE", "#+END_VERSE"),
        ("#+begin_export html", "#+end_export"),
    ] {
        let body = format!("{open}\n* x\n{close}\n");
        assert_eq!(escape_body(&body), body, "{open} content is verbatim");
    }
}

#[test]
fn escaping_resumes_after_the_block_closes() {
    let body = "* before\n#+BEGIN_SRC sh\n* inside\n#+END_SRC\n* after\n";
    assert_eq!(
        escape_body(body),
        ",* before\n#+BEGIN_SRC sh\n* inside\n#+END_SRC\n,* after\n"
    );
}

#[test]
fn an_unclosed_block_still_protects_the_rest() {
    // A half-typed block is the common case while editing; escaping
    // what follows would rewrite the code under the cursor.
    let body = "#+BEGIN_SRC sh\n* x\n";
    assert_eq!(escape_body(body), body);
}

#[test]
fn a_comma_inside_a_block_is_content_not_an_escape() {
    // The escape does not apply there, so neither does the strip: a
    // literal `,* x` in code must survive being read back.
    let body = "#+BEGIN_SRC sh\n,* x\n#+END_SRC\n";
    assert_eq!(unescape_body(body), body);
}

#[test]
fn blocks_round_trip_through_both_halves() {
    for body in [
        "#+BEGIN_SRC sh\n* x\n#+END_SRC\n",
        "* a\n#+BEGIN_QUOTE\n* b\n#+END_QUOTE\n* c\n",
        "#+BEGIN_SRC sh\n,* x\n#+END_SRC\n,* real escape\n",
        "#+BEGIN_SRC\n#+END_SRC\n",
    ] {
        assert_eq!(unescape_body(&escape_body(body)), body, "{body:?}");
    }
}

#[test]
fn an_indented_block_delimiter_still_opens_a_block() {
    // Org allows leading whitespace on the delimiters (a block inside
    // a list item), and the content is protected just the same.
    let body = "  #+BEGIN_SRC sh\n* x\n  #+END_SRC\n";
    assert_eq!(escape_body(body), body);
}

#[test]
fn a_begin_line_that_is_not_a_block_does_not_open_one() {
    // `#+TITLE:` and friends are keywords, not blocks; a starred line
    // after one is still a body line that needs the escape.
    let body = "#+TITLE: t\n* x\n";
    assert_eq!(escape_body(body), "#+TITLE: t\n,* x\n");
}
