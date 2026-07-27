//! `#+BEGIN_…`/`#+END_…` blocks are greater elements.
//!
//! Org reads the whole pair as one element: nothing inside is
//! classified, which is what lets an example block hold org syntax and
//! a quote block hold a starred line. closure honoured that for
//! `#+BEGIN_SRC` alone, so `* x` inside `#+BEGIN_EXAMPLE` parsed as a
//! *headline* — a file written in Emacs came apart on the way in, and
//! the outline grew a phantom row nothing in the vault put there.
//!
//! Byte-exactness (I1) is the other half: a block is carried as one
//! span, so printing the document back gives the file it came from.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{NodeKind, parse, print};

/// The block kinds org defines, minus `SRC` (which has its own node).
const KINDS: [(&str, &str); 6] = [
    ("EXAMPLE", "#+BEGIN_EXAMPLE"),
    ("QUOTE", "#+BEGIN_QUOTE"),
    ("VERSE", "#+BEGIN_VERSE"),
    ("CENTER", "#+BEGIN_CENTER"),
    ("COMMENT", "#+BEGIN_COMMENT"),
    ("EXPORT", "#+BEGIN_EXPORT html"),
];

#[test]
fn a_starred_line_inside_a_block_is_not_a_headline() {
    for (name, open) in KINDS {
        let src = format!("* Real\n{open}\n* not a headline\n#+END_{name}\n");
        let doc = parse(&src).expect("parse");
        assert_eq!(
            doc.roots().len(),
            1,
            "{name} content must not split the outline"
        );
        assert_eq!(doc.roots()[0].title(), "Real");
    }
}

#[test]
fn a_block_is_one_node_of_its_own_kind() {
    let doc = parse("#+BEGIN_QUOTE\nlonely\n#+END_QUOTE\n").expect("parse");
    let kinds: Vec<NodeKind> = doc.preamble().iter().map(closure_org::Node::kind).collect();
    assert_eq!(kinds, vec![NodeKind::Block], "one node, not three lines");
}

#[test]
fn a_block_reports_its_name_and_content() {
    let doc = parse("#+BEGIN_QUOTE\nline one\nline two\n#+END_QUOTE\n").expect("parse");
    let block = doc.preamble()[0].as_block().expect("a block view");
    assert_eq!(block.name, "QUOTE");
    assert_eq!(block.content, "line one\nline two\n");
}

#[test]
fn an_export_block_keeps_its_arguments() {
    let doc = parse("#+BEGIN_EXPORT html\n<b>hi</b>\n#+END_EXPORT\n").expect("parse");
    let block = doc.preamble()[0].as_block().expect("a block view");
    assert_eq!(block.name, "EXPORT");
    assert_eq!(block.args, Some("html"));
}

#[test]
fn a_block_with_no_arguments_reports_none() {
    let doc = parse("#+BEGIN_QUOTE\nx\n#+END_QUOTE\n").expect("parse");
    assert_eq!(doc.preamble()[0].as_block().expect("view").args, None);
}

#[test]
fn the_delimiters_match_case_insensitively() {
    let doc = parse("#+begin_quote\n* x\n#+END_quote\n").expect("parse");
    assert!(doc.roots().is_empty(), "no headline escaped the block");
    assert_eq!(doc.preamble()[0].as_block().expect("view").name, "quote");
}

#[test]
fn a_src_block_is_still_a_code_block() {
    // The dedicated node stays: babel, edit-special and the block list
    // all address it, and `as_block` must not shadow it.
    let doc = parse("#+BEGIN_SRC rust\nfn x() {}\n#+END_SRC\n").expect("parse");
    assert_eq!(doc.preamble()[0].kind(), NodeKind::CodeBlock);
    assert_eq!(doc.code_blocks().len(), 1);
}

#[test]
fn an_unclosed_block_falls_through_to_the_line_classifier() {
    // A half-typed block is every editing session's intermediate
    // state; swallowing the rest of the file would be worse than
    // reading the delimiter as the keyword it looks like.
    let doc = parse("#+BEGIN_QUOTE\n* still a headline\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
}

#[test]
fn a_mismatched_end_does_not_close_the_block() {
    let src = "#+BEGIN_QUOTE\n* x\n#+END_EXAMPLE\n#+END_QUOTE\n";
    let doc = parse(src).expect("parse");
    assert!(doc.roots().is_empty(), "only its own END closes it");
}

#[test]
fn blocks_print_back_byte_for_byte() {
    for src in [
        "#+BEGIN_QUOTE\n* x\n#+END_QUOTE\n",
        "* H\n#+BEGIN_EXAMPLE\n,* escaped\n#+END_EXAMPLE\nafter\n",
        "#+begin_verse\na\n\nb\n#+end_verse\n",
        "#+BEGIN_QUOTE\n#+END_QUOTE\n",
        "#+BEGIN_QUOTE\n* x\n",
    ] {
        assert_eq!(print(&parse(src).expect("parse")), src, "roundtrip {src:?}");
    }
}

#[test]
fn a_block_inside_a_headline_stays_in_its_body() {
    let src = "* H\n#+BEGIN_QUOTE\n* inner\n#+END_QUOTE\n* Sibling\n";
    let doc = parse(src).expect("parse");
    let titles: Vec<&str> = doc
        .roots()
        .iter()
        .map(closure_org::Headline::title)
        .collect();
    assert_eq!(titles, vec!["H", "Sibling"], "no phantom between them");
}
