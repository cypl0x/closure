//! The corners of noweb expansion.
//!
//! `expand_noweb` is literate programming's load-bearing feature: it
//! pastes one named block into another, and what it produces is then
//! *executed*. Three of its failure modes had no test — the depth
//! limit, a block whose `#+NAME:` is the last thing in the file, and
//! the lowercase spelling of the keyword — and each one either runs the
//! wrong program or fails to terminate.
//!
//! The indentation rule is the one worth stating: the indent at the
//! reference is applied to every line replacing it, "because a block
//! being pasted into Python or YAML is being pasted where indentation
//! is syntax". Get that wrong and the expansion is a syntax error at
//! best and a differently-scoped program at worst.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_eval::{NOWEB_DEPTH_LIMIT, NowebError, expand_noweb};

/// A document defining `name` as a block containing `body`.
fn block(name: &str, body: &str) -> String {
    format!("#+NAME: {name}\n#+BEGIN_SRC sh\n{body}\n#+END_SRC\n\n")
}

#[test]
fn a_reference_alone_on_its_line_is_expanded() {
    let doc = block("greeting", "echo hello");
    assert_eq!(
        expand_noweb("<<greeting>>\n", &doc).expect("expand"),
        "echo hello\n"
    );
}

#[test]
fn a_shift_operator_is_not_a_reference() {
    // The rule the doc states: `a << b` is a shift, and a parser that
    // says otherwise finds references in arithmetic.
    let doc = block("x", "nope");
    for src in ["a << b\n", "let y = 1 << 3;\n", "echo <<x>> inline\n"] {
        assert_eq!(
            expand_noweb(src, &doc).expect("expand"),
            src,
            "{src:?} was treated as a reference"
        );
    }
}

#[test]
fn the_indent_at_the_reference_is_applied_to_every_line() {
    // Pasting into Python: the block's second line has to be indented
    // as far as the first, or the expansion changes what is inside the
    // `if`.
    let doc = block("body", "first()\nsecond()");
    let out = expand_noweb("if cond:\n    <<body>>\n", &doc).expect("expand");
    assert_eq!(out, "if cond:\n    first()\n    second()\n", "{out:?}");
}

#[test]
fn a_reference_to_a_block_that_is_not_there_names_it() {
    let err = expand_noweb("<<absent>>\n", "").expect_err("expanded a missing block");
    assert!(
        matches!(&err, NowebError::Unknown(n) if n == "absent"),
        "{err:?}"
    );
}

#[test]
fn a_block_that_includes_itself_is_a_cycle_not_a_hang() {
    let doc = block("loop", "<<loop>>");
    let err = expand_noweb("<<loop>>\n", &doc).expect_err("expanded a self-reference");
    assert!(matches!(err, NowebError::Cycle(_)), "{err:?}");
}

#[test]
fn two_blocks_that_include_each_other_are_a_cycle() {
    let doc = format!("{}{}", block("a", "<<b>>"), block("b", "<<a>>"));
    let err = expand_noweb("<<a>>\n", &doc).expect_err("expanded a ring");
    assert!(matches!(err, NowebError::Cycle(_)), "{err:?}");
}

#[test]
fn nesting_deeper_than_the_limit_is_refused_before_it_recurses_away() {
    // A chain rather than a ring: each block names the next, so no
    // cycle detector fires and only the depth limit stops it. Without
    // that limit this is a stack overflow, which takes the whole
    // process rather than returning an error.
    let mut doc = String::new();
    let depth = NOWEB_DEPTH_LIMIT + 5;
    for i in 0..depth {
        doc.push_str(&block(&format!("n{i}"), &format!("<<n{}>>", i + 1)));
    }
    doc.push_str(&block(&format!("n{depth}"), "echo done"));

    let err = expand_noweb("<<n0>>\n", &doc).expect_err("expanded past the depth limit");
    assert!(
        matches!(err, NowebError::TooDeep(n) if n == NOWEB_DEPTH_LIMIT),
        "{err:?}"
    );
}

#[test]
fn nesting_just_inside_the_limit_still_expands() {
    // The other side of the boundary, so the limit is not simply
    // refusing everything nested.
    let mut doc = String::new();
    for i in 0..5 {
        doc.push_str(&block(&format!("n{i}"), &format!("<<n{}>>", i + 1)));
    }
    doc.push_str(&block("n5", "echo done"));
    assert_eq!(
        expand_noweb("<<n0>>\n", &doc).expect("expand"),
        "echo done\n"
    );
}

#[test]
fn the_name_keyword_is_read_in_either_case() {
    // Org accepts `#+NAME:` and `#+name:`, and a file written by
    // another editor may use either. Reading only one spelling makes
    // half of everybody's files silently unreferenceable.
    let lower = "#+name: greeting\n#+BEGIN_SRC sh\necho hi\n#+END_SRC\n";
    assert_eq!(
        expand_noweb("<<greeting>>\n", lower).expect("expand"),
        "echo hi\n"
    );
}

#[test]
fn a_named_block_that_runs_to_the_end_of_the_file_still_expands() {
    // No `#+END_SRC` — a truncated file, or the last block in one
    // somebody is still writing. The body is whatever is left rather
    // than nothing.
    let doc = "#+NAME: tail\n#+BEGIN_SRC sh\necho last\n";
    let out = expand_noweb("<<tail>>\n", doc).expect("expand");
    assert!(out.contains("echo last"), "{out:?}");
}

#[test]
fn a_source_with_no_reference_comes_back_untouched() {
    // The early return. Cheap, and the common case by far.
    let doc = block("unused", "never");
    for src in ["", "echo plain\n", "no angle brackets here\n"] {
        assert_eq!(expand_noweb(src, &doc).expect("expand"), src);
    }
}

#[test]
fn a_reference_expands_more_than_once_when_it_appears_twice() {
    // Not a cycle: the same block used twice in one program is
    // ordinary literate programming, and a cycle detector that keyed
    // on "seen before" rather than "on the stack" would refuse it.
    let doc = block("step", "echo step");
    let out = expand_noweb("<<step>>\n<<step>>\n", &doc).expect("expand");
    assert_eq!(out, "echo step\necho step\n", "{out:?}");
}
